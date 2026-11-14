//! One other device: its end-to-end encrypted session and the paths to it.
//!
//! Every address the device might be reachable at is a candidate: the address
//! the server sees, the addresses the device reports (its local network) and
//! addresses packets actually arrived from. Candidates are pinged; the fastest
//! one that answers becomes the direct path.

use std::collections::VecDeque;
use std::net::{Ipv4Addr, SocketAddr};
use std::time::{Duration, Instant};

use boringtun::noise::Tunn;
use boringtun::x25519::{PublicKey, StaticSecret};
use link_proto::PeerInfo;

use crate::disco::{DiscoKey, TxId};
use crate::keys::random_bytes;

/// A direct path stays in use while pongs keep arriving within this time.
const PATH_TIMEOUT: Duration = Duration::from_secs(16);
/// A direct path without a pong for this long also sends through the relay,
/// so a path that just broke costs a few seconds, not a TCP timeout. The
/// receiver drops whichever copy arrives second.
const TRUST_DIRECT: Duration = Duration::from_millis(6500);
/// Pings on a working direct path. They also keep home routers' mappings open.
const PING_DIRECT: Duration = Duration::from_secs(5);
/// Pings while searching for a path, right after something changed.
const PING_SEARCH: Duration = Duration::from_secs(2);
const SEARCH_TIME: Duration = Duration::from_secs(20);
/// Pings between searches while no direct path works.
const PING_IDLE: Duration = Duration::from_secs(20);
/// A new search starts this often while there is no direct path.
const SEARCH_AGAIN: Duration = Duration::from_secs(60);
const MAX_CANDIDATES: usize = 12;
const CANDIDATE_IDLE: Duration = Duration::from_secs(120);
const PENDING_LIFETIME: Duration = Duration::from_secs(10);
const MAX_PENDING: usize = 64;
/// Strict routers (mobile carriers, many hotels) give a device a different port
/// for each destination, so the port the server sees is not the one the device
/// uses toward us, and our pings to it go nowhere. Such routers usually hand out
/// ports near each other: when ordinary pings have not found a path after this
/// long, also ping the ports around the one the server saw.
const PROBE_AFTER: Duration = Duration::from_secs(3);
/// Probes per tick (250 ms).
const PROBE_BATCH: usize = 48;
/// Answers to probes are accepted this long after the round starts.
const PROBE_LIFETIME: Duration = Duration::from_secs(20);
/// Many carriers pick each new port at random instead. Then the strict side
/// opens many ports at once (see `due_scatter`) and the other side also probes
/// this many random ports: with a few hundred open, one is very likely hit.
const RANDOM_PROBES: usize = 1024;

/// One round of probing the ports around a device's public address. All
/// probes of a round share one transaction id.
struct Probe {
    id: TxId,
    ip: Ipv4Addr,
    ports: Vec<u16>,
    next: usize,
    started: Instant,
    /// The search this round belongs to; one round per search.
    search: Instant,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Route {
    Direct(SocketAddr),
    /// Direct, but not confirmed lately: send both ways.
    Both(SocketAddr),
    Relay,
    None,
}

impl Route {
    pub fn name(self) -> &'static str {
        match self {
            Route::Direct(_) | Route::Both(_) => "direct",
            Route::Relay => "relay",
            Route::None => "none",
        }
    }
}

pub struct Candidate {
    pub addr: SocketAddr,
    /// In the server's latest list for this device.
    pub listed: bool,
    pub last_pong: Option<Instant>,
    pub rtt: Option<Duration>,
    pub last_seen: Instant,
}

