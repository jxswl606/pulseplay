//! What a device remembers: its keys, its network and its port forwards.
//! Saved as JSON in the data directory.

use std::fs;
use std::io::Write;
use std::net::{Ipv4Addr, SocketAddr};
use std::path::Path;

use serde::de::DeserializeOwned;
use serde::{Deserialize, Serialize};

pub const STATE_FILE: &str = "orbit-link.json";
/// The last list of other devices, so direct connections can start even when
/// the server is unreachable.
pub const PEERS_FILE: &str = "peers.json";

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct DeviceState {
    pub version: u32,
    pub private_key: String,
    /// Address used to join, such as `https://link.example.com`.
    pub server: String,
    /// `host:port` of the server's UDP port.
    pub endpoint: String,
    pub server_key: String,
    pub device_id: String,
    pub network_id: String,
    /// Name of the network.
    pub network: String,
    pub name: String,
    pub kind: String,
    pub address: Ipv4Addr,
    /// Let other devices use this device's payload loader (port 9021).
    #[serde(default)]
    pub allow_loader: bool,
    #[serde(default)]
    pub forwards: Vec<Forward>,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Tcp,
    Udp,
}

impl Protocol {
    pub fn as_str(self) -> &'static str {
        match self {
            Protocol::Tcp => "tcp",
            Protocol::Udp => "udp",
        }
    }
    pub fn parse(text: &str) -> Option<Protocol> {
        match text.trim().to_ascii_lowercase().as_str() {
            "tcp" => Some(Protocol::Tcp),
            "udp" => Some(Protocol::Udp),
            _ => None,
        }
    }
}

/// Makes a port of another device available on this one, such as the PS5's
/// Remote Play ports on the Mac's `127.0.0.1`.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Forward {
    /// Public key of the device that offers the port.
    pub peer: String,
    pub protocol: Protocol,
    /// Port on that device.
    pub port: u16,
    /// Where this device listens.
    pub listen: SocketAddr,
}

/// Ports Remote Play and the Orbit Play helper use, forwarded together.
/// Registration first looks for the console with a UDP packet to 9295, then
/// registers over TCP 9295.
pub const REMOTE_PLAY: [(Protocol, u16); 6] = [
    (Protocol::Tcp, 9295),
    (Protocol::Udp, 9295),
    (Protocol::Udp, 9296),
    (Protocol::Udp, 9297),
    (Protocol::Udp, 9302),
    (Protocol::Tcp, 34179),
];

pub fn load<T: DeserializeOwned>(path: &Path) -> Result<Option<T>, String> {
    match fs::read_to_string(path) {
        Ok(text) => serde_json::from_str(&text).map(Some).map_err(|e| format!("{} is damaged: {e}", path.display())),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => Ok(None),
        Err(e) => Err(format!("cannot read {}: {e}", path.display())),
    }
}

/// Writes atomically, readable only by the owner.
pub fn save<T: Serialize>(path: &Path, value: &T) -> Result<(), String> {
    let text = serde_json::to_string_pretty(value).map_err(|e| e.to_string())?;
    let mut temp = path.as_os_str().to_owned();
    temp.push(".tmp");
    let temp = std::path::PathBuf::from(temp);
    let write = || -> std::io::Result<()> {
        let mut file = fs::File::create(&temp)?;
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            let _ = file.set_permissions(fs::Permissions::from_mode(0o600));
        }
        file.write_all(text.as_bytes())?;
        file.sync_all().ok();
        fs::rename(&temp, path)
    };
    write().map_err(|e| format!("cannot save {}: {e}", path.display()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn state_round_trips_and_tolerates_missing_fields() {
        let dir = std::env::temp_dir()
            .join(format!("orbit-link-state-{:x}", u64::from_le_bytes(crate::keys::random_bytes())));
        fs::create_dir_all(&dir).unwrap();
        let path = dir.join("state-test.json");
        let state = DeviceState {
            version: 2,
            private_key: "k".into(),
            server: "https://link.example.com".into(),
            endpoint: "link.example.com:41700".into(),
            server_key: "s".into(),
            device_id: "d".into(),
            network_id: "n".into(),
            network: "Home".into(),
            name: "PS5".into(),
            kind: "ps5".into(),
            address: Ipv4Addr::new(10, 77, 0, 2),
            allow_loader: false,
            forwards: vec![Forward {
                peer: "p".into(),
                protocol: Protocol::Udp,
                port: 9296,
                listen: "127.0.0.1:9296".parse().unwrap(),
            }],
        };
        save(&path, &state).unwrap();
        assert_eq!(load::<DeviceState>(&path).unwrap(), Some(state));
        fs::write(&path, r#"{"version":2,"private_key":"k","server":"s","endpoint":"e","server_key":"s","device_id":"d",
            "network_id":"n","network":"Home","name":"PS5","kind":"ps5","address":"10.77.0.2"}"#)
        .unwrap();
        let old: DeviceState = load(&path).unwrap().unwrap();
        assert!(old.forwards.is_empty() && !old.allow_loader);
        assert_eq!(load::<DeviceState>(&dir.join("missing.json")).unwrap(), None);
        fs::remove_file(path).ok();
    }
}
