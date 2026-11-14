//! The device's session with its server: hellos, requests and their replies.

use std::net::{SocketAddr, ToSocketAddrs};
use std::sync::mpsc::Sender;
use std::time::{Duration, Instant};

use boringtun::noise::Tunn;
use boringtun::x25519::{PublicKey, StaticSecret};
use link_proto::ToServer;

/// Session index of the server; peers use the following ones.
pub const SERVER_SLOT: u32 = 1;
/// A hello every 25 s keeps home routers' mappings to the server open.
const HELLO_INTERVAL: Duration = Duration::from_secs(25);
const HELLO_TIMEOUT: Duration = Duration::from_secs(5);
/// The server counts as reachable while it answered within this time.
const CONNECTED_WINDOW: Duration = Duration::from_secs(60);
const RESOLVE_RETRY: Duration = Duration::from_secs(30);
const FORCE_HANDSHAKE_EVERY: Duration = Duration::from_secs(10);
const REQUEST_RETRY: Duration = Duration::from_secs(2);
const REQUEST_TIMEOUT: Duration = Duration::from_secs(12);

pub struct Invite {
    pub code: String,
    pub expires_in: u64,
}

pub enum ReplyTo {
    Invite(Sender<Result<Invite, String>>),
    Done(Sender<Result<(), String>>),
}

pub struct Request {
    pub id: u32,
    pub message: ToServer,
    pub reply: ReplyTo,
    first: Instant,
    last_sent: Option<Instant>,
}

pub struct Me {
    pub name: String,
    pub network: String,
    pub endpoint: Option<SocketAddr>,
}

pub struct ServerLink {
    pub key: [u8; 32],
    pub tunn: Tunn,
    pub host: String,
    pub addr: Option<SocketAddr>,
    last_resolve: Option<Instant>,
    last_reply: Option<Instant>,
    hello_due: Instant,
    hello_sent: Option<Instant>,
    misses: u32,
    last_forced: Option<Instant>,
    pub relay: bool,
    pub me: Option<Me>,
    pub requests: Vec<Request>,
    next_id: u32,
    pub error: Option<String>,
    pub ever_connected: bool,
    pub was_connected: bool,
    pub removed: bool,
}

impl ServerLink {
    pub fn new(secret: &StaticSecret, key: [u8; 32], host: &str) -> ServerLink {
        ServerLink {
            key,
            tunn: Tunn::new(secret.clone(), PublicKey::from(key), None, None, SERVER_SLOT, None),
            host: host.to_string(),
            addr: None,
            last_resolve: None,
            last_reply: None,
            hello_due: Instant::now(),
            hello_sent: None,
            misses: 0,
            last_forced: None,
            relay: false,
            me: None,
            requests: Vec::new(),
            next_id: 1,
            error: None,
            ever_connected: false,
            was_connected: false,
            removed: false,
        }
    }

    pub fn connected(&self, now: Instant) -> bool {
        self.last_reply.is_some_and(|t| now - t < CONNECTED_WINDOW)
    }

    /// Whether traffic may go through the server's relay right now.
    pub fn relay_available(&self, now: Instant) -> bool {
        self.relay && self.connected(now)
    }

    /// Looks the server up again when needed (at start, and every 30 s while
    /// it does not answer, in case its address changed).
    pub fn resolve_if_needed(&mut self, now: Instant) {
        let due = self.last_resolve.is_none_or(|t| now - t >= RESOLVE_RETRY);
        if !due || (self.addr.is_some() && self.connected(now)) {
            return;
        }
        self.last_resolve = Some(now);
        match self.host.to_socket_addrs().map(|addrs| addrs.filter(SocketAddr::is_ipv4).next()) {
            Ok(Some(addr)) => {
                if self.addr != Some(addr) {
                    crate::log!("server {} is at {addr}", self.host);
                }
                self.addr = Some(addr);
                self.error = None;
            }
            Ok(None) => self.error = Some(format!("{} has no IPv4 address", self.host)),
            Err(e) => self.error = Some(format!("cannot look up {}: {e}", self.host)),
        }
    }

    /// Whether a hello should go out now. Counts unanswered hellos.
    pub fn hello_due(&mut self, now: Instant) -> bool {
        if let Some(sent) = self.hello_sent {
            if now - sent < HELLO_TIMEOUT {
                return false;
            }
            self.hello_sent = None;
            self.misses += 1;
            return true;
        }
        now >= self.hello_due
    }

    pub fn hello_sent(&mut self, now: Instant) {
        self.hello_sent = Some(now);
    }

    /// Sends the next hello right away (the device's addresses changed).
    pub fn hello_soon(&mut self) {
        self.hello_due = Instant::now();
    }

    /// After two unanswered hellos, start a new session: the server may have
    /// restarted and lost the old one.
    pub fn should_force_handshake(&mut self, now: Instant) -> bool {
        if self.misses >= 2 && self.last_forced.is_none_or(|t| now - t >= FORCE_HANDSHAKE_EVERY) {
            self.last_forced = Some(now);
            return true;
        }
        false
    }

    /// The server answered a hello.
    pub fn answered(&mut self, now: Instant) {
        self.last_reply = Some(now);
        self.hello_sent = None;
        self.misses = 0;
        self.hello_due = now + HELLO_INTERVAL;
        self.ever_connected = true;
    }

    pub fn add_request(&mut self, make: impl FnOnce(u32) -> ToServer, reply: ReplyTo) {
        let id = self.next_id;
        self.next_id = self.next_id.wrapping_add(1).max(1);
        self.requests.push(Request { id, message: make(id), reply, first: Instant::now(), last_sent: None });
    }

    /// Requests to (re)send now; requests without an answer in time fail.
    pub fn due_requests(&mut self, now: Instant) -> Vec<ToServer> {
        let (expired, waiting): (Vec<Request>, Vec<Request>) =
            self.requests.drain(..).partition(|r| now - r.first >= REQUEST_TIMEOUT);
        self.requests = waiting;
        for request in expired {
            fail(request.reply, "The server did not answer. Check the connection and try again.");
        }
        let mut due = Vec::new();
        for request in &mut self.requests {
            if request.last_sent.is_none_or(|t| now - t >= REQUEST_RETRY) {
                request.last_sent = Some(now);
                due.push(request.message.clone());
            }
        }
        due
    }

    pub fn complete(&mut self, id: u32, code: Option<String>, expires_in: Option<u64>, error: Option<String>) {
        let Some(position) = self.requests.iter().position(|r| r.id == id) else { return };
        let request = self.requests.remove(position);
        match (request.reply, error) {
            (reply, Some(error)) => fail(reply, &error),
            (ReplyTo::Invite(tx), None) => {
                let _ = tx.send(code.map(|code| Invite { code, expires_in: expires_in.unwrap_or(0) }).ok_or_else(|| "The server sent no code.".to_string()));
            }
            (ReplyTo::Done(tx), None) => {
                let _ = tx.send(Ok(()));
            }
        }
    }

    /// Fails every waiting request, for example when the engine stops.
    pub fn fail_all(&mut self, message: &str) {
        for request in self.requests.drain(..) {
            fail(request.reply, message);
        }
    }
}

fn fail(reply: ReplyTo, message: &str) {
    match reply {
        ReplyTo::Invite(tx) => {
            let _ = tx.send(Err(message.to_string()));
        }
        ReplyTo::Done(tx) => {
            let _ = tx.send(Err(message.to_string()));
        }
    }
}