pub struct Peer {
    pub key: [u8; 32],
    pub info: PeerInfo,
    pub tunn: Tunn,
    pub slot: u32,
    pub disco: DiscoKey,
    pub candidates: Vec<Candidate>,
    pending: VecDeque<(TxId, SocketAddr, Instant)>,
    pub direct: Option<(SocketAddr, Duration)>,
    next_ping: Instant,
    search_until: Option<Instant>,
    last_search: Option<Instant>,
    call_due: bool,
    probe: Option<Probe>,
    /// The search for which this device's side last opened extra ports.
    scatter_search: Option<Instant>,
    /// When the device last asked through the server to be pinged: it is
    /// online then, even if the server's list has not said so yet.
    last_call: Option<Instant>,
    /// The server round in which this device was last listed.
    pub round: u64,
    pub rx_bytes: u64,
    pub tx_bytes: u64,
    pub last_rx: Option<Instant>,
}

pub enum PathChange {
    Direct(SocketAddr, Duration),
    Lost,
}

impl Peer {
    pub fn new(secret: &StaticSecret, key: [u8; 32], info: PeerInfo, slot: u32, round: u64, now: Instant) -> Peer {
        let public = PublicKey::from(key);
        let mut peer = Peer {
            key,
            tunn: Tunn::new(secret.clone(), public, None, None, slot, None),
            disco: DiscoKey::new(secret, &public),
            info: info.clone(),
            slot,
            candidates: Vec::new(),
            pending: VecDeque::new(),
            direct: None,
            next_ping: now,
            search_until: None,
            last_search: None,
            call_due: false,
            probe: None,
            scatter_search: None,
            last_call: None,
            round,
            rx_bytes: 0,
            tx_bytes: 0,
            last_rx: None,
        };
        peer.update(info, round, now);
        peer.start_search(now);
        peer
    }

    pub fn route(&self, relay: bool) -> Route {
        match self.direct {
            Some((addr, _)) => {
                let confirmed = self.candidates.iter().find(|c| c.addr == addr).and_then(|c| c.last_pong);
                if relay && confirmed.is_none_or(|t| t.elapsed() > TRUST_DIRECT) {
                    Route::Both(addr)
                } else {
                    Route::Direct(addr)
                }
            }
            None if relay => Route::Relay,
            None => Route::None,
        }
    }

    /// Applies the server's latest information. Returns true when it lists an
    /// address not known before, or the device just came online without a path.
    pub fn update(&mut self, info: PeerInfo, round: u64, now: Instant) -> bool {
        self.round = round;
        let mut new = !self.info.online && info.online && self.direct.is_none();
        for candidate in &mut self.candidates {
            candidate.listed = false;
        }
        for addr in &info.candidates {
            if !addr.is_ipv4() {
                continue;
            }
            match self.candidates.iter_mut().find(|c| c.addr == *addr) {
                Some(candidate) => {
                    candidate.listed = true;
                    candidate.last_seen = now;
                }
                None => {
                    new = true;
                    self.candidates.push(Candidate { addr: *addr, listed: true, last_pong: None, rtt: None, last_seen: now });
                }
            }
        }
        self.info = info;
        self.trim();
        new
    }

    /// Adds an address the device was heard from. Returns true if it is new.
    pub fn heard_from(&mut self, addr: SocketAddr, now: Instant) -> bool {
        if let Some(candidate) = self.candidates.iter_mut().find(|c| c.addr == addr) {
            candidate.last_seen = now;
            return false;
        }
        self.candidates.push(Candidate { addr, listed: false, last_pong: None, rtt: None, last_seen: now });
        self.trim();
        self.next_ping = now;
        true
    }

    pub fn add_candidates(&mut self, addrs: &[SocketAddr], now: Instant) {
        for addr in addrs {
            self.heard_from(*addr, now);
        }
    }

    /// Starts a burst of pings, and asks the device (through the server) to
    /// ping back at the same time.
    pub fn start_search(&mut self, now: Instant) {
        self.search_until = Some(now + SEARCH_TIME);
        self.last_search = Some(now);
        self.next_ping = now;
        self.call_due = true;
    }

    pub fn searching(&self, now: Instant) -> bool {
        self.search_until.is_some_and(|until| now < until)
    }

