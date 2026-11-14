//! Asks the home router to forward this device's UDP port, with UPnP (IGD) or
//! NAT-PMP. Many home routers allow this by default.
//!
//! A forwarded port accepts packets from anywhere, so another device can
//! reach this one directly even from behind a strict ("symmetric") NAT, such
//! as many mobile networks, where hole punching alone fails.
//!
//! Runs on its own thread: discovery and requests block for up to a few
//! seconds. Mappings are leased for an hour and renewed every 20 minutes. They
//! are not removed when Orbit Link stops (a restarted engine reuses the same
//! port at once); the lease ends them.

use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4, UdpSocket};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use crate::http;
use crate::net::{is_private, lan_address};

const LEASE_SECONDS: u32 = 3600;
const RENEW_EVERY: Duration = Duration::from_secs(20 * 60);
const RETRY_EVERY: Duration = Duration::from_secs(5 * 60);
const SSDP: (Ipv4Addr, u16) = (Ipv4Addr::new(239, 255, 255, 250), 1900);
const NAT_PMP_PORT: u16 = 5351;
const DESCRIPTION: &str = "Orbit Link";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Mapping {
    /// The address other devices can reach this one at.
    pub external: SocketAddrV4,
    /// `UPnP` or `NAT-PMP`
    pub method: &'static str,
}

/// Keeps a mapping alive while it exists.
pub struct PortMapper {
    current: Arc<Mutex<Option<Mapping>>>,
    stop: Arc<AtomicBool>,
    retry: Arc<AtomicBool>,
}

impl PortMapper {
    pub fn start(port: u16) -> PortMapper {
        let current = Arc::new(Mutex::new(None));
        let stop = Arc::new(AtomicBool::new(false));
        let retry = Arc::new(AtomicBool::new(false));
        let (thread_current, thread_stop, thread_retry) = (current.clone(), stop.clone(), retry.clone());
        let _ = thread::Builder::new()
            .name("orbit-link-portmap".into())
            .spawn(move || run(port, &thread_current, &thread_stop, &thread_retry));
        PortMapper { current, stop, retry }
    }

    pub fn current(&self) -> Option<Mapping> {
        self.current.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    /// Tries again soon, for example after the local network changed.
    pub fn retry(&self) {
        self.retry.store(true, Ordering::Relaxed);
    }
}

impl Drop for PortMapper {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
    }
}

/// How a mapping was made, to renew or remove it.
enum Router {
    Upnp { control: String, service: String, internal: Ipv4Addr },
    NatPmp { gateway: Ipv4Addr },
}

fn run(port: u16, current: &Mutex<Option<Mapping>>, stop: &AtomicBool, retry: &AtomicBool) {
    let mut held: Option<(Router, Mapping)> = None;
    loop {
        let found = match &held {
            Some((router, mapping)) => renew(router, port, mapping.external.port()).map(|m| (router, m)).is_some(),
            None => false,
        };
        if !found {
            held = discover(port);
            match &held {
                Some((_, mapping)) => crate::log!("router forwards {} to UDP {port} ({})", mapping.external, mapping.method),
                None => crate::log!("no UPnP or NAT-PMP router found; relying on hole punching"),
            }
        }
        *current.lock().unwrap_or_else(|e| e.into_inner()) = held.as_ref().map(|(_, m)| m.clone());
        let wait = if held.is_some() { RENEW_EVERY } else { RETRY_EVERY };
        let until = Instant::now() + wait;
        while Instant::now() < until {
            if stop.load(Ordering::Relaxed) {
                return;
            }
            if retry.swap(false, Ordering::Relaxed) {
                break;
            }
            thread::sleep(Duration::from_millis(500));
        }
    }
}

fn discover(port: u16) -> Option<(Router, Mapping)> {
    let lan = lan_address()?;
    let gateways = ssdp_search();
    for location in &gateways {
        if let Some(found) = upnp_map(location, lan, port) {
            return Some(found);
        }
    }
    // NAT-PMP needs the router's address: the one that answered SSDP, or the
    // usual .1 of the local network.
    let mut candidates: Vec<Ipv4Addr> = gateways.iter().filter_map(|l| http::parse_url(l).ok()?.host.parse().ok()).collect();
    let [a, b, c, _] = lan.octets();
    candidates.push(Ipv4Addr::new(a, b, c, 1));
    candidates.dedup();
    for gateway in candidates {
        if let Some(mapping) = nat_pmp_map(gateway, port, LEASE_SECONDS) {
            return Some((Router::NatPmp { gateway }, mapping));
        }
    }
    None
}

fn renew(router: &Router, port: u16, external_port: u16) -> Option<Mapping> {
    match router {
        Router::Upnp { control, service, internal } => {
            add_port_mapping(control, service, *internal, port, external_port).ok()?;
            let ip = external_ip(control, service)?;
            Some(Mapping { external: SocketAddrV4::new(ip, external_port), method: "UPnP" })
        }
        Router::NatPmp { gateway } => nat_pmp_map(*gateway, port, LEASE_SECONDS),
    }
}

