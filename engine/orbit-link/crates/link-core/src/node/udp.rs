//! UDP forwarding.
//!
//! Each remote endpoint talking to a port gets its own local socket connected
//! to the service, so replies find their way back to the right sender. This is
//! what Remote Play's stream (UDP 9296/9297) and discovery (UDP 9302) need.

use std::collections::HashMap;
use std::io::ErrorKind;
use std::net::{Ipv4Addr, SocketAddr, UdpSocket};
use std::time::{Duration, Instant};

use smoltcp::iface::{SocketHandle, SocketSet};
use smoltcp::socket::udp;
use smoltcp::wire::IpEndpoint;

const PACKET_SLOTS: usize = 512;
const BUFFER_BYTES: usize = 512 * 1024;
const SESSION_IDLE: Duration = Duration::from_secs(180);
const PORT_IDLE: Duration = Duration::from_secs(600);
const MAX_SESSIONS: usize = 1024;
const MAX_DATAGRAM: usize = 16 * 1024;

struct Port {
    handle: SocketHandle,
    last: Instant,
}

struct Session {
    remote: IpEndpoint,
    port: u16,
    host: UdpSocket,
    last: Instant,
}

pub struct UdpForwarder {
    ports: HashMap<u16, Port>,
    sessions: HashMap<(IpEndpoint, u16), Session>,
    target: Ipv4Addr,
    scratch: Vec<u8>,
}

impl UdpForwarder {
    pub fn new(target: Ipv4Addr) -> Self {
        UdpForwarder { ports: HashMap::new(), sessions: HashMap::new(), target, scratch: vec![0; 64 * 1024] }
    }

    pub fn len(&self) -> usize {
        self.sessions.len()
    }

    /// Called for each incoming UDP packet before it reaches smoltcp.
    pub fn on_datagram(&mut self, sockets: &mut SocketSet<'static>, port: u16) {
        if let Some(existing) = self.ports.get_mut(&port) {
            existing.last = Instant::now();
            return;
        }
        let meta = || vec![udp::PacketMetadata::EMPTY; PACKET_SLOTS];
        let mut socket = udp::Socket::new(
            udp::PacketBuffer::new(meta(), vec![0; BUFFER_BYTES]),
            udp::PacketBuffer::new(meta(), vec![0; BUFFER_BYTES]),
        );
        if socket.bind(port).is_ok() {
            let handle = sockets.add(socket);
            self.ports.insert(port, Port { handle, last: Instant::now() });
        }
    }

    pub fn poll_fds(&self, fds: &mut Vec<crate::sys::PollFd>) {
        for session in self.sessions.values() {
            fds.push(crate::sys::entry(&session.host, crate::sys::READABLE));
        }
    }

    /// Delivers datagrams that arrived through the tunnel to local services.
    pub fn tunnel_to_host(&mut self, sockets: &mut SocketSet<'static>) {
        let now = Instant::now();
        for (&port, entry) in self.ports.iter_mut() {
            let socket = sockets.get_mut::<udp::Socket>(entry.handle);
            while let Ok((data, meta)) = socket.recv() {
                entry.last = now;
                let key = (meta.endpoint, port);
                if !self.sessions.contains_key(&key) {
                    if self.sessions.len() >= MAX_SESSIONS {
                        continue;
                    }
                    match open_session(self.target, port) {
                        Ok(host) => {
                            self.sessions.insert(key, Session { remote: meta.endpoint, port, host, last: now });
                        }
                        Err(_) => continue,
                    }
                }
                let session = self.sessions.get_mut(&key).unwrap();
                session.last = now;
                // Errors (such as a refused port) only drop this datagram, as UDP would.
                let _ = session.host.send(data);
            }
        }
    }

    /// Sends replies from local services back through the tunnel.
    pub fn host_to_tunnel(&mut self, sockets: &mut SocketSet<'static>) {
        let now = Instant::now();
        for session in self.sessions.values_mut() {
            let Some(port) = self.ports.get(&session.port) else { continue };
            let socket = sockets.get_mut::<udp::Socket>(port.handle);
            loop {
                match session.host.recv(&mut self.scratch) {
                    Ok(n) if n <= MAX_DATAGRAM => {
                        session.last = now;
                        if socket.can_send() {
                            let _ = socket.send_slice(&self.scratch[..n], session.remote);
                        }
                    }
                    Ok(_) => {} // too large to fragment through the tunnel
                    Err(e) if e.kind() == ErrorKind::WouldBlock => break,
                    Err(e) if e.kind() == ErrorKind::Interrupted => {}
                    Err(_) => break, // e.g. ICMP port unreachable reported on a connected socket
                }
            }
        }
    }

    pub fn expire(&mut self, sockets: &mut SocketSet<'static>) {
        let now = Instant::now();
        self.sessions.retain(|_, s| now.duration_since(s.last) < SESSION_IDLE);
        let active: Vec<u16> = self.sessions.values().map(|s| s.port).collect();
        self.ports.retain(|port, entry| {
            let keep = active.contains(port) || now.duration_since(entry.last) < PORT_IDLE;
            if !keep {
                sockets.remove(entry.handle);
            }
            keep
        });
    }
}

fn open_session(target: Ipv4Addr, port: u16) -> std::io::Result<UdpSocket> {
    let bind = if target.is_loopback() { Ipv4Addr::LOCALHOST } else { Ipv4Addr::UNSPECIFIED };
    let socket = UdpSocket::bind((bind, 0))?;
    socket.connect(SocketAddr::from((target, port)))?;
    socket.set_nonblocking(true)?;
    crate::sys::quiet_udp(&socket);
    Ok(socket)
}

/// Destination port of an IPv4 UDP packet (first fragment only).
pub fn udp_port_of(packet: &[u8]) -> Option<u16> {
    let ihl = ((*packet.first()? & 0x0f) as usize) * 4;
    if packet.len() < ihl + 8 || packet[9] != 17 || u16::from_be_bytes([packet[6], packet[7]]) & 0x1fff != 0 {
        return None;
    }
    Some(u16::from_be_bytes([packet[ihl + 2], packet[ihl + 3]]))
}
