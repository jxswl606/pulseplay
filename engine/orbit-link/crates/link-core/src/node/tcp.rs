//! TCP forwarding between the network stack and local sockets.
//!
//! Incoming: when a SYN arrives for an allowed port, the local service is
//! connected first. Only if it accepts is a listening socket created for the
//! SYN, so a closed port is refused with a reset just as it would be on the
//! local network.
//!
//! Outgoing: a connection accepted on a port forward is connected to the other
//! device's port, and data is copied once it is established.

use std::io::{ErrorKind, Read, Write};
use std::net::{Ipv4Addr, Shutdown, SocketAddr, TcpStream};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use smoltcp::iface::{Interface, SocketHandle, SocketSet};
use smoltcp::socket::tcp;
use smoltcp::wire::{IpAddress, IpEndpoint};

use super::ftp::{PassiveRewriter, FTP_PORTS};

const BUFFER_SIZE: usize = 256 * 1024;
const CONNECT_TIMEOUT: Duration = Duration::from_millis(1500);
/// A listener whose SYN was taken by another socket is removed after this.
const UNUSED_LISTENER: Duration = Duration::from_secs(10);
const MAX_CONNECTIONS: usize = 512;
/// An outgoing connection that is not established by then is given up.
const OUTGOING_TIMEOUT: Duration = Duration::from_secs(20);

/// Local ports of the connections made to this machine's services for other
/// devices. They come from this machine's own address, so a service that
/// trusts local programs can ask [`is_tunnel_port`] to tell them apart.
static TUNNEL_PORTS: Mutex<Vec<u16>> = Mutex::new(Vec::new());

/// Whether a connection from `port` on this machine was made for another device.
pub fn is_tunnel_port(port: u16) -> bool {
    port != 0 && TUNNEL_PORTS.lock().unwrap().contains(&port)
}

fn release_tunnel_port(port: u16) {
    if port != 0 {
        let mut ports = TUNNEL_PORTS.lock().unwrap();
        if let Some(index) = ports.iter().position(|&p| p == port) {
            ports.swap_remove(index);
        }
    }
}

#[derive(PartialEq, Eq)]
enum Stage {
    /// Waiting for the SYN this listener was created for.
    Listening,
    /// Outgoing connection waiting to be established.
    Connecting,
    Open,
    /// Finished; removed on the next pass so a pending reset is still sent.
    Dead,
    Reap,
}

struct Conn {
    handle: SocketHandle,
    port: u16,
    remote: IpEndpoint,
    stage: Stage,
    host: Option<TcpStream>,
    /// Local port of `host` when it is a connection to a service here.
    tunnel_port: u16,
    created: Instant,
    host_eof: bool,
    host_write_blocked: bool,
    sent_host_fin: bool,
    to_tunnel: Vec<u8>,
    ftp: Option<PassiveRewriter>,
}

pub struct TcpForwarder {
    conns: Vec<Conn>,
    target: Ipv4Addr,
    address: Ipv4Addr,
}

impl TcpForwarder {
    pub fn new(target: Ipv4Addr, address: Ipv4Addr) -> Self {
        TcpForwarder { conns: Vec::new(), target, address }
    }

    pub fn len(&self) -> usize {
        self.conns.len()
    }

