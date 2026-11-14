//! Device application: runs the engine and serves the setup and status page
//! (port 34180) with a small JSON API. [`Embedded`] runs the same engine and
//! actions inside another program (the Orbit Play helper on the PS5), without
//! the page.
//!
//! Every action is available as a form post for the page and as a JSON call
//! under `/api/` for other programs (such as Orbit Play):
//!
//! | Action | Fields |
//! | --- | --- |
//! | `setup` | `server`, then `code` to join or `create` (network name) to start one; optional `name` |
//! | `invite` | none; returns `code` and `expires_in` |
//! | `remove-device`, `rename-device` | `key`, (`name`) |
//! | `add-forward` | `peer`, then `preset` = `remote-play`, or `protocol`, `port` and optional `local_port`, `bind` |
//! | `remove-forward` | `peer`, `protocol`, `port`, `listen` (or `index`) |
//! | `loader` | `allow` (`true`/`false`) |
//! | `restart`, `leave`, `stop` | none |

use std::fs;
use std::io::Write;
use std::net::{IpAddr, Ipv4Addr, SocketAddr, TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Mutex, Weak};
use std::thread::{self, JoinHandle};
use std::time::Duration;

use serde_json::{json, Value};

use crate::http::{self, Request};
use crate::join::{self, JoinWith};
use crate::node::{Command, Engine, Event, Invite, ServerState, Shared, Status};
use crate::proto::{self, KIND_PS5};
use crate::state::{self, DeviceState, Forward, Protocol, REMOTE_PLAY};
use crate::ui::{self, escape};

const MAX_CLIENTS: usize = 16;
const REPLY_WAIT: Duration = Duration::from_secs(15);

/// What the host platform provides.
pub trait Platform: Send + Sync + 'static {
    fn notify(&self, message: &str);
    fn data_dir(&self) -> PathBuf;
    /// `ps5` or `computer`
    fn kind(&self) -> &'static str;
    /// Suggested name for this device, such as "PS5" or the computer's name.
    fn default_name(&self) -> String;
    /// Offer this machine's ports to the other devices.
    fn serve(&self) -> bool;
}

struct EngineHandle {
    shared: Arc<Shared>,
    thread: JoinHandle<()>,
}

struct App {
    platform: Arc<dyn Platform>,
    state: Mutex<Option<DeviceState>>,
    engine: Mutex<Option<EngineHandle>>,
    clients: AtomicUsize,
    /// The setup page's address; none when embedded.
    bind: Option<SocketAddr>,
    myself: Weak<App>,
    /// Results of recent form posts, shown after the redirect back to the page.
    notices: Mutex<Vec<(u64, Notice)>>,
    /// Changes whenever the engine starts or the setup is forgotten, so a late
    /// "removed" event cannot forget a newer setup.
    generation: AtomicU64,
}

/// What an action produced.
#[derive(Default)]
struct Outcome {
    message: String,
    invite: Option<Invite>,
    stop: bool,
}

impl Outcome {
    fn says(message: impl Into<String>) -> Outcome {
        Outcome { message: message.into(), ..Outcome::default() }
    }
}

/// Creates the data folder and opens the log there.
fn prepare(platform: &dyn Platform) -> Result<(), String> {
    let dir = platform.data_dir();
    if let Err(e) = fs::create_dir_all(&dir) {
        let message = format!("Orbit Link cannot create {}: {e}", dir.display());
        platform.notify(&message);
        return Err(message);
    }
    crate::log::init(dir.join("orbit-link.log"));
    Ok(())
}

/// Runs the device application. Returns only if the page's port is in use,
/// which means another copy is already running.
pub fn run(platform: Arc<dyn Platform>, bind: SocketAddr) -> i32 {
    if prepare(&*platform).is_err() {
        return 1;
    }
    let listener = match TcpListener::bind(bind) {
        Ok(listener) => listener,
        Err(e) => {
            crate::log!("cannot listen on {bind}: {e}");
            platform.notify("Orbit Link is already running.");
            return 1;
        }
    };
    crate::log!("Orbit Link {} started; page on {bind}", crate::VERSION);
    let app = App::new(platform, Some(bind));
    app.resume();

    for stream in listener.incoming() {
        let Ok(stream) = stream else { continue };
        if app.clients.fetch_add(1, Ordering::SeqCst) >= MAX_CLIENTS {
            app.clients.fetch_sub(1, Ordering::SeqCst);
            continue;
        }
        let app = app.clone();
        thread::spawn(move || {
            app.serve(stream);
            app.clients.fetch_sub(1, Ordering::SeqCst);
        });
    }
    0
}

/// Orbit Link inside another program, such as the Orbit Play helper on the
/// PS5: the engine and its actions, without the setup page. The host program
/// asks for actions itself, so it decides who may set up the network.
pub struct Embedded {
    app: Arc<App>,
}

