//! Orbit Link wire protocol, shared by devices and servers.
//!
//! Every device keeps one WireGuard session with its server. WireGuard only
//! carries IP packets, so inside that session both sides exchange frames
//! wrapped in a minimal IPv4 header (protocol 253, reserved for experiments by
//! RFC 3692):
//!
//! - **control**: JSON messages ([`ToServer`], [`ToDevice`])
//! - **relay**: a packet for or from another device, named by its public key
//!
//! Traffic between two devices uses its own end-to-end WireGuard session. It
//! goes directly between the devices once hole punching succeeds, and through
//! the server's relay otherwise (when the server offers one). The server only
//! ever sees those packets encrypted.

use std::net::{Ipv4Addr, SocketAddr};

use base64::engine::general_purpose::STANDARD;
use base64::Engine;
use serde::{Deserialize, Serialize};

/// Every network uses the same private range; addresses only exist inside the
/// Orbit Link engines, so they never clash with the local network.
pub const NETWORK: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 0);
pub const PREFIX: u8 = 24;
/// The server's address inside each device's session.
pub const HUB: Ipv4Addr = Ipv4Addr::new(10, 77, 0, 1);
pub const FRAME_PROTOCOL: u8 = 253;

/// Default UDP port of a server.
pub const SERVER_PORT: u16 = 41700;
/// UDP port a device tries first; any free port works.
pub const DEVICE_PORT: u16 = 41701;
/// Packet size inside device-to-device sessions. Small enough that a relayed
/// packet, wrapped twice, still fits a 1500-byte link without fragmenting.
pub const MTU: usize = 1280;
/// Largest relayed message a server forwards when it does not relay traffic:
/// the hole-punching messages devices exchange to find each other.
pub const MAX_SIGNAL: usize = 512;

pub const API_INFO: &str = "/api/v2/info";
pub const API_JOIN: &str = "/api/v2/join";

pub const KIND_PS5: &str = "ps5";
pub const KIND_COMPUTER: &str = "computer";

pub const CODE_LENGTH: usize = 8;
pub const CODE_ALPHABET: &[u8] = b"23456789ABCDEFGHJKMNPQRSTUVWXYZ";
/// Join codes work once and for this long.
pub const CODE_MINUTES: u64 = 30;

const FRAME_RELAY: u8 = 1;
const FRAME_CONTROL: u8 = 2;
const IPV4_HEADER: usize = 20;

