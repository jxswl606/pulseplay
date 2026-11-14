//! In-memory packet device connecting the WireGuard tunnel to smoltcp.

use std::collections::VecDeque;

use smoltcp::phy::{self, Checksum, ChecksumCapabilities, DeviceCapabilities, Medium};
use smoltcp::time::Instant;

/// Packets waiting to leave through the tunnel are dropped beyond this depth.
const MAX_QUEUED: usize = 4096;

pub struct QueueDevice {
    pub rx: VecDeque<Vec<u8>>,
    pub tx: VecDeque<Vec<u8>>,
    mtu: usize,
}

impl QueueDevice {
    pub fn new(mtu: usize) -> Self {
        QueueDevice { rx: VecDeque::new(), tx: VecDeque::new(), mtu }
    }
}

pub struct RxToken(Vec<u8>);
pub struct TxToken<'a>(&'a mut VecDeque<Vec<u8>>);

impl phy::RxToken for RxToken {
    fn consume<R, F: FnOnce(&[u8]) -> R>(self, f: F) -> R {
        f(&self.0)
    }
}

impl phy::TxToken for TxToken<'_> {
    fn consume<R, F: FnOnce(&mut [u8]) -> R>(self, len: usize, f: F) -> R {
        let mut packet = vec![0u8; len];
        let result = f(&mut packet);
        if self.0.len() < MAX_QUEUED {
            self.0.push_back(packet);
        }
        result
    }
}

impl phy::Device for QueueDevice {
    type RxToken<'a> = RxToken;
    type TxToken<'a> = TxToken<'a>;

    fn receive(&mut self, _timestamp: Instant) -> Option<(RxToken, TxToken<'_>)> {
        let packet = self.rx.pop_front()?;
        Some((RxToken(packet), TxToken(&mut self.tx)))
    }

    fn transmit(&mut self, _timestamp: Instant) -> Option<TxToken<'_>> {
        Some(TxToken(&mut self.tx))
    }

    fn capabilities(&self) -> DeviceCapabilities {
        let mut caps = DeviceCapabilities::default();
        caps.medium = Medium::Ip;
        caps.max_transmission_unit = self.mtu;
        // WireGuard authenticates every packet, so incoming checksums need no
        // second check; outgoing ones are still computed for the receiver.
        let mut checksum = ChecksumCapabilities::default();
        checksum.ipv4 = Checksum::Tx;
        checksum.udp = Checksum::Tx;
        checksum.tcp = Checksum::Tx;
        checksum.icmpv4 = Checksum::Tx;
        caps.checksum = checksum;
        caps
    }
}