    /// Called for each incoming SYN before the packet reaches smoltcp.
    pub fn on_syn(&mut self, sockets: &mut SocketSet<'static>, remote: IpEndpoint, port: u16) {
        if self.conns.iter().any(|c| c.port == port && c.remote == remote && matches!(c.stage, Stage::Listening | Stage::Open)) {
            return; // retransmitted SYN: the socket for it already exists
        }
        if self.conns.len() >= MAX_CONNECTIONS {
            return; // no listener: smoltcp answers with a reset
        }
        let Ok(host) = connect_local(self.target, port) else {
            return; // closed port: smoltcp answers with a reset
        };
        // Registered before any data reaches the service.
        let tunnel_port = host.local_addr().map_or(0, |a| a.port());
        if tunnel_port != 0 {
            TUNNEL_PORTS.lock().unwrap().push(tunnel_port);
        }
        let mut socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; BUFFER_SIZE]),
            tcp::SocketBuffer::new(vec![0; BUFFER_SIZE]),
        );
        socket.pause_synack(true);
        socket.set_nagle_enabled(false);
        socket.set_keep_alive(Some(smoltcp::time::Duration::from_secs(30)));
        socket.set_timeout(Some(smoltcp::time::Duration::from_secs(120)));
        if socket.listen(port).is_err() {
            release_tunnel_port(tunnel_port);
            return;
        }
        let handle = sockets.add(socket);
        self.conns.push(Conn {
            handle,
            port,
            remote,
            stage: Stage::Listening,
            host: Some(host),
            tunnel_port,
            created: Instant::now(),
            host_eof: false,
            host_write_blocked: false,
            sent_host_fin: false,
            to_tunnel: Vec::new(),
            ftp: FTP_PORTS.contains(&port).then(|| PassiveRewriter::new(self.address)),
        });
    }

    /// Connects a local client (accepted on a port forward) to another device.
    pub fn connect_out(
        &mut self,
        iface: &mut Interface,
        sockets: &mut SocketSet<'static>,
        host: TcpStream,
        remote: IpEndpoint,
        local_port: u16,
    ) -> bool {
        if self.conns.len() >= MAX_CONNECTIONS || host.set_nonblocking(true).is_err() {
            return false;
        }
        let _ = host.set_nodelay(true);
        let mut socket = tcp::Socket::new(
            tcp::SocketBuffer::new(vec![0; BUFFER_SIZE]),
            tcp::SocketBuffer::new(vec![0; BUFFER_SIZE]),
        );
        socket.set_nagle_enabled(false);
        socket.set_keep_alive(Some(smoltcp::time::Duration::from_secs(30)));
        socket.set_timeout(Some(smoltcp::time::Duration::from_secs(120)));
        if socket.connect(iface.context(), remote, local_port).is_err() {
            return false;
        }
        let handle = sockets.add(socket);
        self.conns.push(Conn {
            handle,
            port: local_port,
            remote,
            stage: Stage::Connecting,
            host: Some(host),
            tunnel_port: 0,
            created: Instant::now(),
            host_eof: false,
            host_write_blocked: false,
            sent_host_fin: false,
            to_tunnel: Vec::new(),
            ftp: None,
        });
        true
    }

    pub fn poll_fds(&self, sockets: &SocketSet<'static>, fds: &mut Vec<crate::sys::PollFd>) {
        for conn in &self.conns {
            let Some(host) = conn.host.as_ref() else { continue };
            let socket = sockets.get::<tcp::Socket>(conn.handle);
            let mut events = 0;
            if !conn.host_eof && socket.can_send() && conn.to_tunnel.is_empty() {
                events |= crate::sys::READABLE;
            }
            if conn.host_write_blocked {
                events |= crate::sys::WRITABLE;
            }
            if events != 0 {
                fds.push(crate::sys::entry(host, events));
            }
        }
    }

    /// Moves data in both directions and handles connection setup and teardown.
    pub fn service(&mut self, sockets: &mut SocketSet<'static>) {
        let now = Instant::now();
        for conn in &mut self.conns {
            if matches!(conn.stage, Stage::Dead | Stage::Reap) {
                continue;
            }
            let socket = sockets.get_mut::<tcp::Socket>(conn.handle);
            if conn.stage == Stage::Connecting {
                match socket.state() {
                    tcp::State::Established => conn.stage = Stage::Open,
                    tcp::State::SynSent | tcp::State::SynReceived if now.duration_since(conn.created) < OUTGOING_TIMEOUT => continue,
                    _ => {
                        // Refused, unreachable or timed out: closing the local
                        // client tells it the connection failed.
                        socket.abort();
                        conn.stage = Stage::Dead;
                        continue;
                    }
                }
            }
            if conn.stage == Stage::Listening {
                match socket.state() {
                    tcp::State::Listen => {
                        if now.duration_since(conn.created) > UNUSED_LISTENER {
                            conn.stage = Stage::Dead;
                        }
                        continue;
                    }
                    tcp::State::SynReceived => {
                        if let Some(remote) = socket.remote_endpoint() {
                            conn.remote = remote;
                        }
                        conn.stage = Stage::Open;
                        socket.pause_synack(false);
                    }
                    _ => {
                        socket.abort();
                        conn.stage = Stage::Dead;
                    }
                }
            }
            if conn.stage == Stage::Open {
                pump(conn, socket);
            }
        }
        let mut index = 0;
        while index < self.conns.len() {
            match self.conns[index].stage {
                Stage::Reap => {
                    let conn = self.conns.swap_remove(index);
                    sockets.remove(conn.handle);
                }
                Stage::Dead => {
                    let conn = &mut self.conns[index];
                    conn.stage = Stage::Reap;
                    conn.host = None;
                    release_tunnel_port(std::mem::take(&mut conn.tunnel_port));
                    index += 1;
                }
                _ => index += 1,
            }
        }
    }
}

impl Drop for TcpForwarder {
    fn drop(&mut self) {
        for conn in &mut self.conns {
            release_tunnel_port(std::mem::take(&mut conn.tunnel_port));
        }
    }
}

fn connect_local(target: Ipv4Addr, port: u16) -> std::io::Result<TcpStream> {
    let stream = TcpStream::connect_timeout(&SocketAddr::from((target, port)), CONNECT_TIMEOUT)?;
    stream.set_nonblocking(true)?;
    let _ = stream.set_nodelay(true);
    Ok(stream)
}