impl Embedded {
    /// Starts the engine if this device was set up before.
    pub fn open(platform: Arc<dyn Platform>) -> Result<Embedded, String> {
        crate::log::file_only();
        prepare(&*platform)?;
        crate::log!("Orbit Link {} started inside its host program", crate::VERSION);
        let app = App::new(platform, None);
        app.resume();
        Ok(Embedded { app })
    }

    /// Runs an action and answers like the JSON API: `{"ok": true, "message"}`
    /// (plus `code` and `expires_in` for `invite`) or `{"ok": false, "error"}`.
    /// `status` returns the status page's data. `stop` is not available: the
    /// host program owns the process.
    pub fn call(&self, action: &str, fields: &[(String, String)]) -> Value {
        match action {
            "status" => self.app.status_json(),
            "stop" => json!({ "ok": false, "error": "Unknown action." }),
            _ => api_reply(&self.app.act(action, fields)).1,
        }
    }
}

impl App {
    fn new(platform: Arc<dyn Platform>, bind: Option<SocketAddr>) -> Arc<App> {
        Arc::new_cyclic(|myself| App {
            platform,
            state: Mutex::new(None),
            engine: Mutex::new(None),
            clients: AtomicUsize::new(0),
            bind,
            myself: myself.clone(),
            notices: Mutex::new(Vec::new()),
            generation: AtomicU64::new(0),
        })
    }

    /// Reconnects with the saved setup, if there is one.
    fn resume(&self) {
        match state::load::<DeviceState>(&self.state_path()) {
            Ok(Some(saved)) => {
                let message = format!("Orbit Link {} started. Connecting to {}.", crate::VERSION, saved.network);
                // Embedded, the host program announces its own start.
                if self.bind.is_some() {
                    self.platform.notify(&message);
                } else {
                    crate::log!("{message}");
                }
                if let Err(e) = self.start(saved) {
                    self.platform.notify(&format!("Orbit Link could not start: {e}"));
                }
            }
            Ok(None) => self.notify_setup("Orbit Link is ready to set up"),
            Err(e) => {
                crate::log!("{e}");
                self.notify_setup("Orbit Link's saved setup is damaged. Set it up again");
            }
        }
    }

    fn state_path(&self) -> PathBuf {
        self.platform.data_dir().join(state::STATE_FILE)
    }

    fn page_address(bind: SocketAddr) -> String {
        let host = if bind.ip().is_unspecified() {
            crate::net::lan_address().map_or("this device's IP".to_string(), |ip| ip.to_string())
        } else {
            bind.ip().to_string()
        };
        format!("http://{host}:{}", bind.port())
    }

    /// Points to the setup page. Embedded, the host program shows setup itself.
    fn notify_setup(&self, prefix: &str) {
        let Some(bind) = self.bind else {
            crate::log!("{prefix}");
            return;
        };
        let where_ = if self.platform.kind() == KIND_PS5 { " on your phone or computer" } else { "" };
        self.platform.notify(&format!("{prefix}: open {}{where_}.", Self::page_address(bind)));
    }

    fn start(&self, saved: DeviceState) -> Result<(), String> {
        self.stop_engine();
        crate::log!("starting the engine");
        let engine = Engine::new(&saved, self.platform.data_dir(), self.platform.serve())?;
        let shared = Arc::new(Shared::default());
        let thread_shared = shared.clone();
        let platform = self.platform.clone();
        let myself = self.myself.clone();
        let network = saved.network.clone();
        let kind = proto::kind_label(&saved.kind);
        let address = saved.address;
        let generation = self.generation.fetch_add(1, Ordering::SeqCst) + 1;
        let thread = thread::Builder::new()
            .name("orbit-link-engine".into())
            .spawn(move || {
                engine.run(&thread_shared, &|event| match event {
                    Event::Connected { first: true } => {
                        platform.notify(&format!("Orbit Link connected to {network}. This {kind} is {address}."))
                    }
                    Event::Connected { first: false } => platform.notify("Orbit Link reconnected to its server."),
                    Event::Unreachable => {
                        platform.notify("Orbit Link cannot reach its server. Direct connections keep working.")
                    }
                    Event::Removed => {
                        platform.notify(&format!("This {kind} was removed from {network}."));
                        if let Some(app) = myself.upgrade() {
                            // Not on this thread: forgetting joins the engine.
                            thread::spawn(move || {
                                if app.generation.load(Ordering::SeqCst) == generation {
                                    app.forget();
                                }
                            });
                        }
                    }
                    Event::Direct { .. } => {}
                });
            })
            .map_err(|e| e.to_string())?;
        *self.engine.lock().unwrap() = Some(EngineHandle { shared, thread });
        *self.state.lock().unwrap() = Some(saved);
        Ok(())
    }

    fn stop_engine(&self) {
        let handle = self.engine.lock().unwrap().take();
        if let Some(handle) = handle {
            handle.shared.stop.store(true, Ordering::SeqCst);
            if handle.thread.thread().id() != thread::current().id() {
                let _ = handle.thread.join();
            }
        }
    }

    fn shared(&self) -> Option<Arc<Shared>> {
        self.engine.lock().unwrap().as_ref().map(|h| h.shared.clone())
    }

