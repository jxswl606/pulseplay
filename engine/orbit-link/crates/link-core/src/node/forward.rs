//! Port forwards: ports of other devices made available on this one, such as
//! a PS5's Remote Play ports on a Mac's `127.0.0.1`. Programs on this machine
//! connect to the local port and reach the other device without any system
//! VPN.

use std::collections::{HashMap, HashSet};
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, TcpListener, UdpSocket};
use std::time::{Duration, Instant};

use smoltcp::iface::{Interface, SocketHandle, SocketSet};
use smoltcp::socket::udp;
use smoltcp::wire::{IpAddress, IpEndpoint};

use super::tcp::TcpForwarder;
use crate::state::{Forward, Protocol};

const PACKET_SLOTS: usize = 256;
const BUFFER_BYTES: usize = 512 * 1024;
const SESSION_IDLE: Duration = Duration::from_secs(180);
const MAX_SESSIONS: usize = 256;
const MAX_DATAGRAM: usize = 16 * 1024;
const FIRST_PORT: u16 = 40000;
const LAST_PORT: u16 = 60999;

/// Finds a device's network address from its public key.
pub type Resolve<'a> = &'a dyn Fn(&str) -> Option<Ipv4Addr>;

/// Local ports of the network stack used by outgoing connections.
#[derive(Default)]
pub struct Ports {
    next: u16,
    udp_in_use: HashSet<u16>,
}

impl Ports {
    pub fn next(&mut self) -> u16 {
        if self.next == 0 {
            // Start somewhere random: after a restart, the other device may still
            // hold connections from our last run on the first ports, and would
            // take a new connection from one of them for an old one.
            let span = u32::from(LAST_PORT - FIRST_PORT) + 1;
            self.next = FIRST_PORT + (u32::from_le_bytes(crate::keys::random_bytes::<4>()) % span) as u16;
        }
        loop {
            if !(FIRST_PORT..=LAST_PORT).contains(&self.next) {
                self.next = FIRST_PORT;
            }
            let port = self.next;
            self.next += 1;
            if !self.udp_in_use.contains(&port) {
                return port;
            }
        }
    }

    /// Ports in this range are replies to outgoing connections, never services.
    pub fn is_outgoing(port: u16) -> bool {
        (FIRST_PORT..=LAST_PORT).contains(&port)
    }
}

#[derive(Clone, Debug)]
pub struct ForwardStatus {
    pub forward: Forward,
    pub error: Option<String>,
}

struct TcpEntry {
    forward: Forward,
    listener: Option<TcpListener>,
    error: Option<String>,
}

struct UdpSession {
    handle: SocketHandle,
    port: u16,
    last: Instant,
}

struct UdpEntry {
    forward: Forward,
    socket: Option<UdpSocket>,
    error: Option<String>,
    sessions: HashMap<SocketAddr, UdpSession>,
}

#[derive(Default)]
pub struct Forwards {
    tcp: Vec<TcpEntry>,
    udp: Vec<UdpEntry>,
}

impl Forwards {
    /// Opens and closes listeners so they match `wanted`.
    pub fn set(&mut self, wanted: &[Forward], sockets: &mut SocketSet<'static>, ports: &mut Ports) {
        self.tcp.retain(|entry| wanted.contains(&entry.forward));
        self.udp.retain_mut(|entry| {
            let keep = wanted.contains(&entry.forward);
            if !keep {
                for session in entry.sessions.values() {
                    sockets.remove(session.handle);
                    ports.udp_in_use.remove(&session.port);
                }
            }
            keep
        });
        for forward in wanted {
            if self.tcp.iter().any(|e| &e.forward == forward) || self.udp.iter().any(|e| &e.forward == forward) {
                continue;
            }
            match forward.protocol {
                Protocol::Tcp => {
                    let (listener, error) = match TcpListener::bind(forward.listen).and_then(|l| {
                        l.set_nonblocking(true)?;
                        Ok(l)
                    }) {
                        Ok(listener) => (Some(listener), None),
                        Err(e) => (None, Some(bind_error(forward, e))),
                    };
                    if let Some(error) = &error {
                        crate::log!("{error}");
                    }
                    self.tcp.push(TcpEntry { forward: forward.clone(), listener, error });
                }
                Protocol::Udp => {
                    let (socket, error) = match UdpSocket::bind(forward.listen).and_then(|s| {
                        s.set_nonblocking(true)?;
                        crate::sys::quiet_udp(&s);
                        Ok(s)
                    }) {
                        Ok(socket) => (Some(socket), None),
                        Err(e) => (None, Some(bind_error(forward, e))),
                    };
                    if let Some(error) = &error {
                        crate::log!("{error}");
                    }
                    self.udp.push(UdpEntry { forward: forward.clone(), socket, error, sessions: HashMap::new() });
                }
            }
        }
    }

    pub fn status(&self) -> Vec<ForwardStatus> {
        let tcp = self.tcp.iter().map(|e| ForwardStatus { forward: e.forward.clone(), error: e.error.clone() });
        let udp = self.udp.iter().map(|e| ForwardStatus { forward: e.forward.clone(), error: e.error.clone() });
        tcp.chain(udp).collect()
    }