#[derive(Debug, PartialEq, Eq)]
pub enum Frame<'a> {
    Relay { peer: [u8; 32], payload: &'a [u8] },
    Control(&'a [u8]),
}

/// Wraps a frame in an IPv4 packet for the device–server session.
pub fn encode_frame(source: Ipv4Addr, destination: Ipv4Addr, frame: &Frame) -> Vec<u8> {
    let body_len = match frame {
        Frame::Relay { payload, .. } => 1 + 32 + payload.len(),
        Frame::Control(json) => 1 + json.len(),
    };
    let total = IPV4_HEADER + body_len;
    let mut packet = Vec::with_capacity(total);
    packet.extend_from_slice(&[0x45, 0]);
    packet.extend_from_slice(&(total as u16).to_be_bytes());
    packet.extend_from_slice(&[0, 0, 0x40, 0, 64, FRAME_PROTOCOL, 0, 0]);
    packet.extend_from_slice(&source.octets());
    packet.extend_from_slice(&destination.octets());
    let checksum = ipv4_checksum(&packet[..IPV4_HEADER]);
    packet[10..12].copy_from_slice(&checksum.to_be_bytes());
    match frame {
        Frame::Relay { peer, payload } => {
            packet.push(FRAME_RELAY);
            packet.extend_from_slice(peer);
            packet.extend_from_slice(payload);
        }
        Frame::Control(json) => {
            packet.push(FRAME_CONTROL);
            packet.extend_from_slice(json);
        }
    }
    packet
}

/// Reads a frame; returns the packet's source address with it.
pub fn decode_frame(packet: &[u8]) -> Option<(Ipv4Addr, Frame<'_>)> {
    if packet.len() <= IPV4_HEADER || packet[0] != 0x45 || packet[9] != FRAME_PROTOCOL {
        return None;
    }
    let total = u16::from_be_bytes([packet[2], packet[3]]) as usize;
    let packet = packet.get(..total)?;
    let source = Ipv4Addr::new(packet[12], packet[13], packet[14], packet[15]);
    let body = &packet[IPV4_HEADER..];
    let frame = match body[0] {
        FRAME_RELAY if body.len() >= 33 => {
            Frame::Relay { peer: body[1..33].try_into().ok()?, payload: &body[33..] }
        }
        FRAME_CONTROL => Frame::Control(&body[1..]),
        _ => return None,
    };
    Some((source, frame))
}

fn ipv4_checksum(header: &[u8]) -> u16 {
    let mut sum: u32 = header.chunks(2).map(|c| u16::from_be_bytes([c[0], c[1]]) as u32).sum();
    while sum >> 16 != 0 {
        sum = (sum & 0xffff) + (sum >> 16);
    }
    !(sum as u16)
}

/// Messages from a device to its server.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ToServer {
    /// Sent every 25 s and whenever the device's local addresses change. The
    /// server answers with `Me` and one `Peer` per other device.
    Hello {
        version: String,
        /// Addresses other devices may reach this one at, besides the address
        /// the server sees.
        candidates: Vec<SocketAddr>,
        #[serde(default)]
        paths: Vec<PathReport>,
    },
    /// Asks for a join code for this device's network.
    Invite { id: u32 },
    /// Removes a device (any member may, including itself).
    Remove { id: u32, key: String },
    Rename { id: u32, key: String, name: String },
}

/// How a device currently reaches one of its peers; shown in the dashboard.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PathReport {
    pub key: String,
    /// `direct`, `relay` or `none`
    pub path: String,
    #[serde(default)]
    pub rtt_ms: Option<u32>,
}

/// Messages from the server to a device.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "t", rename_all = "snake_case")]
pub enum ToDevice {
    Me {
        name: String,
        address: Ipv4Addr,
        network: String,
        /// The device's public address as the server sees it.
        endpoint: Option<SocketAddr>,
        /// Whether this server relays traffic when a direct connection fails.
        relay: bool,
    },
    Peer(PeerInfo),
    Gone { key: String },
    Reply {
        id: u32,
        #[serde(default)]
        code: Option<String>,
        #[serde(default)]
        expires_in: Option<u64>,
        #[serde(default)]
        error: Option<String>,
    },
    /// This device was removed from its network.
    Removed,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PeerInfo {
    pub key: String,
    pub name: String,
    pub kind: String,
    pub address: Ipv4Addr,
    /// Where to try reaching the device: the address the server sees first,
    /// then the addresses the device reported.
    pub candidates: Vec<SocketAddr>,
    pub online: bool,
    #[serde(default)]
    pub version: Option<String>,
}

/// `GET /api/v2/info`
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ServerInfo {
    pub name: String,
    pub version: String,
    /// Anyone may create a network (a public server).
    pub open_networks: bool,
    pub relay: bool,
}

/// `POST /api/v2/join`: joins with a code, or creates a network when the
/// server allows it.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JoinRequest {
    #[serde(default)]
    pub code: Option<String>,
    /// Name of a new network to create instead of using a code.
    #[serde(default)]
    pub create: Option<String>,
    pub public_key: String,
    pub name: String,
    pub kind: String,
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct JoinResponse {
    pub device_id: String,
    pub network_id: String,
    pub network: String,
    pub name: String,
    pub address: Ipv4Addr,
    pub server_key: String,
    /// `host:port` of the server's UDP port.
    pub endpoint: String,
    pub relay: bool,
}

/// Error body of the HTTP API.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ApiError {
    pub error: String,
}