    fn status(&self) -> Option<Status> {
        self.shared().map(|s| s.status())
    }

    fn save(&self, saved: &DeviceState) -> Result<(), String> {
        state::save(&self.state_path(), saved)
    }

    /// Changes the saved state and returns the new copy.
    fn update(&self, change: impl FnOnce(&mut DeviceState) -> Result<(), String>) -> Result<DeviceState, String> {
        let mut guard = self.state.lock().unwrap();
        let saved = guard.as_mut().ok_or("Orbit Link is not set up yet.")?;
        let mut next = saved.clone();
        change(&mut next)?;
        self.save(&next)?;
        *saved = next.clone();
        Ok(next)
    }

    fn forget(&self) {
        self.generation.fetch_add(1, Ordering::SeqCst);
        self.stop_engine();
        let dir = self.platform.data_dir();
        let _ = fs::remove_file(dir.join(state::STATE_FILE));
        let _ = fs::remove_file(dir.join(state::PEERS_FILE));
        *self.state.lock().unwrap() = None;
        crate::log!("setup removed");
    }

    fn send(&self, command: Command) -> Result<(), String> {
        self.shared().ok_or("Orbit Link is not running.")?.send(command);
        Ok(())
    }

    fn ask<T>(&self, make: impl FnOnce(mpsc::Sender<Result<T, String>>) -> Command) -> Result<T, String> {
        let (tx, rx) = mpsc::channel();
        self.send(make(tx))?;
        rx.recv_timeout(REPLY_WAIT).map_err(|_| "The server did not answer. Try again.".to_string())?
    }

    fn act(&self, action: &str, fields: &[(String, String)]) -> Result<Outcome, String> {
        let get = |name: &str| field(fields, name);
        match action {
            "setup" => self.setup(fields),
            "invite" => {
                let invite = self.ask(Command::Invite)?;
                crate::log!("made a join code, valid {} minutes", invite.expires_in.div_ceil(60));
                Ok(Outcome { message: "Enter this code on the new device.".into(), invite: Some(invite), stop: false })
            }
            "remove-device" => {
                let key = get("key");
                let own = self.state.lock().unwrap().as_ref().map(|s| crate::keys::public_of(&s.private_key));
                if own.is_some_and(|k| k.as_deref() == Ok(key.as_str())) {
                    return Err("To remove this device, use Leave network.".into());
                }
                self.ask(|tx| Command::Remove(key, tx))?;
                Ok(Outcome::says("Device removed from the network."))
            }
            "rename-device" => {
                let (key, name) = (get("key"), get("name"));
                if name.is_empty() {
                    return Err("Enter a name.".into());
                }
                self.ask(|tx| Command::Rename(key, name, tx))?;
                Ok(Outcome::says("Name changed."))
            }
            "add-forward" => self.add_forward(fields),
            "remove-forward" => self.remove_forward(fields),
            "loader" => {
                let allow = matches!(get("allow").as_str(), "on" | "true" | "1" | "yes");
                self.update(|s| {
                    s.allow_loader = allow;
                    Ok(())
                })?;
                self.send(Command::SetAllowLoader(allow))?;
                self.ask(Command::Sync)?;
                Ok(Outcome::says(if allow {
                    "The payload loader (port 9021) is now reachable from your other devices."
                } else {
                    "The payload loader (port 9021) is blocked for your other devices."
                }))
            }
            "restart" => {
                let saved = self.state.lock().unwrap().clone().ok_or("Orbit Link is not set up yet.")?;
                self.start(saved)?;
                Ok(Outcome::says("Restarted."))
            }
            "leave" => {
                let key = self.state.lock().unwrap().as_ref().and_then(|s| crate::keys::public_of(&s.private_key).ok());
                if let Some(key) = key {
                    // Best effort: the device is forgotten here either way.
                    let _ = self.ask(|tx| Command::Remove(key, tx));
                }
                self.forget();
                Ok(Outcome::says("This device left its network."))
            }
            "stop" => Ok(Outcome { message: "Orbit Link has stopped.".into(), stop: true, ..Outcome::default() }),
            _ => Err("Unknown action.".into()),
        }
    }

    fn setup(&self, fields: &[(String, String)]) -> Result<Outcome, String> {
        if self.state.lock().unwrap().is_some() {
            return Err("This device is already in a network. Leave it first.".into());
        }
        let get = |name: &str| field(fields, name);
        let server = match (get("server_choice").as_str(), crate::public_server()) {
            ("public", Some(public)) => public.to_string(),
            _ => get("server"),
        };
        if server.is_empty() {
            return Err("Enter your server's address.".into());
        }
        let (mode, code, network) = (get("mode"), get("code"), get("create"));
        let with = if mode == "create" || (mode.is_empty() && code.is_empty() && !network.is_empty()) {
            JoinWith::Create(proto::clean_name(&network, "Home"))
        } else if !code.is_empty() {
            JoinWith::Code(code)
        } else {
            return Err("Enter the code from one of your devices, or create a new network.".into());
        };
        let name = proto::clean_name(&get("name"), &self.platform.default_name());
        let joined = join::join(&server, with, &name, self.platform.kind())?;
        self.save(&joined)?;
        let message = format!("Joined {} as {} ({}).", joined.network, joined.name, joined.address);
        crate::log!("{message}");
        self.start(joined)?;
        Ok(Outcome::says(message))
    }