    /// The device asked to be pinged (`call_me_maybe`), so it is online.
    pub fn called(&mut self, now: Instant) {
        self.last_call = Some(now);
    }

    /// Whether the device is waiting at a port to be found: the server lists it
    /// online, or it asked to be pinged within the server's online window.
    fn online(&self, now: Instant) -> bool {
        self.info.online || self.last_call.is_some_and(|t| now - t < Duration::from_secs(60))
    }

    /// Something wants to reach this device and there is no direct path.
    pub fn want_path(&mut self, now: Instant) {
        if self.direct.is_none() && !self.searching(now) && self.last_search.is_none_or(|t| now - t >= Duration::from_secs(10)) {
            self.start_search(now);
        }
    }

    /// Whether to send `call_me_maybe` now; at most once per search.
    pub fn take_call(&mut self) -> bool {
        std::mem::take(&mut self.call_due)
    }

    /// Candidates to ping now, each with a fresh transaction id.
    pub fn due_pings(&mut self, now: Instant) -> Vec<(TxId, SocketAddr)> {
        if self.direct.is_none() && !self.searching(now) && self.last_search.is_none_or(|t| now - t >= SEARCH_AGAIN) {
            self.start_search(now);
        }
        if now < self.next_ping {
            return Vec::new();
        }
        let interval = if self.direct.is_some() {
            PING_DIRECT
        } else if self.searching(now) {
            PING_SEARCH
        } else {
            PING_IDLE
        };
        self.next_ping = now + interval;
        while self.pending.front().is_some_and(|(_, _, at)| now - *at > PENDING_LIFETIME) || self.pending.len() > MAX_PENDING {
            self.pending.pop_front();
        }
        let mut pings = Vec::new();
        for candidate in &self.candidates {
            let id = random_bytes::<12>();
            self.pending.push_back((id, candidate.addr, now));
            pings.push((id, candidate.addr));
        }
        pings
    }

    /// The device's public address as the server sees it, when its router seems
    /// to give it another port there than on its own network: a strict router.
    fn translated_public(&self) -> Option<(Ipv4Addr, u16)> {
        let public = self.candidates.iter().find(|c| c.listed && !crate::net::is_private_addr(c.addr))?;
        let SocketAddr::V4(public) = public.addr else { return None };
        let same_port = self
            .candidates
            .iter()
            .any(|c| c.listed && crate::net::is_private_addr(c.addr) && c.addr.port() == public.port());
        (!same_port).then_some((*public.ip(), public.port()))
    }

    /// Probes to send now: after a few seconds of searching without a path, the
    /// ports around a strict router's public port, nearest first, `window` in all.
    /// Returns the probes and whether this call started a new round.
    pub fn due_probes(&mut self, now: Instant, window: u16) -> (Vec<(TxId, SocketAddr)>, bool) {
        let mut started = false;
        if let (None, Some(search)) = (self.direct, self.last_search) {
            let new_round = self.probe.as_ref().is_none_or(|p| p.search != search);
            // Only an online device is waiting at a port to be found.
            if new_round && window > 0 && self.online(now) && self.searching(now) && now - search >= PROBE_AFTER {
                if let Some((ip, port)) = self.translated_public() {
                    let mut ports = Vec::with_capacity(window as usize + RANDOM_PROBES);
                    for distance in 1..=u32::from(window) {
                        for candidate in [u32::from(port) + distance, u32::from(port).wrapping_sub(distance)] {
                            if (1024..=65535).contains(&candidate) && ports.len() < window as usize {
                                ports.push(candidate as u16);
                            }
                        }
                        if ports.len() >= window as usize {
                            break;
                        }
                    }
                    let near = ports.len();
                    let mut tries = 0;
                    while ports.len() < near + RANDOM_PROBES && tries < RANDOM_PROBES * 4 {
                        tries += 1;
                        let candidate = 1024 + u16::from_le_bytes(random_bytes::<2>()) % (65535 - 1024 + 1);
                        if candidate != port && !ports[near..].contains(&candidate) && !ports[..near].contains(&candidate) {
                            ports.push(candidate);
                        }
                    }
                    self.probe = Some(Probe { id: random_bytes::<12>(), ip, ports, next: 0, started: now, search });
                    started = true;
                }
            }
        }
        let Some(probe) = self.probe.as_mut() else { return (Vec::new(), false) };
        if self.direct.is_some() || now - probe.started > PROBE_LIFETIME {
            return (Vec::new(), false);
        }
        let end = (probe.next + PROBE_BATCH).min(probe.ports.len());
        let batch = probe.ports[probe.next..end].iter().map(|&port| (probe.id, SocketAddr::from((probe.ip, port)))).collect();
        probe.next = end;
        (batch, started)
    }