fn pump(conn: &mut Conn, socket: &mut tcp::Socket<'static>) {
    let Conn { host, stage, host_eof, host_write_blocked, sent_host_fin, to_tunnel, ftp, .. } = conn;
    let Some(host) = host.as_mut() else {
        *stage = Stage::Dead;
        return;
    };

    // Tunnel -> local service.
    *host_write_blocked = false;
    while socket.can_recv() {
        let result = socket.recv(|data| match host.write(data) {
            Ok(n) => (n, Ok(n)),
            Err(e) => (0, Err(e)),
        });
        match result {
            Ok(Ok(0)) => break,
            Ok(Ok(_)) => {}
            Ok(Err(e)) if e.kind() == ErrorKind::WouldBlock => {
                *host_write_blocked = true;
                break;
            }
            Ok(Err(e)) if e.kind() == ErrorKind::Interrupted => {}
            _ => {
                socket.abort();
                *stage = Stage::Dead;
                return;
            }
        }
    }
    let peer_closed = matches!(
        socket.state(),
        tcp::State::CloseWait | tcp::State::LastAck | tcp::State::Closing | tcp::State::TimeWait | tcp::State::Closed
    );
    if peer_closed && socket.recv_queue() == 0 && !*sent_host_fin {
        let _ = host.shutdown(Shutdown::Write);
        *sent_host_fin = true;
    }

    // Local service -> tunnel.
    flush_to_tunnel(to_tunnel, socket);
    let mut scratch = [0u8; 16 * 1024];
    while !*host_eof && to_tunnel.is_empty() && socket.can_send() {
        let read = match ftp.as_mut() {
            Some(rewriter) => match host.read(&mut scratch) {
                Ok(0) => Ok(0),
                Ok(n) => {
                    rewriter.feed(&scratch[..n], to_tunnel);
                    flush_to_tunnel(to_tunnel, socket);
                    Ok(n)
                }
                Err(e) => Err(e),
            },
            None => match socket.send(|buffer| match host.read(buffer) {
                Ok(n) => (n, Ok(n)),
                Err(e) => (0, Err(e)),
            }) {
                Ok(result) => result,
                Err(_) => break,
            },
        };
        match read {
            Ok(0) => {
                *host_eof = true;
                if let Some(rewriter) = ftp.as_mut() {
                    rewriter.finish(to_tunnel);
                    flush_to_tunnel(to_tunnel, socket);
                }
            }
            Ok(_) => {}
            Err(e) if e.kind() == ErrorKind::WouldBlock => break,
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            Err(_) => {
                socket.abort();
                *stage = Stage::Dead;
                return;
            }
        }
    }
    if *host_eof && to_tunnel.is_empty() && socket.may_send() {
        socket.close();
    }

    if matches!(socket.state(), tcp::State::Closed | tcp::State::TimeWait) {
        *stage = Stage::Dead;
    }
}

fn flush_to_tunnel(to_tunnel: &mut Vec<u8>, socket: &mut tcp::Socket<'static>) {
    if to_tunnel.is_empty() || !socket.may_send() {
        return;
    }
    if let Ok(sent) = socket.send_slice(to_tunnel) {
        to_tunnel.drain(..sent);
    }
}

/// Reads the TCP ports and flags of an IPv4 packet; used to spot new SYNs.
pub fn syn_of(packet: &[u8]) -> Option<(IpEndpoint, u16)> {
    let ihl = ((*packet.first()? & 0x0f) as usize) * 4;
    if packet.len() < ihl + 14 || packet[9] != 6 || u16::from_be_bytes([packet[6], packet[7]]) & 0x1fff != 0 {
        return None;
    }
    let flags = packet[ihl + 13];
    if flags & 0x02 == 0 || flags & 0x10 != 0 {
        return None; // not a connection request
    }
    let src = Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]);
    let src_port = u16::from_be_bytes([packet[ihl], packet[ihl + 1]]);
    let dst_port = u16::from_be_bytes([packet[ihl + 2], packet[ihl + 3]]);
    Some((IpEndpoint::new(IpAddress::Ipv4(src), src_port), dst_port))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::TcpListener;

    #[test]
    fn connections_to_local_services_are_known_until_closed() {
        let service = TcpListener::bind("127.0.0.1:0").unwrap();
        let port = service.local_addr().unwrap().port();
        let mut sockets = SocketSet::new(Vec::new());
        let mut forwarder = TcpForwarder::new(Ipv4Addr::LOCALHOST, Ipv4Addr::new(100, 64, 0, 1));
        let remote = IpEndpoint::new(IpAddress::v4(100, 64, 0, 2), 50000);
        forwarder.on_syn(&mut sockets, remote, port);
        let (_, client) = service.accept().unwrap();
        assert!(is_tunnel_port(client.port()));
        // A program on this machine is not mistaken for another device.
        let own = TcpStream::connect(("127.0.0.1", port)).unwrap();
        assert!(!is_tunnel_port(own.local_addr().unwrap().port()));
        assert!(!is_tunnel_port(0));
        drop(forwarder);
        assert!(!is_tunnel_port(client.port()));
    }
}
