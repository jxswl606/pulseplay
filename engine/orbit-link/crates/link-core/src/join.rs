//! Joining a network: the one HTTP exchange with the server. Everything after
//! it runs over the device's encrypted UDP session.

use link_proto::{ApiError, JoinRequest, JoinResponse, ServerInfo, API_INFO, API_JOIN};

use crate::http;
use crate::keys;
use crate::state::DeviceState;

pub enum JoinWith {
    /// A one-time code from another device or the server's dashboard.
    Code(String),
    /// Create a new network with this name (servers that allow it).
    Create(String),
}

/// `link.example.com` becomes `https://link.example.com`.
pub fn normalize_server(server: &str) -> Result<String, String> {
    let url = http::parse_url(server)?;
    let scheme = if url.https { "https" } else { "http" };
    let default_port = if url.https { 443 } else { 80 };
    let host = if url.host.contains(':') { format!("[{}]", url.host) } else { url.host };
    Ok(if url.port == default_port {
        format!("{scheme}://{host}{}", url.base_path)
    } else {
        format!("{scheme}://{host}:{}{}", url.port, url.base_path)
    })
}

pub fn server_info(server: &str) -> Result<ServerInfo, String> {
    match http::get(server, API_INFO)? {
        (200, body) => serde_json::from_str(&body).map_err(|_| "that address is not an Orbit Link server".to_string()),
        (404, _) => Err("that address is not an Orbit Link server (or runs an older version)".into()),
        (status, body) => Err(error_text(status, &body)),
    }
}

/// Joins a network and returns the device's new state. Generates the device's
/// key here; the private key never leaves the device.
pub fn join(server: &str, with: JoinWith, name: &str, kind: &str) -> Result<DeviceState, String> {
    let server = normalize_server(server)?;
    let (private_key, public_key) = keys::generate();
    let (code, create) = match with {
        JoinWith::Code(code) => (Some(link_proto::normalize_code(&code)), None),
        JoinWith::Create(network) => (None, Some(network)),
    };
    let request = JoinRequest { code, create, public_key, name: name.to_string(), kind: kind.to_string() };
    let body = serde_json::to_string(&request).map_err(|e| e.to_string())?;
    let (status, text) = http::post_json(&server, API_JOIN, &body).map_err(|e| format!("Could not reach {server}: {e}"))?;
    if status != 200 {
        return Err(error_text(status, &text));
    }
    let joined: JoinResponse =
        serde_json::from_str(&text).map_err(|_| "the server sent an answer this version cannot read".to_string())?;
    Ok(DeviceState {
        version: 2,
        private_key,
        server,
        endpoint: joined.endpoint,
        server_key: joined.server_key,
        device_id: joined.device_id,
        network_id: joined.network_id,
        network: joined.network,
        name: joined.name,
        kind: kind.to_string(),
        address: joined.address,
        allow_loader: false,
        forwards: Vec::new(),
    })
}

fn error_text(status: u16, body: &str) -> String {
    if let Ok(error) = serde_json::from_str::<ApiError>(body) {
        return error.error;
    }
    match status {
        404 => "That code is not valid or has expired. Create a new one.".into(),
        429 => "Too many attempts. Wait a minute and try again.".into(),
        _ => {
            let body: String = body.chars().take(200).collect();
            format!("The server answered {status}: {body}")
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalizes_server_addresses() {
        assert_eq!(normalize_server("link.example.com/").unwrap(), "https://link.example.com");
        assert_eq!(normalize_server("http://192.168.1.5:8080").unwrap(), "http://192.168.1.5:8080");
        assert_eq!(normalize_server("https://example.com:443/orbit").unwrap(), "https://example.com/orbit");
        assert!(normalize_server("").is_err());
        assert_eq!(error_text(403, r#"{"error":"Only devices with a code can join this server."}"#), "Only devices with a code can join this server.");
    }
}
