//! Small network helpers.

use std::net::{Ipv4Addr, SocketAddr, UdpSocket};

/// The address other devices on the local network use for this machine.
pub fn lan_address() -> Option<Ipv4Addr> {
    // Connecting a UDP socket picks the outgoing interface without sending anything.
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect((Ipv4Addr::new(192, 0, 2, 1), 9)).ok()?;
    match socket.local_addr().ok()? {
        SocketAddr::V4(addr) if !addr.ip().is_unspecified() => Some(*addr.ip()),
        _ => None,
    }
}

/// Asks for larger socket buffers, which absorb video bursts. A smaller
/// system limit is fine.
pub fn set_buffers(socket: &impl crate::sys::AsRaw, bytes: i32) {
    crate::sys::set_buffers(socket, bytes);
}

/// Private, shared (carrier NAT) or link-local IPv4 addresses.
pub fn is_private(ip: Ipv4Addr) -> bool {
    let [a, b, ..] = ip.octets();
    ip.is_private() || ip.is_link_local() || ip.is_loopback() || (a == 100 && (64..128).contains(&b))
}

/// Whether an address is on a private, shared or local network.
pub fn is_private_addr(addr: SocketAddr) -> bool {
    match addr {
        SocketAddr::V4(v4) => is_private(*v4.ip()),
        SocketAddr::V6(_) => true,
    }
}