    /// When this device's own router changes ports (a strict router), it should
    /// open many ports toward the other device for the random probes to find:
    /// returns where to send from them, once per search.
    pub fn due_scatter(&mut self, now: Instant, me_translated: bool) -> Option<SocketAddr> {
        let search = self.last_search?;
        if !me_translated || self.direct.is_some() || !self.online(now) || !self.searching(now) || now - search < PROBE_AFTER {
            return None;
        }
        if self.scatter_search == Some(search) {
            return None;
        }
        let target = self.candidates.iter().find(|c| c.listed && !crate::net::is_private_addr(c.addr))?.addr;
        self.scatter_search = Some(search);
        Some(target)
    }

    /// The device started a new session from another address (it restarted or
    /// moved to another network): use that address at once instead of waiting
    /// for the old path to time out. Returns true if the path changed.
    pub fn roam(&mut self, addr: SocketAddr, now: Instant) -> bool {
        let Some((current, rtt)) = self.direct else { return false };
        if current == addr {
            return false;
        }
        self.heard_from(addr, now);
        if let Some(candidate) = self.candidates.iter_mut().find(|c| c.addr == addr) {
            candidate.last_pong = Some(now);
            candidate.rtt = Some(rtt);
        }
        if let Some(old) = self.candidates.iter_mut().find(|c| c.addr == current) {
            old.last_pong = None; // the old address is gone; don't switch back to it
        }
        self.direct = Some((addr, rtt));
        true
    }

    /// Handles a pong. Returns false if it answers no ping we sent from there.
    pub fn on_pong(&mut self, id: TxId, from: SocketAddr, now: Instant) -> bool {
        if let (Some(probe), SocketAddr::V4(from4)) = (&self.probe, from) {
            if probe.id == id
                && *from4.ip() == probe.ip
                && probe.ports[..probe.next].contains(&from4.port())
                && now - probe.started <= PROBE_LIFETIME
            {
                // A probe found the port this device uses toward us.
                let rtt = now - probe.started;
                match self.candidates.iter_mut().find(|c| c.addr == from) {
                    Some(candidate) => {
                        candidate.last_pong = Some(now);
                        candidate.rtt.get_or_insert(rtt);
                        candidate.last_seen = now;
                    }
                    None => {
                        self.candidates.push(Candidate { addr: from, listed: false, last_pong: Some(now), rtt: Some(rtt), last_seen: now });
                        self.trim();
                    }
                }
                self.next_ping = now; // measure it properly with an ordinary ping
                return true;
            }
        }
        let Some(position) = self.pending.iter().position(|(pending, addr, _)| *pending == id && *addr == from) else {
            return false;
        };
        let (_, addr, sent) = self.pending.remove(position).unwrap();
        if let Some(candidate) = self.candidates.iter_mut().find(|c| c.addr == addr) {
            candidate.last_pong = Some(now);
            candidate.rtt = Some(now - sent);
            candidate.last_seen = now;
        }
        true
    }