// --- UPnP IGD ---

/// Sends an SSDP search and returns the description URLs of answering routers.
fn ssdp_search() -> Vec<String> {
    let Ok(socket) = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)) else { return Vec::new() };
    let _ = socket.set_read_timeout(Some(Duration::from_millis(300)));
    for target in [
        "urn:schemas-upnp-org:device:InternetGatewayDevice:1",
        "urn:schemas-upnp-org:device:InternetGatewayDevice:2",
        "urn:schemas-upnp-org:service:WANIPConnection:1",
    ] {
        let search = format!(
            "M-SEARCH * HTTP/1.1\r\nHOST: 239.255.255.250:1900\r\nMAN: \"ssdp:discover\"\r\nMX: 2\r\nST: {target}\r\n\r\n"
        );
        let _ = socket.send_to(search.as_bytes(), SSDP);
    }
    let mut locations = Vec::new();
    let deadline = Instant::now() + Duration::from_millis(2500);
    let mut buffer = [0u8; 2048];
    while Instant::now() < deadline {
        let Ok((n, _)) = socket.recv_from(&mut buffer) else { continue };
        let text = String::from_utf8_lossy(&buffer[..n]);
        for line in text.lines() {
            if let Some((name, value)) = line.split_once(':') {
                let value = value.trim().to_string();
                if name.trim().eq_ignore_ascii_case("location") && value.starts_with("http://") && !locations.contains(&value) {
                    locations.push(value);
                }
            }
        }
    }
    locations
}

/// Splits `http://host:port/path` into (`http://host:port`, `/path`).
fn split_url(url: &str) -> Option<(String, String)> {
    let rest = url.strip_prefix("http://")?;
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, "/"),
    };
    Some((format!("http://{authority}"), path.to_string()))
}

/// The text inside the first `<tag>…</tag>` in `xml`, ignoring namespace prefixes.
fn element<'a>(xml: &'a str, tag: &str) -> Option<&'a str> {
    let mut rest = xml;
    loop {
        let start = rest.find('<')?;
        let after = &rest[start + 1..];
        let end = after.find('>')?;
        let name = after[..end].split_whitespace().next().unwrap_or("");
        let local = name.rsplit(':').next().unwrap_or(name);
        if local == tag && !name.starts_with('/') {
            let body = &after[end + 1..];
            let close = body.find("</")?;
            return Some(body[..close].trim());
        }
        rest = &after[end + 1..];
    }
}

/// The WAN connection service of a router: (control URL, service type).
fn wan_service(location: &str) -> Option<(String, String)> {
    let (base, path) = split_url(location)?;
    let (200, xml) = http::get(&base, &path).ok()? else { return None };
    for block in xml.split("<service>").skip(1) {
        let service = element(block, "serviceType")?;
        if !(service.contains(":WANIPConnection:") || service.contains(":WANPPPConnection:")) {
            continue;
        }
        let control = element(block, "controlURL")?;
        let control = if control.starts_with("http://") {
            control.to_string()
        } else {
            let base = element(&xml, "URLBase").map(|b| b.trim_end_matches('/').to_string()).unwrap_or(base.clone());
            format!("{base}/{}", control.trim_start_matches('/'))
        };
        return Some((control, service.to_string()));
    }
    None
}

fn soap(control: &str, service: &str, action: &str, arguments: &str) -> Result<String, String> {
    let (base, path) = split_url(control).ok_or("bad control URL")?;
    let body = format!(
        "<?xml version=\"1.0\"?><s:Envelope xmlns:s=\"http://schemas.xmlsoap.org/soap/envelope/\" \
         s:encodingStyle=\"http://schemas.xmlsoap.org/soap/encoding/\"><s:Body>\
         <u:{action} xmlns:u=\"{service}\">{arguments}</u:{action}></s:Body></s:Envelope>"
    );
    let soap_action = format!("\"{service}#{action}\"");
    let (status, text) = http::call(
        &base,
        "POST",
        &path,
        &[("SOAPAction", &soap_action)],
        Some(("text/xml; charset=\"utf-8\"", body.as_bytes())),
    )?;
    if status == 200 {
        Ok(text)
    } else {
        Err(element(&text, "errorCode").unwrap_or("unknown").to_string())
    }
}

