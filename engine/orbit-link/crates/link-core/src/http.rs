//! Minimal HTTP/1.1 helpers: a client for joining a server and the request
//! parsing used by the device's setup page.

use std::io::{BufRead, BufReader, ErrorKind, Read, Write};
use std::net::{TcpStream, ToSocketAddrs};
use std::sync::Arc;
use std::time::Duration;

const TIMEOUT: Duration = Duration::from_secs(15);
const MAX_RESPONSE: usize = 256 * 1024;

#[derive(Debug, PartialEq, Eq)]
pub struct Url {
    pub https: bool,
    pub host: String,
    pub port: u16,
    pub base_path: String,
}

/// Parses a server address. Without a scheme, HTTPS is assumed.
pub fn parse_url(text: &str) -> Result<Url, String> {
    let text = text.trim().trim_end_matches('/');
    let (https, rest) = if let Some(rest) = text.strip_prefix("https://") {
        (true, rest)
    } else if let Some(rest) = text.strip_prefix("http://") {
        (false, rest)
    } else if text.contains("://") {
        return Err("the server address must start with https:// or http://".into());
    } else {
        (true, text)
    };
    let (authority, path) = match rest.find('/') {
        Some(i) => (&rest[..i], &rest[i..]),
        None => (rest, ""),
    };
    if authority.is_empty() {
        return Err("the server address is empty".into());
    }
    let default_port = if https { 443 } else { 80 };
    let (host, port) = if let Some(stripped) = authority.strip_prefix('[') {
        let end = stripped.find(']').ok_or("bad IPv6 address")?;
        let port = match &stripped[end + 1..] {
            "" => default_port,
            p => p.trim_start_matches(':').parse().map_err(|_| "bad port")?,
        };
        (stripped[..end].to_string(), port)
    } else {
        match authority.rsplit_once(':') {
            Some((host, port)) => (host.to_string(), port.parse().map_err(|_| "bad port in server address")?),
            None => (authority.to_string(), default_port),
        }
    };
    Ok(Url { https, host, port, base_path: path.to_string() })
}

trait Stream: Read + Write {}
impl<T: Read + Write> Stream for T {}

fn connect(url: &Url) -> Result<Box<dyn Stream>, String> {
    let addrs = (url.host.as_str(), url.port)
        .to_socket_addrs()
        .map_err(|e| format!("cannot look up {}: {e}", url.host))?;
    let mut last = format!("{} has no address", url.host);
    for addr in addrs {
        match TcpStream::connect_timeout(&addr, TIMEOUT) {
            Ok(tcp) => {
                tcp.set_read_timeout(Some(TIMEOUT)).ok();
                tcp.set_write_timeout(Some(TIMEOUT)).ok();
                if !url.https {
                    return Ok(Box::new(tcp));
                }
                let mut roots = rustls::RootCertStore::empty();
                roots.extend(webpki_roots::TLS_SERVER_ROOTS.iter().cloned());
                let config = rustls::ClientConfig::builder_with_provider(Arc::new(rustls::crypto::ring::default_provider()))
                    .with_safe_default_protocol_versions()
                    .map_err(|e| e.to_string())?
                    .with_root_certificates(roots)
                    .with_no_client_auth();
                let name = rustls::pki_types::ServerName::try_from(url.host.clone())
                    .map_err(|_| format!("bad server name {}", url.host))?;
                let conn = rustls::ClientConnection::new(Arc::new(config), name).map_err(|e| e.to_string())?;
                return Ok(Box::new(rustls::StreamOwned::new(conn, tcp)));
            }
            Err(e) => last = format!("cannot connect to {addr}: {e}"),
        }
    }
    Err(last)
}

