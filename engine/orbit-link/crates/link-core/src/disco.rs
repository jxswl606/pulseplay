//! Path discovery messages ("disco") exchanged between two devices.
//!
//! Devices send pings to every address a peer might be reachable at, and the
//! peer answers each with a pong. A pong proves that path works in both
//! directions, so traffic can move to it. Sending pings also opens the sender's
//! router for replies from that address, which is how hole punching works:
//! both devices ping each other at the same time.
//!
//! `call_me_maybe` travels through the server and asks the peer to start
//! pinging now, listing where to reach the sender.
//!
//! Messages are authenticated with a key both devices derive from their
//! WireGuard keys, so nobody else can fake a path. They are not encrypted; they
//! only carry addresses the server already knows.
//!
//! Layout: `magic(4) | sender key(32) | type(1) | id(12) | body | mac(16)`

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

use boringtun::x25519::{PublicKey, StaticSecret};
use ring::{digest, hmac};

pub const MAGIC: [u8; 4] = *b"OLK\x01";
const HEADER: usize = 4 + 32 + 1 + 12;
const MAC: usize = 16;
const MAX_CANDIDATES: usize = 16;

const PING: u8 = 1;
const PONG: u8 = 2;
const CALL_ME_MAYBE: u8 = 3;

pub type TxId = [u8; 12];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Message {
    Ping,
    /// `observed` is the address the ping came from, as the peer saw it.
    Pong { observed: SocketAddrV4 },
    CallMeMaybe { candidates: Vec<SocketAddrV4> },
}

/// The shared key for one pair of devices.
pub struct DiscoKey(hmac::Key);

impl DiscoKey {
    pub fn new(secret: &StaticSecret, peer: &PublicKey) -> DiscoKey {
        let shared = secret.diffie_hellman(peer);
        let mut context = digest::Context::new(&digest::SHA256);
        context.update(b"orbit-link disco v1");
        context.update(shared.as_bytes());
        DiscoKey(hmac::Key::new(hmac::HMAC_SHA256, context.finish().as_ref()))
    }
}

pub fn is_disco(data: &[u8]) -> bool {
    data.len() >= HEADER + MAC && data[..4] == MAGIC
}

/// The public key the message claims to come from; check it with [`open`].
pub fn sender(data: &[u8]) -> Option<[u8; 32]> {
    is_disco(data).then(|| data[4..36].try_into().ok()).flatten()
}

pub fn seal(key: &DiscoKey, me: &[u8; 32], id: TxId, message: &Message) -> Vec<u8> {
    let mut body = Vec::new();
    let kind = match message {
        Message::Ping => PING,
        Message::Pong { observed } => {
            put_address(&mut body, *observed);
            PONG
        }
        Message::CallMeMaybe { candidates } => {
            let candidates = &candidates[..candidates.len().min(MAX_CANDIDATES)];
            body.push(candidates.len() as u8);
            for candidate in candidates {
                put_address(&mut body, *candidate);
            }
            CALL_ME_MAYBE
        }
    };
    let mut out = Vec::with_capacity(HEADER + body.len() + MAC);
    out.extend_from_slice(&MAGIC);
    out.extend_from_slice(me);
    out.push(kind);
    out.extend_from_slice(&id);
    out.extend_from_slice(&body);
    let tag = hmac::sign(&key.0, &out);
    out.extend_from_slice(&tag.as_ref()[..MAC]);
    out
}

/// Checks the message's authenticity and reads it.
pub fn open(key: &DiscoKey, data: &[u8]) -> Option<(TxId, Message)> {
    if !is_disco(data) {
        return None;
    }
    let (body, tag) = data.split_at(data.len() - MAC);
    let expected = hmac::sign(&key.0, body);
    if !constant_time_eq(&expected.as_ref()[..MAC], tag) {
        return None;
    }
    let id: TxId = body[37..49].try_into().ok()?;
    let rest = &body[HEADER..];
    let message = match body[36] {
        PING => Message::Ping,
        PONG => Message::Pong { observed: get_address(rest)? },
        CALL_ME_MAYBE => {
            let count = *rest.first()? as usize;
            if count > MAX_CANDIDATES || rest.len() < 1 + count * 6 {
                return None;
            }
            let candidates = (0..count).filter_map(|i| get_address(&rest[1 + i * 6..])).collect();
            Message::CallMeMaybe { candidates }
        }
        _ => return None,
    };
    Some((id, message))
}

fn put_address(out: &mut Vec<u8>, address: SocketAddrV4) {
    out.extend_from_slice(&address.ip().octets());
    out.extend_from_slice(&address.port().to_be_bytes());
}

fn get_address(data: &[u8]) -> Option<SocketAddrV4> {
    let bytes = data.get(..6)?;
    Some(SocketAddrV4::new(
        Ipv4Addr::new(bytes[0], bytes[1], bytes[2], bytes[3]),
        u16::from_be_bytes([bytes[4], bytes[5]]),
    ))
}

pub fn v4(address: SocketAddr) -> Option<SocketAddrV4> {
    match address {
        SocketAddr::V4(v4) => Some(v4),
        SocketAddr::V6(v6) => v6.ip().to_ipv4_mapped().map(|ip| SocketAddrV4::new(ip, v6.port())),
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    a.len() == b.len() && a.iter().zip(b).fold(0u8, |acc, (x, y)| acc | (x ^ y)) == 0
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::keys::random_bytes;

    fn pair() -> (StaticSecret, PublicKey, StaticSecret, PublicKey) {
        let a = StaticSecret::from(random_bytes::<32>());
        let b = StaticSecret::from(random_bytes::<32>());
        let (pa, pb) = (PublicKey::from(&a), PublicKey::from(&b));
        (a, pa, b, pb)
    }

    #[test]
    fn both_sides_derive_the_same_key() {
        let (a, pa, b, pb) = pair();
        let from_a = DiscoKey::new(&a, &pb);
        let from_b = DiscoKey::new(&b, &pa);
        let id = random_bytes::<12>();
        let observed: SocketAddrV4 = "203.0.113.7:41701".parse().unwrap();
        for message in [
            Message::Ping,
            Message::Pong { observed },
            Message::CallMeMaybe { candidates: vec![observed, "192.168.1.20:41701".parse().unwrap()] },
        ] {
            let sealed = seal(&from_a, pa.as_bytes(), id, &message);
            assert_eq!(sender(&sealed), Some(*pa.as_bytes()));
            assert_eq!(open(&from_b, &sealed), Some((id, message)));
        }
    }

    #[test]
    fn rejects_forgeries() {
        let (a, pa, b, pb) = pair();
        let (c, _, _, _) = pair();
        let id = random_bytes::<12>();
        let mut sealed = seal(&DiscoKey::new(&a, &pb), pa.as_bytes(), id, &Message::Ping);
        assert!(open(&DiscoKey::new(&c, &pb), &sealed).is_none(), "wrong key");
        let last = sealed.len() - 1;
        sealed[last] ^= 1;
        assert!(open(&DiscoKey::new(&b, &pa), &sealed).is_none(), "changed tag");
        assert!(!is_disco(&[1, 0, 0, 0, 5, 6]));
    }
}
