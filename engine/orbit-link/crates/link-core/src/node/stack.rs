//! The device's in-process network stack, bound to its network address.
//!
//! It serves this machine's ports to other devices (on a PS5) and carries
//! connections made through port forwards (on a computer).

use std::net::Ipv4Addr;
use std::time::Instant;

use smoltcp::iface::{Config, Interface, SocketSet};
use smoltcp::wire::{HardwareAddress, IpAddress, IpCidr};

use super::device::QueueDevice;
use super::forward::{ForwardStatus, Forwards, Ports, Resolve};
use super::tcp::{self, TcpForwarder};
use super::udp::{self, UdpForwarder};
use crate::keys;
use crate::state::Forward;

pub struct Stack {
    pub address: Ipv4Addr,
    /// Offer this machine's ports to the other devices.
    pub serve: bool,
    pub allow_loader: bool,
    iface: Interface,
    pub device: QueueDevice,
    sockets: SocketSet<'static>,
    tcp: TcpForwarder,
    udp: UdpForwarder,
    forwards: Forwards,
    ports: Ports,
    start: Instant,
    scratch: Vec<u8>,
}

impl Stack {
    pub fn new(address: Ipv4Addr, serve: bool, allow_loader: bool) -> Stack {
        let mut device = QueueDevice::new(link_proto::MTU);
        let mut config = Config::new(HardwareAddress::Ip);
        config.random_seed = u64::from_le_bytes(keys::random_bytes::<8>());
        let mut iface = Interface::new(config, &mut device, smoltcp::time::Instant::from_micros(0));
        iface.update_ip_addrs(|addrs| {
            let _ = addrs.push(IpCidr::new(IpAddress::Ipv4(address), link_proto::PREFIX));
        });
        Stack {
            address,
            serve,
            allow_loader,
            iface,
            device,
            sockets: SocketSet::new(Vec::new()),
            tcp: TcpForwarder::new(Ipv4Addr::LOCALHOST, address),
            udp: UdpForwarder::new(Ipv4Addr::LOCALHOST),
            forwards: Forwards::default(),
            ports: Ports::default(),
            start: Instant::now(),
            scratch: vec![0; 64 * 1024],
        }
    }

    fn now(&self) -> smoltcp::time::Instant {
        smoltcp::time::Instant::from_micros(self.start.elapsed().as_micros() as i64)
    }

    fn allows(&self, port: u16) -> bool {
        self.allow_loader || port != crate::LOADER_PORT
    }

    /// A decrypted packet from another device.
    pub fn ingest(&mut self, packet: &[u8]) {
        if packet.len() < 20 || packet[16..20] != self.address.octets() {
            return;
        }
        if self.serve {
            if let Some((remote, port)) = tcp::syn_of(packet) {
                if self.allows(port) && !Ports::is_outgoing(port) {
                    self.tcp.on_syn(&mut self.sockets, remote, port);
                }
            } else if let Some(port) = udp::udp_port_of(packet) {
                if self.allows(port) && !Ports::is_outgoing(port) {
                    self.udp.on_datagram(&mut self.sockets, port);
                }
            }
        }
        self.device.rx.push_back(packet.to_vec());
    }

    pub fn set_forwards(&mut self, forwards: &[Forward]) {
        self.forwards.set(forwards, &mut self.sockets, &mut self.ports);
    }

    pub fn forward_status(&self) -> Vec<ForwardStatus> {
        self.forwards.status()
    }

    /// (TCP connections, UDP sessions)
    pub fn counts(&self) -> (usize, usize) {
        (self.tcp.len(), self.udp.len() + self.forwards.sessions())
    }

    pub fn poll_fds(&self, fds: &mut Vec<crate::sys::PollFd>) {
        self.tcp.poll_fds(&self.sockets, fds);
        self.udp.poll_fds(fds);
        self.forwards.poll_fds(fds);
    }

    /// Milliseconds until the stack needs attention, at most `max`.
    pub fn poll_delay(&mut self, max: i64) -> i64 {
        let now = self.now();
        self.iface
            .poll_delay(now, &self.sockets)
            .map_or(max, |d| d.total_millis() as i64)
            .clamp(0, max)
    }

    /// Moves data between local sockets and the stack. Packets to send end up
    /// in `device.tx`.
    pub fn io(&mut self, resolve: Resolve) {
        self.forwards.accept(resolve, &mut self.tcp, &mut self.iface, &mut self.sockets, &mut self.ports);
        self.forwards.host_to_tunnel(resolve, &mut self.sockets, &mut self.ports, &mut self.scratch);
        self.udp.host_to_tunnel(&mut self.sockets);
        self.tcp.service(&mut self.sockets);
        let now = self.now();
        self.iface.poll(now, &mut self.device, &mut self.sockets);
        self.tcp.service(&mut self.sockets);
        self.udp.tunnel_to_host(&mut self.sockets);
        self.forwards.tunnel_to_host(&mut self.sockets);
        self.iface.poll(now, &mut self.device, &mut self.sockets);
    }

    /// Closes idle sessions.
    pub fn expire(&mut self) {
        self.udp.expire(&mut self.sockets);
        self.forwards.expire(&mut self.sockets, &mut self.ports);
    }
}