/// Sends a request and returns the status code and body.
pub fn call(
    server: &str,
    method: &str,
    path: &str,
    headers: &[(&str, &str)],
    body: Option<(&str, &[u8])>,
) -> Result<(u16, String), String> {
    let url = parse_url(server)?;
    let host_header = if (url.https && url.port == 443) || (!url.https && url.port == 80) {
        url.host.clone()
    } else {
        format!("{}:{}", url.host, url.port)
    };
    let mut request = format!(
        "{method} {}{} HTTP/1.1\r\nHost: {}\r\nUser-Agent: orbit-link/{}\r\nAccept: application/json\r\nConnection: close\r\n",
        url.base_path,
        path,
        host_header,
        crate::VERSION,
    )
    .into_bytes();
    for (name, value) in headers {
        request.extend_from_slice(format!("{name}: {value}\r\n").as_bytes());
    }
    match body {
        Some((content_type, data)) => {
            request.extend_from_slice(format!("Content-Type: {content_type}\r\nContent-Length: {}\r\n\r\n", data.len()).as_bytes());
            request.extend_from_slice(data);
        }
        None => request.extend_from_slice(b"\r\n"),
    }
    let mut stream = connect(&url)?;
    stream.write_all(&request).map_err(|e| tls_error(&url, e))?;
    stream.flush().map_err(|e| tls_error(&url, e))?;
    let mut response = Vec::new();
    let mut chunk = [0u8; 8192];
    loop {
        match stream.read(&mut chunk) {
            Ok(0) => break,
            Ok(n) => {
                response.extend_from_slice(&chunk[..n]);
                if response.len() > MAX_RESPONSE {
                    return Err("the server response is too large".into());
                }
            }
            Err(e) if e.kind() == ErrorKind::Interrupted => {}
            // Some servers close TLS without a close_notify; keep what arrived.
            Err(e) if e.kind() == ErrorKind::UnexpectedEof && !response.is_empty() => break,
            Err(e) => return Err(tls_error(&url, e)),
        }
    }
    parse_response(&response)
}

pub fn get(server: &str, path: &str) -> Result<(u16, String), String> {
    call(server, "GET", path, &[], None)
}

pub fn post_json(server: &str, path: &str, json: &str) -> Result<(u16, String), String> {
    call(server, "POST", path, &[], Some(("application/json", json.as_bytes())))
}

fn tls_error(url: &Url, error: std::io::Error) -> String {
    if url.https {
        format!("secure connection to {} failed: {error}", url.host)
    } else {
        format!("connection to {} failed: {error}", url.host)
    }
}

fn parse_response(data: &[u8]) -> Result<(u16, String), String> {
    let split = data.windows(4).position(|w| w == b"\r\n\r\n").ok_or("incomplete response from the server")?;
    let head = String::from_utf8_lossy(&data[..split]);
    let body = &data[split + 4..];
    let mut lines = head.lines();
    let status: u16 = lines
        .next()
        .and_then(|l| l.split_whitespace().nth(1))
        .and_then(|s| s.parse().ok())
        .ok_or("the server did not answer with HTTP")?;
    let mut chunked = false;
    let mut length = None;
    for line in lines {
        if let Some((name, value)) = line.split_once(':') {
            let name = name.trim().to_ascii_lowercase();
            if name == "transfer-encoding" && value.to_ascii_lowercase().contains("chunked") {
                chunked = true;
            } else if name == "content-length" {
                length = value.trim().parse::<usize>().ok();
            }
        }
    }
    let body = if chunked {
        dechunk(body)?
    } else if let Some(length) = length {
        body.get(..length).ok_or("truncated response from the server")?.to_vec()
    } else {
        body.to_vec()
    };
    Ok((status, String::from_utf8_lossy(&body).into_owned()))
}

fn dechunk(mut data: &[u8]) -> Result<Vec<u8>, String> {
    let mut out = Vec::new();
    loop {
        let end = data.windows(2).position(|w| w == b"\r\n").ok_or("bad chunked response")?;
        let size_text = String::from_utf8_lossy(&data[..end]);
        let size = usize::from_str_radix(size_text.split(';').next().unwrap_or("").trim(), 16)
            .map_err(|_| "bad chunk size")?;
        data = &data[end + 2..];
        if size == 0 {
            return Ok(out);
        }
        out.extend_from_slice(data.get(..size).ok_or("truncated chunk")?);
        data = data.get(size + 2..).ok_or("truncated chunk")?;
    }
}

pub fn encode(text: &str) -> String {
    let mut out = String::new();
    for byte in text.bytes() {
        match byte {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => out.push(byte as char),
            _ => out.push_str(&format!("%{byte:02X}")),
        }
    }
    out
}