    fn add_forward(&self, fields: &[(String, String)]) -> Result<Outcome, String> {
        let get = |name: &str| field(fields, name);
        let peer = get("peer");
        if !self.status().is_some_and(|s| s.peers.iter().any(|p| p.key == peer)) {
            return Err("Choose one of the devices in your network.".into());
        }
        let bind: IpAddr = match get("bind").as_str() {
            "" => Ipv4Addr::LOCALHOST.into(),
            text => text.parse().map_err(|_| "The listen address must be an IP address.".to_string())?,
        };
        let wanted: Vec<Forward> = if get("preset") == "remote-play" {
            REMOTE_PLAY
                .iter()
                .map(|&(protocol, port)| Forward { peer: peer.clone(), protocol, port, listen: SocketAddr::new(bind, port) })
                .collect()
        } else {
            let protocol = Protocol::parse(&get("protocol")).ok_or("Choose TCP or UDP.")?;
            let port = parse_port(&get("port")).ok_or("Enter the port on the device (1–65535).")?;
            let local = match get("local_port").as_str() {
                "" => port,
                text => parse_port(text).ok_or("Enter a local port (1–65535).")?,
            };
            vec![Forward { peer: peer.clone(), protocol, port, listen: SocketAddr::new(bind, local) }]
        };
        let mut added = 0;
        let saved = self.update(|s| {
            for forward in &wanted {
                let clash = s.forwards.iter().any(|f| f.protocol == forward.protocol && f.listen == forward.listen);
                if !clash {
                    s.forwards.push(forward.clone());
                    added += 1;
                }
            }
            Ok(())
        })?;
        self.send(Command::SetForwards(saved.forwards))?;
        self.ask(Command::Sync)?;
        if added == 0 {
            return Err("Those local ports are already forwarded.".into());
        }
        Ok(Outcome::says(format!("Added {added} port forward{}.", if added == 1 { "" } else { "s" })))
    }

    fn remove_forward(&self, fields: &[(String, String)]) -> Result<Outcome, String> {
        let get = |name: &str| field(fields, name);
        let index: Option<usize> = get("index").parse().ok();
        let peer = get("peer");
        let protocol = Protocol::parse(&get("protocol"));
        let port = parse_port(&get("port"));
        let listen: Option<SocketAddr> = get("listen").parse().ok();
        let saved = self.update(|s| {
            let before = s.forwards.len();
            match index {
                Some(i) if i < s.forwards.len() => {
                    s.forwards.remove(i);
                }
                Some(_) => {}
                None if get("all") == "true" => s.forwards.retain(|f| f.peer != peer),
                None => s.forwards.retain(|f| {
                    !(f.peer == peer && Some(f.protocol) == protocol && Some(f.port) == port && listen.is_none_or(|l| l == f.listen))
                }),
            }
            if s.forwards.len() == before {
                return Err("No such port forward.".into());
            }
            Ok(())
        })?;
        self.send(Command::SetForwards(saved.forwards))?;
        self.ask(Command::Sync)?;
        Ok(Outcome::says("Port forward removed."))
    }

    fn serve(&self, stream: TcpStream) {
        stream.set_read_timeout(Some(Duration::from_secs(10))).ok();
        stream.set_write_timeout(Some(Duration::from_secs(10))).ok();
        let request = match http::read_request(&stream, 64 * 1024) {
            Ok(request) => request,
            Err(_) => return,
        };
        if !self.known_host(&request) {
            return respond(&stream, 403, "text/plain", "Open this page at its own address.");
        }
        let (path, query) = request.path.split_once('?').unwrap_or((request.path.as_str(), ""));
        let path = path.to_string();
        let api = path.starts_with("/api/");
        match (request.method.as_str(), path.as_str()) {
            ("GET", "/") => {
                let id: Option<u64> = http::parse_form(query).iter().find(|(k, _)| k == "n").and_then(|(_, v)| v.parse().ok());
                let notice = id.and_then(|id| self.notices.lock().unwrap().iter().find(|(n, _)| *n == id).map(|(_, notice)| notice.clone()));
                return respond(&stream, 200, "text/html; charset=utf-8", &self.page(notice));
            }
            ("GET", "/api/status") => return respond(&stream, 200, "application/json", &self.status_json().to_string()),
            ("POST", _) => {}
            _ => return respond(&stream, 404, "text/plain", "Not found"),
        }
        if !same_origin(&request) || (api && !is_json(&request)) {
            return respond(&stream, 403, "text/plain", "Cross-site request refused.");
        }
        let action = path.trim_start_matches("/api/").trim_start_matches('/');
        let fields = if api { json_fields(&request.body) } else { http::parse_form(&request.body) };
        let result = self.act(action, &fields);
        let stop = result.as_ref().is_ok_and(|o| o.stop);
        if api {
            let (status, body) = api_reply(&result);
            respond(&stream, status, "application/json", &body.to_string());
        } else if stop {
            let body = ui::page(
                "Orbit Link stopped",
                "",
                None,
                "<div class=\"card\">Orbit Link has stopped. Start it again to reconnect.</div>",
            );
            respond(&stream, 200, "text/html; charset=utf-8", &body);
        } else {
            let notice = match &result {
                Ok(outcome) => Notice::Done(outcome.message.clone(), outcome.invite.as_ref().map(|i| (i.code.clone(), i.expires_in))),
                Err(e) => Notice::Failed(e.clone()),
            };
            let id = u64::from_le_bytes(crate::keys::random_bytes());
            {
                let mut notices = self.notices.lock().unwrap();
                if notices.len() >= 32 {
                    notices.remove(0);
                }
                notices.push((id, notice));
            }
            redirect(&stream, &format!("/?n={id}"));
        }
        if stop {
            crate::log!("stopped from the page");
            self.stop_engine();
            std::process::exit(0);
        }
    }

