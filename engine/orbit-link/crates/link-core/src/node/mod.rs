//! The device engine. One UDP socket carries everything:
//!
//! - the encrypted session with the server, which lists the other devices,
//!   passes hole-punching messages and, on servers that offer it, relays
//!   traffic when no direct path works;
//! - the end-to-end encrypted session with each other device, sent directly
//!   once a path is found;
//! - path discovery pings and pongs ([`crate::disco`]).
//!
//! Decrypted packets go into an in-process network stack ([`stack`]). Runs a
//! single-threaded event loop.

pub mod device;
pub mod forward;
pub mod ftp;
pub mod peer;
pub mod server;
pub mod stack;
pub mod tcp;
pub mod udp;

use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::mpsc::Sender;
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::sys;
use boringtun::noise::handshake::parse_handshake_anon;
use boringtun::noise::rate_limiter::RateLimiter;
use boringtun::noise::{Packet, Tunn, TunnResult};
use boringtun::x25519::{PublicKey, StaticSecret};
use link_proto::{decode_frame, encode_frame, Frame, PathReport, PeerInfo, ToDevice, ToServer, HUB};

use crate::disco::{self, Message};
use crate::keys;
use crate::portmap::PortMapper;
use crate::state::{self, DeviceState, Forward};
pub use forward::ForwardStatus;
use peer::{PathChange, Peer, Route};
pub use server::Invite;
use server::{Me, ReplyTo, ServerLink, SERVER_SLOT};
use stack::Stack;

const BUFFER_SIZE: usize = 65536 + 256;
const TICK: Duration = Duration::from_millis(250);
/// Behind a strict router that picks ports at random, a device opens this many
/// extra UDP ports toward the other device for its random probes to find.
const SCATTER_SOCKETS: usize = 192;
const SCATTER_LIFETIME: Duration = Duration::from_secs(20);
const SCATTER_PING: Duration = Duration::from_secs(2);

/// Extra ports opened toward one device; the first one it reaches becomes this
/// device's socket.
struct Scatter {
    sockets: Vec<UdpSocket>,
    key: [u8; 32],
    target: SocketAddr,
    until: Instant,
    next_ping: Instant,
    /// The device reached our main socket too: keep that, never switch.
    main_heard: bool,
}
const SOCKET_BUFFER: i32 = 4 * 1024 * 1024;
const LOCALS_EVERY: Duration = Duration::from_secs(10);
const CACHE_DELAY: Duration = Duration::from_secs(5);
/// A device missing from this many of the server's lists in a row has left.
const MISSED_ROUNDS: u64 = 3;
/// Handshakes per second this device answers before asking for cookies.
const HANDSHAKE_RATE_LIMIT: u64 = 50;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum ServerState {
    #[default]
    Connecting,
    Connected,
    /// Was connected, then stopped answering. Direct paths keep working.
    Unreachable,
    /// This device was removed from its network.
    Removed,
}

impl ServerState {
    pub fn as_str(self) -> &'static str {
        match self {
            ServerState::Connecting => "connecting",
            ServerState::Connected => "connected",
            ServerState::Unreachable => "unreachable",
            ServerState::Removed => "removed",
        }
    }
}

#[derive(Clone, Debug)]
pub struct PeerStatus {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub address: Ipv4Addr,
    pub online: bool,
    /// `direct`, `relay` or `none`
    pub path: &'static str,
    pub endpoint: Option<SocketAddr>,
    pub rtt_ms: Option<u32>,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub last_handshake: Option<Duration>,
}

#[derive(Clone, Debug, Default)]
pub struct Status {
    pub server: ServerState,
    pub server_address: Option<SocketAddr>,
    /// This device's public address as the server sees it.
    pub public_endpoint: Option<SocketAddr>,
    pub relay: bool,
    pub local_port: u16,
    pub candidates: Vec<SocketAddr>,
    /// The port the home router forwards to this device (UPnP or NAT-PMP).
    pub port_mapping: Option<String>,
    pub peers: Vec<PeerStatus>,
    pub forwards: Vec<ForwardStatus>,
    pub tcp_connections: usize,
    pub udp_sessions: usize,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub last_error: Option<String>,
}

pub enum Command {
    Invite(Sender<Result<Invite, String>>),
    Remove(String, Sender<Result<(), String>>),
    Rename(String, String, Sender<Result<(), String>>),
    SetForwards(Vec<Forward>),
    SetAllowLoader(bool),
    /// Answered once every earlier command has taken effect.
    Sync(Sender<Result<(), String>>),
}