pub fn decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        match bytes[i] {
            b'+' => out.push(b' '),
            b'%' if i + 2 < bytes.len() => {
                let hex = |b: u8| (b as char).to_digit(16);
                match (hex(bytes[i + 1]), hex(bytes[i + 2])) {
                    (Some(high), Some(low)) => {
                        out.push((high * 16 + low) as u8);
                        i += 2;
                    }
                    _ => out.push(b'%'),
                }
            }
            other => out.push(other),
        }
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

/// Parses an `application/x-www-form-urlencoded` body.
pub fn parse_form(body: &str) -> Vec<(String, String)> {
    body.split('&')
        .filter(|p| !p.is_empty())
        .map(|pair| match pair.split_once('=') {
            Some((k, v)) => (decode(k), decode(v)),
            None => (decode(pair), String::new()),
        })
        .collect()
}

pub struct Request {
    pub method: String,
    pub path: String,
    pub headers: Vec<(String, String)>,
    pub body: String,
}

impl Request {
    pub fn header(&self, name: &str) -> Option<&str> {
        self.headers.iter().find(|(k, _)| k.eq_ignore_ascii_case(name)).map(|(_, v)| v.as_str())
    }
}

/// Reads one request from a client of the setup page.
pub fn read_request(stream: &TcpStream, max_body: usize) -> Result<Request, String> {
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line).map_err(|e| e.to_string())?;
    let mut parts = line.split_whitespace();
    let method = parts.next().ok_or("empty request")?.to_string();
    let path = parts.next().ok_or("no path")?.to_string();
    let mut headers = Vec::new();
    loop {
        let mut header = String::new();
        if reader.read_line(&mut header).map_err(|e| e.to_string())? == 0 {
            break;
        }
        let header = header.trim_end();
        if header.is_empty() {
            break;
        }
        if headers.len() > 64 {
            return Err("too many headers".into());
        }
        if let Some((name, value)) = header.split_once(':') {
            headers.push((name.trim().to_string(), value.trim().to_string()));
        }
    }
    let length = headers
        .iter()
        .find(|(k, _)| k.eq_ignore_ascii_case("content-length"))
        .and_then(|(_, v)| v.parse::<usize>().ok())
        .unwrap_or(0);
    if length > max_body {
        return Err("request body too large".into());
    }
    let mut body = vec![0u8; length];
    reader.read_exact(&mut body).map_err(|e| e.to_string())?;
    Ok(Request { method, path, headers, body: String::from_utf8_lossy(&body).into_owned() })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_server_addresses() {
        assert_eq!(
            parse_url("link.example.com").unwrap(),
            Url { https: true, host: "link.example.com".into(), port: 443, base_path: String::new() }
        );
        assert_eq!(
            parse_url("http://192.168.1.20:8080/").unwrap(),
            Url { https: false, host: "192.168.1.20".into(), port: 8080, base_path: String::new() }
        );
        assert_eq!(parse_url("https://example.com/orbit").unwrap().base_path, "/orbit");
        assert!(parse_url("ftp://x").is_err());
    }

    #[test]
    fn forms_round_trip() {
        let encoded = format!("server={}&code={}", encode("https://a.b/c d"), encode("AB12-CD34"));
        let form = parse_form(&encoded);
        assert_eq!(form[0], ("server".into(), "https://a.b/c d".into()));
        assert_eq!(form[1], ("code".into(), "AB12-CD34".into()));
        assert_eq!(decode("a+b%2"), "a b%2");
    }

    #[test]
    fn reads_chunked_and_sized_responses() {
        let chunked = b"HTTP/1.1 200 OK\r\nTransfer-Encoding: chunked\r\n\r\n5\r\nhello\r\n6\r\n world\r\n0\r\n\r\n";
        assert_eq!(parse_response(chunked).unwrap(), (200, "hello world".into()));
        let sized = b"HTTP/1.1 404 Not Found\r\nContent-Length: 4\r\n\r\ngone";
        assert_eq!(parse_response(sized).unwrap(), (404, "gone".into()));
    }
}