    /// A page on loopback answers only to its own address, so a web site cannot
    /// reach it by pointing one of its names at this machine (DNS rebinding).
    fn known_host(&self, request: &Request) -> bool {
        let Some(bind) = self.bind else { return false };
        if !bind.ip().is_loopback() {
            return true;
        }
        let host = request.header("Host").unwrap_or("");
        let port = bind.port();
        [format!("127.0.0.1:{port}"), format!("localhost:{port}"), format!("[::1]:{port}")]
            .iter()
            .any(|known| known.eq_ignore_ascii_case(host))
    }

    fn status_json(&self) -> Value {
        let saved = self.state.lock().unwrap().clone();
        let mut body = json!({
            "version": crate::VERSION,
            "kind": self.platform.kind(),
            "configured": saved.is_some(),
            "public_server": crate::public_server(),
        });
        let (Some(saved), Some(status)) = (saved, self.status()) else { return body };
        body["device"] = json!({
            "name": saved.name, "address": saved.address.to_string(), "network": saved.network,
            "server": saved.server, "key": crate::keys::public_of(&saved.private_key).unwrap_or_default(),
            "allow_loader": saved.allow_loader,
        });
        body["server"] = json!({
            "state": status.server.as_str(),
            "address": status.server_address.map(|a| a.to_string()),
            "public_endpoint": status.public_endpoint.map(|a| a.to_string()),
            "relay": status.relay,
        });
        body["local_port"] = json!(status.local_port);
        body["port_mapping"] = json!(status.port_mapping);
        body["candidates"] = json!(status.candidates.iter().map(|a| a.to_string()).collect::<Vec<_>>());
        body["peers"] = Value::Array(
            status
                .peers
                .iter()
                .map(|p| {
                    json!({
                        "key": p.key, "name": p.name, "kind": p.kind, "address": p.address.to_string(),
                        "online": p.online, "path": p.path, "endpoint": p.endpoint.map(|a| a.to_string()),
                        "rtt_ms": p.rtt_ms, "rx_bytes": p.rx_bytes, "tx_bytes": p.tx_bytes,
                        "last_handshake_seconds": p.last_handshake.map(|d| d.as_secs()),
                    })
                })
                .collect(),
        );
        body["forwards"] = Value::Array(
            status
                .forwards
                .iter()
                .map(|f| {
                    json!({
                        "peer": f.forward.peer, "protocol": f.forward.protocol.as_str(), "port": f.forward.port,
                        "listen": f.forward.listen.to_string(), "error": f.error,
                    })
                })
                .collect(),
        );
        body["tcp_connections"] = json!(status.tcp_connections);
        body["udp_sessions"] = json!(status.udp_sessions);
        body["rx_bytes"] = json!(status.rx_bytes);
        body["tx_bytes"] = json!(status.tx_bytes);
        body["error"] = json!(status.last_error);
        body
    }