#[derive(Debug)]
pub enum Event {
    Connected { first: bool },
    Unreachable,
    Removed,
    Direct { name: String },
}

/// Shared between the engine thread and the application.
#[derive(Default)]
pub struct Shared {
    pub stop: AtomicBool,
    pub status: Mutex<Status>,
    commands: Mutex<Vec<Command>>,
}

impl Shared {
    pub fn send(&self, command: Command) {
        self.commands.lock().unwrap_or_else(|e| e.into_inner()).push(command);
    }

    pub fn status(&self) -> Status {
        self.status.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }
}

/// The socket and the server session: everything needed to send.
struct Net {
    socket: UdpSocket,
    server: ServerLink,
    address: Ipv4Addr,
    frame_buffer: Vec<u8>,
    rx_bytes: u64,
    tx_bytes: u64,
}

impl Net {
    fn send_to(&mut self, to: SocketAddr, data: &[u8]) {
        match self.socket.send_to(data, to) {
            Ok(n) => self.tx_bytes += n as u64,
            Err(e) if e.kind() == ErrorKind::WouldBlock => {} // dropped, as on a full link
            Err(_) => {}
        }
    }

    /// Sends a frame to the server inside the device's session.
    fn send_frame(&mut self, frame: &Frame) {
        let Some(server) = self.server.addr else { return };
        let packet = encode_frame(self.address, HUB, frame);
        if self.frame_buffer.len() < packet.len() + 148 {
            self.frame_buffer.resize(packet.len() + 148, 0);
        }
        if let TunnResult::WriteToNetwork(data) = self.server.tunn.encapsulate(&packet, &mut self.frame_buffer) {
            match self.socket.send_to(data, server) {
                Ok(n) => self.tx_bytes += n as u64,
                Err(_) => {}
            }
        }
    }

    fn send_control(&mut self, message: &ToServer) {
        if let Ok(json) = serde_json::to_vec(message) {
            self.send_frame(&Frame::Control(&json));
        }
    }

    /// Sends a device-to-device packet along `route`.
    fn send(&mut self, route: Route, peer: &[u8; 32], data: &[u8]) {
        match route {
            Route::Direct(addr) => self.send_to(addr, data),
            Route::Both(addr) => {
                self.send_to(addr, data);
                self.send_frame(&Frame::Relay { peer: *peer, payload: data });
            }
            Route::Relay => self.send_frame(&Frame::Relay { peer: *peer, payload: data }),
            Route::None => {}
        }
    }
}

pub struct Engine {
    secret: StaticSecret,
    public: PublicKey,
    /// Screens handshakes from unknown senders before the costly key exchange.
    limiter: RateLimiter,
    last_limiter_reset: Instant,
    me: [u8; 32],
    net: Net,
    peers: Vec<Peer>,
    next_slot: u32,
    round: u64,
    stack: Stack,
    /// Where this device can be reached besides the address the server sees:
    /// its local network address and a port its router forwards.
    locals: Vec<SocketAddr>,
    next_locals: Instant,
    portmap: Option<PortMapper>,
    cache_path: PathBuf,
    cache_due: Option<Instant>,
    out: Vec<u8>,
    port: u16,
    /// A "connected" notification was shown in this run.
    announced: bool,
    scatter: Option<Scatter>,
}

enum Target {
    Server,
    Peer(usize),
}