    /// Picks the fastest candidate that answers; keeps the current one unless
    /// another is clearly faster.
    pub fn choose_path(&mut self, now: Instant) -> Option<PathChange> {
        let alive = |c: &&Candidate| c.last_pong.is_some_and(|t| now - t < PATH_TIMEOUT);
        let best = self.candidates.iter().filter(alive).min_by_key(|c| c.rtt.unwrap_or(Duration::MAX));
        let current = self.direct.and_then(|(addr, _)| self.candidates.iter().filter(alive).find(|c| c.addr == addr));
        let chosen = match (current, best) {
            (Some(current), Some(best)) => {
                let (current_rtt, best_rtt) = (current.rtt.unwrap_or_default(), best.rtt.unwrap_or_default());
                if best_rtt * 3 / 2 + Duration::from_millis(1) < current_rtt {
                    best
                } else {
                    current
                }
            }
            (None, Some(best)) => best,
            (_, None) => {
                return self.direct.take().map(|_| {
                    self.start_search(now);
                    PathChange::Lost
                });
            }
        };
        let (addr, rtt) = (chosen.addr, chosen.rtt.unwrap_or_default());
        let changed = self.direct.is_none_or(|(current, _)| current != addr);
        self.direct = Some((addr, rtt));
        if changed {
            self.search_until = None;
            return Some(PathChange::Direct(addr, rtt));
        }
        None
    }

    /// Forgets addresses that are neither listed nor heard from recently.
    fn trim(&mut self) {
        let direct = self.direct.map(|(addr, _)| addr);
        let now = Instant::now();
        self.candidates
            .retain(|c| c.listed || Some(c.addr) == direct || now.saturating_duration_since(c.last_seen) < CANDIDATE_IDLE);
        while self.candidates.len() > MAX_CANDIDATES {
            let Some(oldest) = self
                .candidates
                .iter()
                .enumerate()
                .filter(|(_, c)| !c.listed && Some(c.addr) != direct)
                .min_by_key(|(_, c)| c.last_seen)
                .map(|(i, _)| i)
            else {
                break;
            };
            self.candidates.remove(oldest);
        }
    }

    pub fn prune(&mut self) {
        self.trim();
    }