    fn page(&self, notice: Option<Notice>) -> String {
        let kind = proto::kind_label(self.platform.kind());
        let mut body = String::new();
        let mut invite_card = String::new();
        match notice {
            Some(Notice::Done(message, invite)) => {
                if let Some((code, expires_in)) = invite {
                    invite_card = format!(
                        "<div class=\"card\"><h2>Add a device</h2>\
                         <p>On the new device, open Orbit Link, choose the same server and enter:</p>\
                         <p class=\"code\">{}</p><p class=\"muted small\">Works once, for {} minutes.</p></div>",
                        escape(&proto::format_code(&code)),
                        expires_in.div_ceil(60)
                    );
                } else {
                    body.push_str(&format!("<div class=\"notice ok\">{}</div>", escape(&message)));
                }
            }
            Some(Notice::Failed(message)) => body.push_str(&format!("<div class=\"notice bad\">{}</div>", escape(&message))),
            None => {}
        }
        let saved = self.state.lock().unwrap().clone();
        let Some(saved) = saved else {
            body.push_str(&self.setup_card(kind));
            return ui::page("Orbit Link", &format!("Set up this {kind} · version {}", crate::VERSION), None, &body);
        };
        let status = self.status().unwrap_or_default();
        body.push_str(&invite_card);
        body.push_str(&status_card(&saved, &status));
        body.push_str(&devices_card(&saved, &status));
        if saved.kind == KIND_PS5 {
            body.push_str(&loader_card(&saved));
        } else {
            body.push_str(&forwards_card(&saved, &status));
        }
        body.push_str(
            "<div class=\"card\"><h2>Manage</h2><div class=\"row\">\
             <form class=\"inline\" method=\"post\" action=\"/restart\"><button class=\"btn secondary\">Restart</button></form>\
             <form class=\"inline\" method=\"post\" action=\"/stop\"><button class=\"btn secondary\">Stop Orbit Link</button></form>\
             <form class=\"inline\" method=\"post\" action=\"/leave\" onsubmit=\"return confirm('Leave this network? You need a new code to join again.')\">\
             <button class=\"btn danger\">Leave network</button></form></div></div>",
        );
        let refresh = if invite_card.is_empty() { Some((10, "/")) } else { None };
        ui::page("Orbit Link", &format!("{} · {kind} · version {}", escape(&saved.name), crate::VERSION), refresh, &body)
    }

    fn setup_card(&self, kind: &str) -> String {
        let server_choice = match crate::public_server() {
            Some(public) => format!(
                "<label class=\"check\"><input type=\"radio\" name=\"server_choice\" value=\"public\" checked> Orbit Link public server \
                 <span class=\"muted small\">&nbsp;({})</span></label>\
                 <label class=\"check\"><input type=\"radio\" name=\"server_choice\" value=\"own\"> My own server</label>\
                 <input name=\"server\" placeholder=\"https://link.example.com\" autocomplete=\"url\">",
                escape(public)
            ),
            None => "<input type=\"hidden\" name=\"server_choice\" value=\"own\">\
                     <input name=\"server\" placeholder=\"https://link.example.com\" autocomplete=\"url\" required>"
                .to_string(),
        };
        format!(
            "<div class=\"card\"><h2>Join an Orbit Link network</h2>\
             <p class=\"muted small\">Your devices find each other through an Orbit Link server and then connect directly.</p>\
             <form method=\"post\" action=\"/setup\">\
             <label>Server</label>{server_choice}\
             <label>Network</label>\
             <label class=\"check\"><input type=\"radio\" name=\"mode\" value=\"join\" checked> Join with a code from one of my devices</label>\
             <input name=\"code\" placeholder=\"ABCD-EFGH\" autocapitalize=\"characters\" autocomplete=\"off\">\
             <label class=\"check\"><input type=\"radio\" name=\"mode\" value=\"create\"> Create a new network</label>\
             <input name=\"create\" placeholder=\"Network name, such as Home\">\
             <label for=\"name\">Name of this {kind}</label>\
             <input id=\"name\" name=\"name\" value=\"{name}\">\
             <p><button class=\"btn\" type=\"submit\">Continue</button></p></form></div>\
             <div class=\"card small muted\">To get a code, open Orbit Link on a device that is already in your network and \
             choose <b>Add a device</b>. On your own server, the dashboard can create codes too.</div>",
            name = escape(&self.platform.default_name()),
        )
    }
}

#[derive(Clone)]
enum Notice {
    Done(String, Option<(String, u64)>),
    Failed(String),
}

fn status_card(saved: &DeviceState, status: &Status) -> String {
    let (dot, label) = match status.server {
        ServerState::Connected => ("ok", format!("Connected to {}", saved.network)),
        ServerState::Connecting => ("warn", "Connecting to the server".to_string()),
        ServerState::Unreachable => ("warn", "Server unreachable · direct connections keep working".to_string()),
        ServerState::Removed => ("bad", "Removed from the network".to_string()),
    };
    let public = status
        .public_endpoint
        .map_or("unknown yet".to_string(), |e| format!("{e} (port {} here)", status.local_port));
    let relay = if status.relay {
        "Available when a direct connection is not possible"
    } else {
        "Not offered by this server: devices connect directly only"
    };
    let error = status
        .last_error
        .as_ref()
        .map_or(String::new(), |e| format!("<tr><th>Last problem</th><td>{}</td></tr>", escape(e)));
    format!(
        "<div class=\"card\"><div class=\"row\"><span class=\"dot {dot}\"></span><h2 style=\"margin:0\">{label}</h2></div>\
         <table style=\"margin-top:12px\">\
         <tr><th>This device</th><td>{name} · <b>{address}</b></td></tr>\
         <tr><th>Server</th><td>{server}</td></tr>\
         <tr><th>Public address</th><td>{public}</td></tr>\
         <tr><th>Router port forward</th><td>{mapping}</td></tr>\
         <tr><th>Relay</th><td>{relay}</td></tr>\
         <tr><th>Traffic</th><td>{rx} received · {tx} sent</td></tr>{error}</table></div>",
        label = escape(&label),
        mapping = escape(status.port_mapping.as_deref().unwrap_or("None (UPnP and NAT-PMP not available)")),
        name = escape(&saved.name),
        address = saved.address,
        server = escape(&saved.server),
        rx = ui::bytes(status.rx_bytes),
        tx = ui::bytes(status.tx_bytes),
    )
}