    pub fn sessions(&self) -> usize {
        self.udp.iter().map(|e| e.sessions.len()).sum()
    }

    pub fn poll_fds(&self, fds: &mut Vec<crate::sys::PollFd>) {
        for listener in self.tcp.iter().filter_map(|e| e.listener.as_ref()) {
            fds.push(crate::sys::entry(listener, crate::sys::READABLE));
        }
        for socket in self.udp.iter().filter_map(|e| e.socket.as_ref()) {
            fds.push(crate::sys::entry(socket, crate::sys::READABLE));
        }
    }

    /// Accepts local clients and connects each to its device.
    pub fn accept(
        &mut self,
        resolve: Resolve,
        tcp: &mut TcpForwarder,
        iface: &mut Interface,
        sockets: &mut SocketSet<'static>,
        ports: &mut Ports,
    ) {
        for entry in &mut self.tcp {
            let Some(listener) = entry.listener.as_ref() else { continue };
            loop {
                let stream = match listener.accept() {
                    Ok((stream, _)) => stream,
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };
                let Some(address) = resolve(&entry.forward.peer) else {
                    continue; // the device is not in the network (any more): drop the client
                };
                let remote = IpEndpoint::new(IpAddress::Ipv4(address), entry.forward.port);
                tcp.connect_out(iface, sockets, stream, remote, ports.next());
            }
        }
    }

    /// Datagrams from local clients to their devices.
    pub fn host_to_tunnel(&mut self, resolve: Resolve, sockets: &mut SocketSet<'static>, ports: &mut Ports, scratch: &mut [u8]) {
        let now = Instant::now();
        for entry in &mut self.udp {
            let Some(socket) = entry.socket.as_ref() else { continue };
            loop {
                let (n, client) = match socket.recv_from(scratch) {
                    Ok(received) => received,
                    Err(e) if e.kind() == ErrorKind::Interrupted => continue,
                    Err(_) => break,
                };
                if n > MAX_DATAGRAM {
                    continue;
                }
                let Some(address) = resolve(&entry.forward.peer) else { continue };
                if !entry.sessions.contains_key(&client) {
                    if entry.sessions.len() >= MAX_SESSIONS {
                        continue;
                    }
                    let port = ports.next();
                    let meta = || vec![udp::PacketMetadata::EMPTY; PACKET_SLOTS];
                    let mut session = udp::Socket::new(
                        udp::PacketBuffer::new(meta(), vec![0; BUFFER_BYTES]),
                        udp::PacketBuffer::new(meta(), vec![0; BUFFER_BYTES]),
                    );
                    if session.bind(port).is_err() {
                        continue;
                    }
                    ports.udp_in_use.insert(port);
                    let handle = sockets.add(session);
                    entry.sessions.insert(client, UdpSession { handle, port, last: now });
                }
                let session = entry.sessions.get_mut(&client).unwrap();
                session.last = now;
                let remote = IpEndpoint::new(IpAddress::Ipv4(address), entry.forward.port);
                let _ = sockets.get_mut::<udp::Socket>(session.handle).send_slice(&scratch[..n], remote);
            }
        }
    }

    /// Replies from devices to local clients.
    pub fn tunnel_to_host(&mut self, sockets: &mut SocketSet<'static>) {
        let now = Instant::now();
        for entry in &mut self.udp {
            let Some(socket) = entry.socket.as_ref() else { continue };
            for (client, session) in entry.sessions.iter_mut() {
                let receiver = sockets.get_mut::<udp::Socket>(session.handle);
                while let Ok((data, meta)) = receiver.recv() {
                    if meta.endpoint.port != entry.forward.port {
                        continue;
                    }
                    session.last = now;
                    let _ = socket.send_to(data, client);
                }
            }
        }
    }

    pub fn expire(&mut self, sockets: &mut SocketSet<'static>, ports: &mut Ports) {
        let now = Instant::now();
        for entry in &mut self.udp {
            entry.sessions.retain(|_, session| {
                let keep = now.duration_since(session.last) < SESSION_IDLE;
                if !keep {
                    sockets.remove(session.handle);
                    ports.udp_in_use.remove(&session.port);
                }
                keep
            });
        }
    }
}

fn bind_error(forward: &Forward, error: std::io::Error) -> String {
    let reason = if error.kind() == ErrorKind::AddrInUse { "already in use".to_string() } else { error.to_string() };
    format!("cannot listen on {} {}: {reason}", forward.protocol.as_str().to_uppercase(), forward.listen)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn outgoing_ports_start_at_random_and_stay_in_range() {
        let starts: HashSet<u16> = (0..8).map(|_| Ports::default().next()).collect();
        assert!(starts.len() > 1, "a restarted device does not reuse its first ports");
        let mut ports = Ports::default();
        for _ in 0..30_000 {
            assert!(Ports::is_outgoing(ports.next()));
        }
    }
}