    pub fn rtt(&self) -> Option<Duration> {
        self.direct.map(|(_, rtt)| rtt)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::Ipv4Addr;

    fn peer(candidates: &[&str]) -> Peer {
        let secret = StaticSecret::from(random_bytes::<32>());
        let other = PublicKey::from(&StaticSecret::from(random_bytes::<32>()));
        let info = PeerInfo {
            key: link_proto::key_to_text(other.as_bytes()),
            name: "PS5".into(),
            kind: "ps5".into(),
            address: Ipv4Addr::new(10, 77, 0, 2),
            candidates: candidates.iter().map(|c| c.parse().unwrap()).collect(),
            online: true,
            version: None,
        };
        Peer::new(&secret, *other.as_bytes(), info, 2, 1, Instant::now())
    }

    #[test]
    fn strict_routers_are_probed_near_their_public_port() {
        // The server saw port 37646; on its own network the device uses 41701.
        let mut peer = peer(&["198.51.100.9:37646", "172.20.10.3:41701"]);
        let start = peer.last_search.unwrap();
        assert!(peer.due_probes(start + Duration::from_secs(1), 8).0.is_empty(), "ordinary pings go first");
        let (probes, started) = peer.due_probes(start + PROBE_AFTER, 8);
        assert!(started);
        let ports: Vec<u16> = probes.iter().map(|(_, addr)| addr.port()).collect();
        assert_eq!(ports[..8], [37647, 37645, 37648, 37644, 37649, 37643, 37650, 37642], "nearest ports first");
        assert!(probes.iter().all(|(id, addr)| *id == probes[0].0 && addr.ip().to_string() == "198.51.100.9"));
        let (more, started) = peer.due_probes(start + PROBE_AFTER, 8);
        assert!(!started && more.iter().all(|(id, _)| *id == probes[0].0), "the same round continues");

        let id = probes[0].0;
        let now = start + PROBE_AFTER + Duration::from_millis(300);
        assert!(!peer.on_pong(id, "198.51.100.9:40000".parse().unwrap(), now), "only probed ports count");
        assert!(!peer.on_pong(id, "198.51.100.10:37648".parse().unwrap(), now), "only the probed address counts");
        assert!(peer.on_pong(id, "198.51.100.9:37648".parse().unwrap(), now));
        assert!(matches!(peer.choose_path(now), Some(PathChange::Direct(addr, _)) if addr.port() == 37648));
        assert!(peer.due_probes(now, 8).0.is_empty(), "no probes once a path works");
    }

    #[test]
    fn devices_that_keep_their_port_are_not_probed() {
        let mut peer = peer(&["203.0.113.7:41701", "192.168.1.20:41701"]);
        let start = peer.last_search.unwrap();
        assert!(peer.due_probes(start + PROBE_AFTER, 1024).0.is_empty());
        let mut strict = super::tests::peer(&["198.51.100.9:37646", "172.20.10.3:41701"]);
        let start = strict.last_search.unwrap();
        assert!(strict.due_probes(start + PROBE_AFTER, 0).0.is_empty(), "no window, no probes");
    }

    #[test]
    fn random_carriers_get_random_probes_and_strict_sides_open_ports() {
        let mut peer = peer(&["198.51.100.9:37646", "172.20.10.3:41701"]);
        let start = peer.last_search.unwrap();
        let mut all = Vec::new();
        let (first, _) = peer.due_probes(start + PROBE_AFTER, 8);
        all.extend(first);
        loop {
            let (more, _) = peer.due_probes(start + PROBE_AFTER, 8);
            if more.is_empty() { break; }
            all.extend(more);
        }
        assert_eq!(all.len(), 8 + RANDOM_PROBES, "the nearby window, then random ports");
        let mut ports: Vec<u16> = all.iter().map(|(_, addr)| addr.port()).collect();
        ports.sort_unstable();
        ports.dedup();
        assert_eq!(ports.len(), 8 + RANDOM_PROBES, "no port twice");
        assert!(!ports.contains(&37646) && ports.iter().all(|&p| p >= 1024));
        let far = all.last().unwrap();
        assert!(peer.on_pong(far.0, far.1, start + PROBE_AFTER + Duration::from_secs(5)), "a random port can answer too");

        let mut strict = super::tests::peer(&["203.0.113.7:41701", "192.168.1.20:41701"]);
        let start = strict.last_search.unwrap();
        assert_eq!(strict.due_scatter(start + PROBE_AFTER, false), None, "a router that keeps ports opens nothing");
        assert_eq!(strict.due_scatter(start + Duration::from_secs(1), true), None, "ordinary pings first");
        assert_eq!(strict.due_scatter(start + PROBE_AFTER, true), Some("203.0.113.7:41701".parse().unwrap()));
        assert_eq!(strict.due_scatter(start + PROBE_AFTER, true), None, "once per search");
    }

    #[test]
    fn a_new_session_from_another_address_moves_the_path() {
        let mut peer = peer(&["203.0.113.7:41701"]);
        let start = Instant::now();
        let pings = peer.due_pings(start);
        assert!(peer.on_pong(pings[0].0, pings[0].1, start));
        peer.choose_path(start);
        let moved: SocketAddr = "198.51.100.20:30939".parse().unwrap();
        assert!(peer.roam(moved, start));
        assert_eq!(peer.route(false), Route::Direct(moved));
        assert!(peer.choose_path(start).is_none(), "the new address stays chosen");
        assert!(!peer.roam(moved, start), "no change for the same address");
    }

    #[test]
    fn offline_devices_are_not_probed() {
        let mut peer = peer(&["198.51.100.9:37646", "172.20.10.3:41701"]);
        peer.info.online = false;
        let start = peer.last_search.unwrap();
        assert!(peer.due_probes(start + PROBE_AFTER, 1024).0.is_empty());

        // Asking to be pinged proves the device is online before the list says so.
        peer.called(start + PROBE_AFTER);
        assert!(!peer.due_probes(start + PROBE_AFTER, 1024).0.is_empty());

        // Coming online in the list starts a new search, so probing need not
        // wait for the next one.
        let mut late = super::tests::peer(&["198.51.100.9:37646", "172.20.10.3:41701"]);
        late.info.online = false;
        let info = PeerInfo { online: true, ..late.info.clone() };
        assert!(late.update(info.clone(), 2, Instant::now()), "coming online is news");
        assert!(!late.update(info, 3, Instant::now()), "staying online is not");
    }

    #[test]
    fn probes_stay_in_the_valid_port_range() {
        let mut peer = peer(&["198.51.100.9:1030", "172.20.10.3:41701"]);
        let start = peer.last_search.unwrap();
        let (probes, _) = peer.due_probes(start + PROBE_AFTER, 20);
        let ports: Vec<u16> = probes.iter().map(|(_, addr)| addr.port()).collect();
        assert_eq!(ports.len(), PROBE_BATCH);
        assert!(ports.iter().all(|&port| port >= 1024));
        assert!(ports[..20].iter().all(|&port| port.abs_diff(1030) <= 30), "the window comes first");
    }

    #[test]
    fn pongs_choose_the_fastest_path() {
        let mut peer = peer(&["203.0.113.7:41701", "192.168.1.20:41701"]);
        let start = Instant::now();
        let pings = peer.due_pings(start);
        assert_eq!(pings.len(), 2);
        assert!(peer.due_pings(start).is_empty(), "pings are paced");
        assert_eq!(peer.route(true), Route::Relay);
        assert_eq!(peer.route(false), Route::None);

        let (wan, lan) = (pings[0], pings[1]);
        assert!(!peer.on_pong(wan.0, lan.1, start), "a pong must come from where the ping went");
        assert!(peer.on_pong(wan.0, wan.1, start + Duration::from_millis(40)));
        assert!(matches!(peer.choose_path(start), Some(PathChange::Direct(addr, _)) if addr == wan.1));
        assert!(peer.on_pong(lan.0, lan.1, start + Duration::from_millis(2)));
        assert!(matches!(peer.choose_path(start), Some(PathChange::Direct(addr, _)) if addr == lan.1), "faster LAN path wins");
        assert_eq!(peer.route(false), Route::Direct(lan.1));

        peer.candidates.iter_mut().for_each(|c| c.last_pong = c.last_pong.map(|_| Instant::now() - TRUST_DIRECT * 2));
        assert_eq!(peer.route(true), Route::Both(lan.1), "an unconfirmed path also uses the relay");
        assert_eq!(peer.route(false), Route::Direct(lan.1));

        let later = start + PATH_TIMEOUT + Duration::from_secs(1);
        assert!(matches!(peer.choose_path(later), Some(PathChange::Lost)));
        assert_eq!(peer.route(true), Route::Relay);
        assert!(peer.searching(later), "losing the path starts a new search");
    }

    #[test]
    fn learns_addresses_it_hears_from() {
        let mut peer = peer(&["203.0.113.7:41701"]);
        let now = Instant::now();
        assert!(peer.heard_from("198.51.100.9:6000".parse().unwrap(), now));
        assert!(!peer.heard_from("198.51.100.9:6000".parse().unwrap(), now));
        assert_eq!(peer.candidates.len(), 2);
        let info = PeerInfo { candidates: vec!["203.0.113.7:41702".parse().unwrap()], ..peer.info.clone() };
        assert!(peer.update(info, 2, now), "a new listed address");
        assert_eq!(peer.candidates.iter().filter(|c| c.listed).count(), 1);
    }
}