fn path_text(peer: &crate::node::PeerStatus, relay: bool) -> (&'static str, String) {
    match peer.path {
        "direct" => ("ok", format!("Direct · {} ms", peer.rtt_ms.unwrap_or(0))),
        "relay" => ("warn", "Through the server's relay".to_string()),
        _ if !peer.online => ("", "Offline".to_string()),
        _ if relay => ("warn", "Connecting".to_string()),
        _ => ("warn", "Looking for a direct connection".to_string()),
    }
}

fn devices_card(saved: &DeviceState, status: &Status) -> String {
    let mut rows = String::new();
    for peer in &status.peers {
        let (dot, text) = path_text(peer, status.relay);
        rows.push_str(&format!(
            "<tr><td><b>{name}</b><div class=\"muted small\">{kind} · {address}</div></td>\
             <td><span class=\"dot {dot}\"></span> {text}</td>\
             <td class=\"actions\"><form class=\"inline\" method=\"post\" action=\"/remove-device\" \
             onsubmit=\"return confirm('Remove {js_name} from the network?')\"><input type=\"hidden\" name=\"key\" value=\"{key}\">\
             <button class=\"btn danger\">Remove</button></form></td></tr>",
            name = escape(&peer.name),
            js_name = escape(&peer.name.replace('\'', "")),
            kind = proto::kind_label(&peer.kind),
            address = peer.address,
            key = escape(&peer.key),
        ));
    }
    let table = if rows.is_empty() {
        "<p class=\"muted\">No other devices yet.</p>".to_string()
    } else {
        format!("<table class=\"devices\">{rows}</table>")
    };
    let hint = if !status.relay && status.peers.iter().any(|p| p.online && p.path == "none") {
        "<p class=\"muted small\">If a device keeps looking, the two networks block direct connections (common on some \
         mobile and hotel networks). Try another network, or use your own Orbit Link server, which can relay.</p>"
    } else {
        ""
    };
    format!(
        "<div class=\"card\"><h2>Devices in {network}</h2>{table}{hint}\
         <form method=\"post\" action=\"/invite\" style=\"margin-top:14px\"><button class=\"btn\">Add a device</button></form></div>",
        network = escape(&saved.network),
    )
}

fn loader_card(saved: &DeviceState) -> String {
    let (state, button, value) = if saved.allow_loader {
        ("Your other devices can send payloads to port 9021 of this PS5.", "Block the payload loader", "false")
    } else {
        ("The payload loader (port 9021) is blocked for your other devices. Every other port is reachable.", "Allow the payload loader", "true")
    };
    format!(
        "<div class=\"card\"><h2>Payload loader</h2><p class=\"muted small\">{state}</p>\
         <form method=\"post\" action=\"/loader\"><input type=\"hidden\" name=\"allow\" value=\"{value}\">\
         <button class=\"btn secondary\">{button}</button></form></div>"
    )
}