impl Engine {
    pub fn new(state: &DeviceState, data_dir: PathBuf, serve: bool) -> Result<Engine, String> {
        let secret = keys::parse_secret(&state.private_key)?;
        let public = PublicKey::from(&secret);
        let server_key = *keys::parse_public(&state.server_key)?.as_bytes();

        let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, link_proto::DEVICE_PORT))
            .or_else(|_| UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)))
            .map_err(|e| format!("cannot open a UDP socket: {e}"))?;
        socket.set_nonblocking(true).map_err(|e| e.to_string())?;
        crate::net::set_buffers(&socket, SOCKET_BUFFER);
        crate::sys::quiet_udp(&socket);
        let port = socket.local_addr().map_err(|e| e.to_string())?.port();
        crate::log!("engine: UDP {port} open");

        let mut stack = Stack::new(state.address, serve, state.allow_loader);
        stack.set_forwards(&state.forwards);
        crate::log!("engine: network stack ready");
        let server = ServerLink::new(&secret, server_key, &state.endpoint);
        let limiter = RateLimiter::new(&public, HANDSHAKE_RATE_LIMIT);
        crate::log!("engine: server session ready");
        let portmap = (std::env::var_os("ORBIT_LINK_NO_PORTMAP").is_none() && option_env!("ORBIT_LINK_NO_PORTMAP").is_none())
            .then(|| PortMapper::start(port));
        crate::log!("engine: port mapping {}", if portmap.is_some() { "started" } else { "off" });
        let mut engine = Engine {
            me: *public.as_bytes(),
            net: Net {
                socket,
                server,
                address: state.address,
                frame_buffer: vec![0; BUFFER_SIZE],
                rx_bytes: 0,
                tx_bytes: 0,
            },
            limiter,
            last_limiter_reset: Instant::now(),
            secret,
            public,
            peers: Vec::new(),
            next_slot: SERVER_SLOT + 1,
            round: 0,
            stack,
            locals: Vec::new(),
            next_locals: Instant::now(),
            // ORBIT_LINK_NO_PORTMAP at run time, or when building (the PS5 has no environment),
            // leaves the router alone: for tests on someone's home network.
            portmap,
            cache_path: data_dir.join(state::PEERS_FILE),
            cache_due: None,
            out: vec![0; BUFFER_SIZE],
            port,
            announced: false,
            scatter: None,
        };
        // Devices from the last run, so direct paths can start before the
        // server answers (or while it is down).
        if let Ok(Some(cached)) = state::load::<Vec<PeerInfo>>(&engine.cache_path) {
            let now = Instant::now();
            for info in cached {
                engine.upsert_peer(info, now);
            }
        }
        crate::log!("engine started on UDP {port} as {} in {}", state.address, state.network);
        Ok(engine)
    }

    /// Runs until `shared.stop` is set or the device is removed from its network.
    pub fn run(mut self, shared: &Shared, on_event: &dyn Fn(Event)) {
        crate::log!("engine: running");
        let mut wire = vec![0u8; BUFFER_SIZE];
        let mut last_tick = Instant::now() - TICK;
        let mut fds: Vec<sys::PollFd> = Vec::new();

        while !shared.stop.load(Ordering::Relaxed) && !self.net.server.removed {
            let now = Instant::now();
            if now.duration_since(last_tick) >= TICK {
                last_tick = now;
                self.commands(shared);
                self.locals_tick(now);
                self.server_tick(now, on_event);
                self.peers_tick(now, on_event);
                self.stack.expire();
                self.save_cache(now);
                self.publish(shared);
            }

            fds.clear();
            fds.push(sys::entry(&self.net.socket, sys::READABLE));
            if let Some(scatter) = &self.scatter {
                fds.extend(scatter.sockets.iter().map(|socket| sys::entry(socket, sys::READABLE)));
            }
            self.stack.poll_fds(&mut fds);
            let wait = self.stack.poll_delay(TICK.as_millis() as i64);
            sys::poll(&mut fds, wait as i32);

            self.receive(&mut wire);
            self.receive_scatter(&mut wire);
            let peers = &self.peers;
            let resolve = |key: &str| peers.iter().find(|p| p.info.key == key).map(|p| p.info.address);
            self.stack.io(&resolve);
            self.transmit();
        }
        if self.net.server.removed {
            on_event(Event::Removed);
        }
        self.net.server.fail_all("Orbit Link stopped.");
        self.publish(shared);
        crate::log!("engine stopped");
    }

    fn commands(&mut self, shared: &Shared) {
        let commands: Vec<Command> = std::mem::take(&mut *shared.commands.lock().unwrap_or_else(|e| e.into_inner()));
        for command in commands {
            match command {
                Command::Invite(tx) => self.net.server.add_request(|id| ToServer::Invite { id }, ReplyTo::Invite(tx)),
                Command::Remove(key, tx) => self.net.server.add_request(|id| ToServer::Remove { id, key }, ReplyTo::Done(tx)),
                Command::Rename(key, name, tx) => {
                    self.net.server.add_request(|id| ToServer::Rename { id, key, name }, ReplyTo::Done(tx))
                }
                Command::SetForwards(forwards) => self.stack.set_forwards(&forwards),
                Command::SetAllowLoader(allow) => self.stack.allow_loader = allow,
                Command::Sync(tx) => {
                    let _ = tx.send(Ok(()));
                }
            }
        }
    }

    /// This device's addresses on its local network.
    fn locals_tick(&mut self, now: Instant) {
        if now < self.next_locals {
            return;
        }
        self.next_locals = now + LOCALS_EVERY;
        let lan = crate::net::lan_address().map(|ip| SocketAddr::from((ip, self.port)));
        let mapped = self.portmap.as_ref().and_then(|p| p.current()).map(|m| SocketAddr::V4(m.external));
        let locals: Vec<SocketAddr> = lan.into_iter().chain(mapped).collect();
        if locals != self.locals {
            crate::log!("reachable at: {}", locals.iter().map(|a| a.to_string()).collect::<Vec<_>>().join(", "));
            if lan != self.locals.first().copied() {
                if let Some(portmap) = &self.portmap {
                    portmap.retry(); // a new network may have a different router
                }
                // Some systems (the PS5) invalidate sockets when the network changes:
                // a new local IP address, not just a new port of our own.
                let ip = |addr: Option<&SocketAddr>| addr.map(|a| a.ip());
                if lan.is_some() && self.locals.first().is_some() && ip(lan.as_ref()) != ip(self.locals.first()) {
                    self.reopen_socket();
                }
            }
            self.locals = locals;
            self.net.server.hello_soon();
            for peer in &mut self.peers {
                peer.start_search(now); // tell everyone where to find us now
            }
        }
    }

    /// Opens the UDP socket again on the same port (another port if that one is
    /// taken), after a network change.
    fn reopen_socket(&mut self) {
        let temporary = match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
            Ok(socket) => socket,
            Err(e) => return crate::log!("cannot reopen the UDP socket: {e}"),
        };
        // Close the old socket before binding its port again.
        drop(std::mem::replace(&mut self.net.socket, temporary));
        if let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, self.port)) {
            self.net.socket = socket;
        }
        let _ = self.net.socket.set_nonblocking(true);
        crate::net::set_buffers(&self.net.socket, SOCKET_BUFFER);
        sys::quiet_udp(&self.net.socket);
        if let Ok(address) = self.net.socket.local_addr() {
            self.port = address.port();
        }
        crate::log!("network changed: UDP {} reopened", self.port);
    }

    /// Where other devices can try to reach this one.
    fn my_candidates(&self) -> Vec<SocketAddr> {
        let mut candidates = Vec::new();
        if let Some(endpoint) = self.net.server.me.as_ref().and_then(|me| me.endpoint) {
            candidates.push(endpoint);
        }
        for local in &self.locals {
            if !candidates.contains(local) {
                candidates.push(*local);
            }
        }
        candidates
    }

    fn server_tick(&mut self, now: Instant, on_event: &dyn Fn(Event)) {
        let server = &mut self.net.server;
        server.resolve_if_needed(now);
        if server.should_force_handshake(now) {
            if let (Some(addr), TunnResult::WriteToNetwork(data)) =
                (server.addr, server.tunn.format_handshake_initiation(&mut self.out, true))
            {
                let _ = self.net.socket.send_to(data, addr);
            }
        }
        if let TunnResult::WriteToNetwork(data) = self.net.server.tunn.update_timers(&mut self.out) {
            if let Some(addr) = self.net.server.addr {
                let _ = self.net.socket.send_to(data, addr);
            }
        }
        if self.net.server.hello_due(now) {
            let relay = self.net.server.relay_available(now);
            let paths = self
                .peers
                .iter()
                .map(|p| PathReport {
                    key: p.info.key.clone(),
                    path: p.route(relay).name().to_string(),
                    rtt_ms: p.rtt().map(|d| d.as_millis() as u32),
                })
                .collect();
            let hello = ToServer::Hello { version: crate::VERSION.to_string(), candidates: self.locals.clone(), paths };
            self.net.send_control(&hello);
            self.net.server.hello_sent(now);
        }
        for message in self.net.server.due_requests(now) {
            self.net.send_control(&message);
        }

        let connected = self.net.server.connected(now);
        if connected != self.net.server.was_connected {
            self.net.server.was_connected = connected;
            if connected {
                crate::log!("connected to the server");
                on_event(Event::Connected { first: !std::mem::replace(&mut self.announced, true) });
            } else {
                crate::log!("the server stopped answering; direct connections keep working");
                on_event(Event::Unreachable);
            }
        }
    }

    /// How many ports to probe around a strict router's public port. Probing
    /// is cheap when our own router keeps our port (it opens nothing new for
    /// each destination); when it changes ports too, probe fewer.
    /// Whether this device's router gives it another port outside than inside.
    fn translated(&self) -> bool {
        let public = self.net.server.me.as_ref().and_then(|me| me.endpoint);
        public.is_some_and(|endpoint| endpoint.port() != self.port)
    }

    /// Opens the extra ports toward `target` and starts pinging from them.
    fn open_scatter(&mut self, key: [u8; 32], target: SocketAddr, name: &str, now: Instant) {
        let mut sockets = Vec::with_capacity(SCATTER_SOCKETS);
        for _ in 0..SCATTER_SOCKETS {
            match UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) {
                Ok(socket) if socket.set_nonblocking(true).is_ok() => sockets.push(socket),
                _ => break, // out of descriptors: use what we have
            }
        }
        crate::log!("opening {} ports toward {name} at {target} through a strict router", sockets.len());
        self.scatter = Some(Scatter { sockets, key, target, until: now + SCATTER_LIFETIME, next_ping: now, main_heard: false });
    }

    fn scatter_tick(&mut self, now: Instant) {
        let Some(scatter) = self.scatter.as_mut() else { return };
        // Done when time is up, or once a path works anyway (from the ordinary probes).
        let found = self.peers.iter().any(|p| p.key == scatter.key && p.direct.is_some());
        if now >= scatter.until || found {
            self.scatter = None;
            return;
        }
        if now < scatter.next_ping {
            return;
        }
        scatter.next_ping = now + SCATTER_PING;
        let Some(peer) = self.peers.iter().find(|p| p.key == scatter.key) else { return };
        for socket in &scatter.sockets {
            let ping = disco::seal(&peer.disco, &self.me, keys::random_bytes(), &Message::Ping);
            let _ = socket.send_to(&ping, scatter.target);
        }
    }

    /// The other device reached one of the extra ports: that port gets through
    /// the strict router, so it becomes this device's socket.
    fn receive_scatter(&mut self, wire: &mut [u8]) {
        let Some(scatter) = self.scatter.as_ref() else { return };
        let mut adopted = None;
        'sockets: for (index, socket) in scatter.sockets.iter().enumerate() {
            while let Ok((n, from)) = socket.recv_from(wire) {
                if disco::sender(&wire[..n]) == Some(scatter.key) {
                    adopted = Some((index, from, n));
                    break 'sockets;
                }
            }
        }
        let Some((index, from, n)) = adopted else { return };
        let mut scatter = self.scatter.take().expect("checked above");
        // A path that works, or is being found, on the main socket stays: switching
        // ports would break it.
        if scatter.main_heard || self.peers.iter().any(|p| p.key == scatter.key && p.direct.is_some()) {
            return;
        }
        let socket = scatter.sockets.swap_remove(index);
        self.net.socket = socket;
        crate::net::set_buffers(&self.net.socket, SOCKET_BUFFER);
        sys::quiet_udp(&self.net.socket);
        if let Ok(address) = self.net.socket.local_addr() {
            self.port = address.port();
        }
        crate::log!("a strict router let {from} through on UDP {}; using that port now", self.port);
        self.net.server.hello_soon();
        let data = wire[..n].to_vec();
        self.on_datagram(from, &data);
    }

    fn probe_window(&self) -> u16 {
        let public = self.net.server.me.as_ref().and_then(|me| me.endpoint);
        match public {
            Some(endpoint) if endpoint.port() == self.port => 1024,
            Some(_) => 256,
            None => 0,
        }
    }

    fn peers_tick(&mut self, now: Instant, on_event: &dyn Fn(Event)) {
        let relay = self.net.server.relay_available(now);
        let mine: Vec<_> = self.my_candidates().into_iter().filter_map(disco::v4).collect();
        let window = self.probe_window();
        let translated = self.translated();
        let mut scatter_to = None;
        for peer in &mut self.peers {
            if let Some(target) = peer.due_scatter(now, translated) {
                scatter_to.get_or_insert((peer.key, target, peer.info.name.clone()));
            }
            if let TunnResult::WriteToNetwork(data) = peer.tunn.update_timers(&mut self.out) {
                self.net.send(peer.route(relay), &peer.key, data);
            }
            for (id, addr) in peer.due_pings(now) {
                let ping = disco::seal(&peer.disco, &self.me, id, &Message::Ping);
                self.net.send_to(addr, &ping);
            }
            let (probes, started) = peer.due_probes(now, window);
            if started {
                if let Some((_, first)) = probes.first() {
                    crate::log!("probing {window} ports near {}:{} for {}", first.ip(), first.port(), peer.info.name);
                }
            }
            for (id, addr) in probes {
                let ping = disco::seal(&peer.disco, &self.me, id, &Message::Ping);
                self.net.send_to(addr, &ping);
            }
            if self.net.server.connected(now) && peer.take_call() {
                let call = disco::seal(&peer.disco, &self.me, keys::random_bytes(), &Message::CallMeMaybe { candidates: mine.clone() });
                self.net.send_frame(&Frame::Relay { peer: peer.key, payload: &call });
            }
            match peer.choose_path(now) {
                Some(PathChange::Direct(addr, rtt)) => {
                    crate::log!("direct path to {} via {addr} ({} ms)", peer.info.name, rtt.as_millis());
                    self.net.server.hello_soon(); // report the new path to the server
                    on_event(Event::Direct { name: peer.info.name.clone() });
                    // Start the session now if earlier handshakes had no path.
                    if peer.tunn.time_since_last_handshake().is_none() {
                        if let TunnResult::WriteToNetwork(data) = peer.tunn.format_handshake_initiation(&mut self.out, true) {
                            self.net.send_to(addr, data);
                        }
                    }
                }
                Some(PathChange::Lost) => {
                    self.net.server.hello_soon();
                    crate::log!("lost the direct path to {}; {}", peer.info.name, if relay { "using the relay" } else { "searching again" });
                }
                None => {}
            }
            peer.prune();
        }
        if let Some((key, target, name)) = scatter_to {
            if self.scatter.is_none() {
                self.open_scatter(key, target, &name, now);
            }
        }
        self.scatter_tick(now);
        if self.net.server.connected(now) {
            let round = self.round;
            let before = self.peers.len();
            self.peers.retain(|p| {
                let keep = round.saturating_sub(p.round) < MISSED_ROUNDS;
                if !keep {
                    crate::log!("{} left the network", p.info.name);
                }
                keep
            });
            if self.peers.len() != before {
                self.cache_due.get_or_insert(now);
            }
        }
    }

    fn receive(&mut self, wire: &mut [u8]) {
        for _ in 0..1024 {
            let (n, from) = match self.net.socket.recv_from(wire) {
                Ok(received) => received,
                Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                Err(_) => return,
            };
            self.net.rx_bytes += n as u64;
            if let Some(scatter) = self.scatter.as_mut() {
                if disco::sender(&wire[..n]) == Some(scatter.key) {
                    scatter.main_heard = true;
                }
            }
            self.on_datagram(from, &wire[..n]);
        }
    }

    fn on_datagram(&mut self, from: SocketAddr, data: &[u8]) {
        if disco::is_disco(data) {
            self.on_disco(Some(from), data);
            return;
        }
        let Ok(packet) = Tunn::parse_incoming_packet(data) else { return };
        let target = match packet {
            Packet::HandshakeInit(init) => {
                if Instant::now() - self.last_limiter_reset >= Duration::from_secs(1) {
                    self.limiter.reset_count();
                    self.last_limiter_reset = Instant::now();
                }
                match self.limiter.verify_packet(Some(from.ip()), data, &mut self.out) {
                    Ok(_) => {}
                    Err(TunnResult::WriteToNetwork(cookie)) => {
                        let _ = self.net.socket.send_to(cookie, from);
                        return;
                    }
                    Err(_) => return,
                }
                let Ok(half) = parse_handshake_anon(&self.secret, &self.public, &init) else { return };
                if half.peer_static_public == self.net.server.key {
                    Some(Target::Server)
                } else {
                    self.peer_index(&half.peer_static_public).map(Target::Peer)
                }
            }
            Packet::HandshakeResponse(p) => self.target_for(p.receiver_idx),
            Packet::PacketCookieReply(p) => self.target_for(p.receiver_idx),
            Packet::PacketData(p) => self.target_for(p.receiver_idx),
        };
        match target {
            Some(Target::Server) => self.on_server_datagram(from, data),
            Some(Target::Peer(index)) => self.on_peer_datagram(index, Some(from), data),
            None => {}
        }
    }

    fn target_for(&self, receiver_index: u32) -> Option<Target> {
        let slot = receiver_index >> 8;
        if slot == SERVER_SLOT {
            return Some(Target::Server);
        }
        self.peers.iter().position(|p| p.slot == slot).map(Target::Peer)
    }

    fn peer_index(&self, key: &[u8; 32]) -> Option<usize> {
        self.peers.iter().position(|p| &p.key == key)
    }

    fn on_server_datagram(&mut self, from: SocketAddr, data: &[u8]) {
        let mut frames = Vec::new();
        let mut input = data;
        loop {
            match self.net.server.tunn.decapsulate(Some(from.ip()), input, &mut self.out) {
                TunnResult::WriteToNetwork(reply) => {
                    let _ = self.net.socket.send_to(reply, from);
                    input = &[];
                }
                TunnResult::WriteToTunnelV4(packet, _) => {
                    frames.push(packet.to_vec());
                    break;
                }
                _ => break,
            }
        }
        for frame in frames {
            self.on_frame(&frame);
        }
    }

    fn on_frame(&mut self, packet: &[u8]) {
        match decode_frame(packet) {
            Some((_, Frame::Control(json))) => match serde_json::from_slice::<ToDevice>(json) {
                Ok(message) => self.on_control(message),
                Err(e) => crate::log!("unreadable message from the server: {e}"),
            },
            Some((_, Frame::Relay { peer, payload })) => {
                if disco::is_disco(payload) {
                    self.on_disco(None, payload);
                } else if let Some(index) = self.peer_index(&peer) {
                    self.on_peer_datagram(index, None, payload);
                }
            }
            None => {}
        }
    }

    fn on_control(&mut self, message: ToDevice) {
        let now = Instant::now();
        match message {
            ToDevice::Me { name, address, network, endpoint, relay } => {
                if address != self.stack.address {
                    crate::log!("the server lists this device as {address}, not {}", self.stack.address);
                }
                if self.net.server.me.as_ref().is_none_or(|me| me.endpoint != endpoint) {
                    crate::log!("public address: {}", endpoint.map_or("unknown".into(), |e| e.to_string()));
                }
                self.net.server.me = Some(Me { name, network, endpoint });
                self.net.server.relay = relay;
                self.net.server.answered(now);
                self.round += 1;
            }
            ToDevice::Peer(info) => {
                if self.upsert_peer(info, now) {
                    self.cache_due.get_or_insert(now);
                }
            }
            ToDevice::Gone { key } => {
                if let Some(key) = link_proto::key_from_text(&key) {
                    if let Some(index) = self.peer_index(&key) {
                        let peer = self.peers.remove(index);
                        crate::log!("{} was removed from the network", peer.info.name);
                        self.cache_due.get_or_insert(now);
                    }
                }
            }
            ToDevice::Reply { id, code, expires_in, error } => self.net.server.complete(id, code, expires_in, error),
            ToDevice::Removed => {
                crate::log!("this device was removed from its network");
                self.net.server.removed = true; // reported when the loop ends
            }
        }
    }

    /// Adds or updates a device from the server's list. Returns true when
    /// something worth caching changed.
    fn upsert_peer(&mut self, info: PeerInfo, now: Instant) -> bool {
        let Some(key) = link_proto::key_from_text(&info.key) else { return false };
        if key == self.me {
            return false;
        }
        if let Some(index) = self.peer_index(&key) {
            let peer = &mut self.peers[index];
            let changed = peer.info.name != info.name || peer.info.candidates != info.candidates;
            if peer.update(info, self.round, now) {
                peer.start_search(now);
            }
            return changed;
        }
        let slot = self.next_slot;
        self.next_slot = (self.next_slot + 1) & 0x00ff_ffff;
        if self.next_slot <= SERVER_SLOT {
            self.next_slot = SERVER_SLOT + 1;
        }
        crate::log!("device {} ({}) is {}", info.name, keys::short(&info.key), info.address);
        self.peers.push(Peer::new(&self.secret, key, info, slot, self.round, now));
        true
    }

    fn on_peer_datagram(&mut self, index: usize, from: Option<SocketAddr>, data: &[u8]) {
        let now = Instant::now();
        let relay = self.net.server.relay_available(now);
        let peer = &mut self.peers[index];
        let route = match from {
            Some(addr) => Route::Direct(addr),
            None => peer.route(relay),
        };
        let mut input = data;
        let mut valid = false;
        loop {
            match peer.tunn.decapsulate(from.map(|a| a.ip()), input, &mut self.out) {
                TunnResult::WriteToNetwork(reply) => {
                    valid = true;
                    self.net.send(route, &peer.key, reply);
                    input = &[];
                }
                TunnResult::WriteToTunnelV4(packet, source) => {
                    valid = true;
                    if source == peer.info.address {
                        peer.rx_bytes += packet.len() as u64;
                        self.stack.ingest(packet);
                    }
                    break;
                }
                TunnResult::Done => {
                    valid |= !input.is_empty();
                    break;
                }
                _ => break,
            }
        }
        if valid {
            peer.last_rx = Some(now);
            if let Some(addr) = from {
                // An authenticated handshake from a new address: the device moved.
                let handshake = data.first() == Some(&1);
                if handshake && peer.roam(addr, now) {
                    crate::log!("{} moved to {addr}", peer.info.name);
                    self.net.server.hello_soon();
                } else {
                    peer.heard_from(addr, now);
                }
            }
        }
    }

    fn on_disco(&mut self, from: Option<SocketAddr>, data: &[u8]) {
        let Some(sender) = disco::sender(data) else { return };
        let Some(index) = self.peer_index(&sender) else { return };
        let now = Instant::now();
        let peer = &mut self.peers[index];
        let Some((id, message)) = disco::open(&peer.disco, data) else { return };
        match (message, from) {
            (Message::Ping, Some(addr)) => {
                let Some(observed) = disco::v4(addr) else { return };
                let pong = disco::seal(&peer.disco, &self.me, id, &Message::Pong { observed });
                self.net.send_to(addr, &pong);
                peer.heard_from(addr, now);
            }
            (Message::Pong { .. }, Some(addr)) => {
                peer.on_pong(id, addr, now);
            }
            (Message::CallMeMaybe { candidates }, _) => {
                let candidates: Vec<SocketAddr> = candidates.into_iter().map(SocketAddr::V4).collect();
                peer.add_candidates(&candidates, now);
                peer.called(now);
                if !peer.searching(now) && peer.direct.is_none() {
                    peer.start_search(now);
                }
            }
            _ => {}
        }
    }

    /// Encrypts packets from the stack and sends each to its device.
    fn transmit(&mut self) {
        let now = Instant::now();
        let relay = self.net.server.relay_available(now);
        while let Some(packet) = self.stack.device.tx.pop_front() {
            if packet.len() < 20 {
                continue;
            }
            let destination = Ipv4Addr::new(packet[16], packet[17], packet[18], packet[19]);
            let Some(peer) = self.peers.iter_mut().find(|p| p.info.address == destination) else { continue };
            let route = peer.route(relay);
            if route == Route::None {
                peer.want_path(now);
            }
            if let TunnResult::WriteToNetwork(data) = peer.tunn.encapsulate(&packet, &mut self.out) {
                peer.tx_bytes += packet.len() as u64;
                self.net.send(route, &peer.key, data);
            }
        }
    }

    fn save_cache(&mut self, now: Instant) {
        if self.cache_due.is_none_or(|since| now - since < CACHE_DELAY) {
            return;
        }
        self.cache_due = None;
        let infos: Vec<PeerInfo> = self.peers.iter().map(|p| p.info.clone()).collect();
        if let Err(e) = state::save(&self.cache_path, &infos) {
            crate::log!("{e}");
        }
    }

    fn publish(&self, shared: &Shared) {
        let now = Instant::now();
        let relay = self.net.server.relay_available(now);
        let server = &self.net.server;
        let (tcp_connections, udp_sessions) = self.stack.counts();
        let status = Status {
            server: if server.removed {
                ServerState::Removed
            } else if server.connected(now) {
                ServerState::Connected
            } else if server.ever_connected {
                ServerState::Unreachable
            } else {
                ServerState::Connecting
            },
            server_address: server.addr,
            public_endpoint: server.me.as_ref().and_then(|me| me.endpoint),
            relay: server.relay,
            local_port: self.port,
            candidates: self.my_candidates(),
            port_mapping: self
                .portmap
                .as_ref()
                .and_then(|p| p.current())
                .map(|m| format!("{} ({})", m.external, m.method)),
            peers: self
                .peers
                .iter()
                .map(|p| {
                    let route = p.route(relay);
                    PeerStatus {
                        key: p.info.key.clone(),
                        name: p.info.name.clone(),
                        kind: p.info.kind.clone(),
                        address: p.info.address,
                        online: p.info.online || p.last_rx.is_some_and(|t| now - t < Duration::from_secs(30)),
                        path: route.name(),
                        endpoint: match route {
                            Route::Direct(addr) | Route::Both(addr) => Some(addr),
                            _ => None,
                        },
                        rtt_ms: p.rtt().map(|d| d.as_millis() as u32),
                        rx_bytes: p.rx_bytes,
                        tx_bytes: p.tx_bytes,
                        last_handshake: p.tunn.time_since_last_handshake(),
                    }
                })
                .collect(),
            forwards: self.stack.forward_status(),
            tcp_connections,
            udp_sessions,
            rx_bytes: self.net.rx_bytes,
            tx_bytes: self.net.tx_bytes,
            last_error: server.error.clone(),
        };
        *shared.status.lock().unwrap_or_else(|e| e.into_inner()) = status;
    }
}