fn add_port_mapping(control: &str, service: &str, internal: Ipv4Addr, port: u16, external: u16) -> Result<(), String> {
    let mut lease = LEASE_SECONDS;
    loop {
        let arguments = format!(
            "<NewRemoteHost></NewRemoteHost><NewExternalPort>{external}</NewExternalPort><NewProtocol>UDP</NewProtocol>\
             <NewInternalPort>{port}</NewInternalPort><NewInternalClient>{internal}</NewInternalClient>\
             <NewEnabled>1</NewEnabled><NewPortMappingDescription>{DESCRIPTION}</NewPortMappingDescription>\
             <NewLeaseDuration>{lease}</NewLeaseDuration>"
        );
        match soap(control, service, "AddPortMapping", &arguments) {
            // 725: the router only supports permanent mappings.
            Err(code) if code == "725" && lease != 0 => lease = 0,
            result => return result.map(|_| ()),
        }
    }
}

fn external_ip(control: &str, service: &str) -> Option<Ipv4Addr> {
    let text = soap(control, service, "GetExternalIPAddress", "").ok()?;
    let ip: Ipv4Addr = element(&text, "NewExternalIPAddress")?.parse().ok()?;
    // Behind a second NAT (such as a carrier's), the router's own address is
    // private and a mapping on it does not help.
    (!is_private(ip) && !ip.is_unspecified()).then_some(ip)
}

fn upnp_map(location: &str, internal: Ipv4Addr, port: u16) -> Option<(Router, Mapping)> {
    let (control, service) = wan_service(location)?;
    let ip = external_ip(&control, &service)?;
    // The same port outside when free; otherwise a few others.
    for external in [port, port.wrapping_add(1), port.wrapping_add(2), port.wrapping_add(10), port.wrapping_add(100)] {
        match add_port_mapping(&control, &service, internal, port, external) {
            Ok(()) => {
                let mapping = Mapping { external: SocketAddrV4::new(ip, external), method: "UPnP" };
                return Some((Router::Upnp { control, service, internal }, mapping));
            }
            Err(code) if code == "718" => continue, // taken by another device
            Err(_) => return None,
        }
    }
    None
}

// --- NAT-PMP (RFC 6886) ---

fn nat_pmp_request(gateway: Ipv4Addr, request: &[u8], expected_len: usize) -> Option<Vec<u8>> {
    let socket = UdpSocket::bind((Ipv4Addr::UNSPECIFIED, 0)).ok()?;
    socket.connect(SocketAddr::from((gateway, NAT_PMP_PORT))).ok()?;
    let mut buffer = [0u8; 64];
    for timeout in [250, 500, 1000] {
        socket.set_read_timeout(Some(Duration::from_millis(timeout))).ok()?;
        socket.send(request).ok()?;
        if let Ok(n) = socket.recv(&mut buffer) {
            // version 0, response opcode, result code 0 (success)
            if n >= expected_len && buffer[0] == 0 && buffer[1] == request[1] + 128 && buffer[2..4] == [0, 0] {
                return Some(buffer[..n].to_vec());
            }
            return None;
        }
    }
    None
}

fn nat_pmp_map(gateway: Ipv4Addr, port: u16, lifetime: u32) -> Option<Mapping> {
    let address = nat_pmp_request(gateway, &[0, 0], 12)?;
    let ip = Ipv4Addr::new(address[8], address[9], address[10], address[11]);
    let mut request = vec![0, 1, 0, 0];
    request.extend_from_slice(&port.to_be_bytes());
    request.extend_from_slice(&port.to_be_bytes());
    request.extend_from_slice(&lifetime.to_be_bytes());
    let reply = nat_pmp_request(gateway, &request, 16)?;
    let external = u16::from_be_bytes([reply[10], reply[11]]);
    if lifetime == 0 || is_private(ip) || ip.is_unspecified() {
        return None;
    }
    Some(Mapping { external: SocketAddrV4::new(ip, external), method: "NAT-PMP" })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_router_descriptions() {
        let xml = r#"<?xml version="1.0"?><root><URLBase>http://192.168.1.1:5000/</URLBase><device><serviceList>
            <service><serviceType>urn:schemas-upnp-org:service:Layer3Forwarding:1</serviceType><controlURL>/L3F</controlURL></service>
            <service><serviceType>urn:schemas-upnp-org:service:WANIPConnection:1</serviceType>
            <controlURL>/ctl/IPConn</controlURL></service></serviceList></device></root>"#;
        assert_eq!(element(xml, "URLBase"), Some("http://192.168.1.1:5000/"));
        let block = xml.split("<service>").nth(2).unwrap();
        assert_eq!(element(block, "controlURL"), Some("/ctl/IPConn"));
        let reply = r#"<s:Envelope><s:Body><u:GetExternalIPAddressResponse xmlns:u="x">
            <NewExternalIPAddress>203.0.113.9</NewExternalIPAddress></u:GetExternalIPAddressResponse></s:Body></s:Envelope>"#;
        assert_eq!(element(reply, "NewExternalIPAddress"), Some("203.0.113.9"));
        assert_eq!(split_url("http://192.168.1.1:5000/rootDesc.xml").unwrap(), ("http://192.168.1.1:5000".into(), "/rootDesc.xml".into()));
    }
}