fn forwards_card(saved: &DeviceState, status: &Status) -> String {
    let name_of = |key: &str| status.peers.iter().find(|p| p.key == key).map_or("Removed device".to_string(), |p| p.name.clone());
    let mut rows = String::new();
    for (index, forward) in saved.forwards.iter().enumerate() {
        let error = status
            .forwards
            .iter()
            .find(|f| &f.forward == forward)
            .and_then(|f| f.error.clone())
            .map_or(String::new(), |e| format!("<div class=\"small\" style=\"color:var(--bad)\">{}</div>", escape(&e)));
        rows.push_str(&format!(
            "<tr><td><code>{listen}</code> {protocol}{error}</td><td>{peer} port {port}</td>\
             <td class=\"actions\"><form class=\"inline\" method=\"post\" action=\"/remove-forward\">\
             <input type=\"hidden\" name=\"index\" value=\"{index}\"><button class=\"btn secondary\">Remove</button></form></td></tr>",
            listen = forward.listen,
            protocol = forward.protocol.as_str().to_uppercase(),
            peer = escape(&name_of(&forward.peer)),
            port = forward.port,
        ));
    }
    let options: String = status
        .peers
        .iter()
        .map(|p| format!("<option value=\"{}\">{}</option>", escape(&p.key), escape(&p.name)))
        .collect();
    let presets: String = status
        .peers
        .iter()
        .filter(|p| p.kind == KIND_PS5)
        .map(|p| {
            let forwarded = REMOTE_PLAY.iter().all(|&(protocol, port)| {
                saved.forwards.iter().any(|f| f.peer == p.key && f.protocol == protocol && f.port == port)
            });
            if forwarded {
                return format!("<span class=\"muted small\">Remote Play ports of {} are forwarded.</span>", escape(&p.name));
            }
            format!(
                "<form class=\"inline\" method=\"post\" action=\"/add-forward\"><input type=\"hidden\" name=\"peer\" value=\"{}\">\
                 <input type=\"hidden\" name=\"preset\" value=\"remote-play\"><button class=\"btn\">Remote Play to {}</button></form>",
                escape(&p.key),
                escape(&p.name)
            )
        })
        .collect();
    let list = if rows.is_empty() {
        "<p class=\"muted\">No port forwards yet.</p>".to_string()
    } else {
        format!("<table class=\"devices\">{rows}</table>")
    };
    let add = if options.is_empty() {
        String::new()
    } else {
        format!(
            "<details style=\"margin-top:14px\"><summary>Forward another port</summary>\
             <form method=\"post\" action=\"/add-forward\"><label>Device</label><select name=\"peer\">{options}</select>\
             <div class=\"two\"><div><label>Protocol</label><select name=\"protocol\"><option>TCP</option><option>UDP</option></select></div>\
             <div><label>Port on the device</label><input name=\"port\" inputmode=\"numeric\" placeholder=\"34177\"></div></div>\
             <label>Port on this computer (optional)</label><input name=\"local_port\" inputmode=\"numeric\" placeholder=\"same\">\
             <p><button class=\"btn secondary\">Add</button></p></form></details>"
        )
    };
    format!(
        "<div class=\"card\"><h2>Use your devices from this computer</h2>\
         <p class=\"muted small\">A forwarded port works on <code>127.0.0.1</code> here, such as \
         <code>127.0.0.1:8084</code> for Payload Manager. Orbit Play sets up Remote Play itself.</p><div class=\"row\">{presets}</div>{list}{add}</div>"
    )
}

/// The JSON API's answer to an action.
fn api_reply(result: &Result<Outcome, String>) -> (u16, Value) {
    match result {
        Ok(outcome) => {
            let mut body = json!({ "ok": true, "message": outcome.message });
            if let Some(invite) = &outcome.invite {
                body["code"] = json!(proto::format_code(&invite.code));
                body["expires_in"] = json!(invite.expires_in);
            }
            (200, body)
        }
        Err(e) => (400, json!({ "ok": false, "error": e })),
    }
}

fn parse_port(text: &str) -> Option<u16> {
    text.trim().parse::<u16>().ok().filter(|p| *p > 0)
}

fn field(fields: &[(String, String)], name: &str) -> String {
    fields.iter().find(|(k, _)| k == name).map(|(_, v)| v.trim().to_string()).unwrap_or_default()
}

/// A JSON object's members as form-style fields.
fn json_fields(body: &str) -> Vec<(String, String)> {
    let Ok(Value::Object(map)) = serde_json::from_str::<Value>(body) else { return Vec::new() };
    map.into_iter()
        .filter_map(|(k, v)| {
            let text = match v {
                Value::String(s) => s,
                Value::Number(n) => n.to_string(),
                Value::Bool(b) => b.to_string(),
                _ => return None,
            };
            Some((k, text))
        })
        .collect()
}

fn is_json(request: &Request) -> bool {
    request.header("Content-Type").is_some_and(|t| t.trim().to_ascii_lowercase().starts_with("application/json"))
}

/// Refuses posts made by other web sites.
fn same_origin(request: &Request) -> bool {
    let Some(origin) = request.header("Origin") else { return true };
    let host = request.header("Host").unwrap_or("");
    origin.split_once("://").is_some_and(|(_, rest)| rest == host)
}

fn redirect(mut stream: &TcpStream, location: &str) {
    let head = format!(
        "HTTP/1.1 303 See Other\r\nLocation: {location}\r\nContent-Length: 0\r\nCache-Control: no-store\r\nConnection: close\r\n\r\n"
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.flush();
}

fn respond(mut stream: &TcpStream, status: u16, content_type: &str, body: &str) {
    let reason = match status {
        200 => "OK",
        400 => "Bad Request",
        403 => "Forbidden",
        404 => "Not Found",
        _ => "Error",
    };
    let head = format!(
        "HTTP/1.1 {status} {reason}\r\nContent-Type: {content_type}\r\nContent-Length: {}\r\n\
         Cache-Control: no-store\r\nX-Frame-Options: DENY\r\nConnection: close\r\n\r\n",
        body.len()
    );
    let _ = stream.write_all(head.as_bytes());
    let _ = stream.write_all(body.as_bytes());
    let _ = stream.flush();
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_bodies_become_fields() {
        let fields = json_fields(r#"{"peer":"abc","port":9295,"all":true,"skip":null}"#);
        assert_eq!(field(&fields, "peer"), "abc");
        assert_eq!(field(&fields, "port"), "9295");
        assert_eq!(field(&fields, "all"), "true");
        assert_eq!(field(&fields, "skip"), "");
        assert!(json_fields("not json").is_empty());
        assert_eq!(parse_port("0"), None);
        assert_eq!(parse_port(" 9295 "), Some(9295));
    }
}