pub fn key_to_text(key: &[u8; 32]) -> String {
    STANDARD.encode(key)
}

pub fn key_from_text(text: &str) -> Option<[u8; 32]> {
    STANDARD.decode(text.trim()).ok()?.try_into().ok()
}

/// Upper-case letters and digits only, so `abcd-efgh` matches `ABCDEFGH`.
pub fn normalize_code(text: &str) -> String {
    text.chars().filter(char::is_ascii_alphanumeric).collect::<String>().to_ascii_uppercase()
}

/// `ABCDEFGH` as `ABCD-EFGH`.
pub fn format_code(code: &str) -> String {
    if code.len() == CODE_LENGTH {
        format!("{}-{}", &code[..4], &code[4..])
    } else {
        code.to_string()
    }
}

/// A display name without control characters, at most 48 characters.
pub fn clean_name(name: &str, fallback: &str) -> String {
    let name: String = name.chars().filter(|c| !c.is_control()).take(48).collect();
    match name.trim() {
        "" => fallback.to_string(),
        name => name.to_string(),
    }
}

pub fn kind_label(kind: &str) -> &'static str {
    match kind {
        KIND_PS5 => "PS5",
        _ => "Computer",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn frames_round_trip() {
        let peer = [7u8; 32];
        let relay = encode_frame(Ipv4Addr::new(10, 77, 0, 2), HUB, &Frame::Relay { peer, payload: b"secret" });
        assert_eq!(ipv4_checksum(&relay[..20]), 0);
        let (source, frame) = decode_frame(&relay).unwrap();
        assert_eq!(source, Ipv4Addr::new(10, 77, 0, 2));
        assert_eq!(frame, Frame::Relay { peer, payload: b"secret" });

        let mut control = encode_frame(HUB, Ipv4Addr::new(10, 77, 0, 3), &Frame::Control(b"{}"));
        control.extend_from_slice(&[0; 9]); // WireGuard padding is ignored
        assert_eq!(decode_frame(&control).unwrap().1, Frame::Control(b"{}"));
        assert!(decode_frame(&control[..21]).is_none());
    }

    #[test]
    fn messages_use_tagged_json() {
        let hello = ToServer::Hello { version: "0.2.0".into(), candidates: vec!["192.168.1.20:41701".parse().unwrap()], paths: vec![] };
        let text = serde_json::to_string(&hello).unwrap();
        assert!(text.starts_with(r#"{"t":"hello""#), "{text}");
        assert_eq!(serde_json::from_str::<ToServer>(&text).unwrap(), hello);

        let peer = ToDevice::Peer(PeerInfo {
            key: key_to_text(&[1; 32]),
            name: "PS5".into(),
            kind: KIND_PS5.into(),
            address: Ipv4Addr::new(10, 77, 0, 2),
            candidates: vec![],
            online: true,
            version: None,
        });
        let text = serde_json::to_string(&peer).unwrap();
        assert!(text.starts_with(r#"{"t":"peer""#), "{text}");
        assert_eq!(serde_json::from_str::<ToDevice>(&text).unwrap(), peer);
        assert_eq!(serde_json::from_str::<ToDevice>(r#"{"t":"removed"}"#).unwrap(), ToDevice::Removed);
    }

    #[test]
    fn codes_and_names() {
        assert_eq!(normalize_code(" abcd-efgh "), "ABCDEFGH");
        assert_eq!(format_code("ABCDEFGH"), "ABCD-EFGH");
        assert_eq!(clean_name("  \u{7}Living room ", "PS5"), "Living room");
        assert_eq!(clean_name("", "PS5"), "PS5");
        assert_eq!(key_from_text(&key_to_text(&[9; 32])), Some([9; 32]));
        assert_eq!(key_from_text("short"), None);
    }
}
