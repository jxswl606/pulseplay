//! Orbit Link for macOS, Linux and Windows: the computer side of an Orbit Link network.
//!
//! It runs the same engine as the PS5 payload. Its page (by default
//! http://127.0.0.1:34180) joins a network and forwards ports of other devices,
//! such as a PS5's Remote Play ports, to this computer. Other programs can use
//! the same actions through its JSON API.

use std::net::SocketAddr;
use std::path::PathBuf;
use std::sync::Arc;

use link_core::app::{self, Platform};
use link_core::proto::{KIND_COMPUTER, KIND_PS5};

struct Host {
    data: PathBuf,
    kind: &'static str,
    serve: bool,
    name: Option<String>,
}

impl Platform for Host {
    fn notify(&self, message: &str) {
        link_core::log!("notification: {message}");
    }
    fn data_dir(&self) -> PathBuf {
        self.data.clone()
    }
    fn kind(&self) -> &'static str {
        self.kind
    }
    fn default_name(&self) -> String {
        self.name.clone().or_else(host_name).unwrap_or_else(|| link_core::proto::kind_label(self.kind).to_string())
    }
    fn serve(&self) -> bool {
        self.serve
    }
}

#[cfg(windows)]
fn host_name() -> Option<String> {
    std::env::var("COMPUTERNAME").ok().filter(|name| !name.is_empty())
}

#[cfg(unix)]
fn host_name() -> Option<String> {
    let mut buffer = [0u8; 256];
    let result = unsafe { libc::gethostname(buffer.as_mut_ptr() as *mut libc::c_char, buffer.len()) };
    if result != 0 {
        return None;
    }
    let end = buffer.iter().position(|&b| b == 0).unwrap_or(buffer.len());
    let name = String::from_utf8_lossy(&buffer[..end]).trim_end_matches(".local").to_string();
    (!name.is_empty()).then_some(name)
}

/// `~/Library/Application Support/Orbit Link` on macOS, `%APPDATA%\Orbit Link` on
/// Windows, `~/.local/share/orbit-link` elsewhere.
fn default_data_dir() -> PathBuf {
    if cfg!(windows) {
        if let Some(appdata) = std::env::var_os("APPDATA").filter(|d| !d.is_empty()) {
            return PathBuf::from(appdata).join("Orbit Link");
        }
    }
    let home = std::env::var_os("HOME").map(PathBuf::from);
    if cfg!(target_os = "macos") {
        if let Some(home) = home {
            return home.join("Library/Application Support/Orbit Link");
        }
    } else if let Some(data) = std::env::var_os("XDG_DATA_HOME").filter(|d| !d.is_empty()) {
        return PathBuf::from(data).join("orbit-link");
    } else if let Some(home) = home {
        return home.join(".local/share/orbit-link");
    }
    PathBuf::from("orbit-link-data")
}

/// Starts the helper at every login (macOS), or stops doing that.
#[cfg(target_os = "macos")]
fn login_item(install: bool) -> ! {
    use std::process::Command;
    let home = std::env::var_os("HOME").map(PathBuf::from).unwrap_or_else(|| usage());
    let plist = home.join("Library/LaunchAgents/orbit-link.helper.plist");
    let uid = unsafe { libc::getuid() };
    let domain = format!("gui/{uid}");
    let _ = Command::new("launchctl").args(["bootout", &domain]).arg(&plist).output();
    if !install {
        let _ = std::fs::remove_file(&plist);
        println!("Orbit Link no longer starts at login.");
        std::process::exit(0);
    }
    let exe = std::env::current_exe().unwrap_or_else(|e| {
        eprintln!("cannot find this program: {e}");
        std::process::exit(1)
    });
    let text = format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n\
         <!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n\
         <plist version=\"1.0\"><dict>\n\
         <key>Label</key><string>orbit-link.helper</string>\n\
         <key>ProgramArguments</key><array><string>{}</string><string>--no-browser</string></array>\n\
         <key>RunAtLoad</key><true/>\n<key>KeepAlive</key><true/>\n</dict></plist>\n",
        link_core::ui::escape(&exe.to_string_lossy())
    );
    if let Some(parent) = plist.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Err(e) = std::fs::write(&plist, text) {
        eprintln!("cannot write {}: {e}", plist.display());
        std::process::exit(1);
    }
    let status = Command::new("launchctl").args(["bootstrap", &domain]).arg(&plist).status();
    if !status.is_ok_and(|s| s.success()) {
        eprintln!("launchctl could not start Orbit Link");
        std::process::exit(1);
    }
    println!("Orbit Link now starts at every login (from {}). Keep the program there.", exe.display());
    println!("Page: http://127.0.0.1:{}", link_core::SETUP_PORT);
    std::process::exit(0);
}

#[cfg(not(target_os = "macos"))]
fn login_item(_install: bool) -> ! {
    eprintln!("--install is for macOS. On Linux, run orbit-link-node as a systemd user service; on Windows, add it to Startup.");
    std::process::exit(2);
}

fn main() {
    let mut host = Host { data: default_data_dir(), kind: KIND_COMPUTER, serve: false, name: None };
    let mut bind: SocketAddr = ([127, 0, 0, 1], link_core::SETUP_PORT).into();
    let mut browser = cfg!(target_os = "macos");
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            "--data" => host.data = args.next().map(PathBuf::from).unwrap_or_else(|| usage()),
            "--listen" => bind = args.next().and_then(|a| a.parse().ok()).unwrap_or_else(|| usage()),
            "--name" => host.name = Some(args.next().unwrap_or_else(|| usage())),
            "--serve" => host.serve = true,
            // Behave like the PS5 payload; used by the tests.
            "--kind" => match args.next().as_deref() {
                Some(KIND_PS5) => {
                    host.kind = KIND_PS5;
                    host.serve = true;
                }
                Some(KIND_COMPUTER) => host.kind = KIND_COMPUTER,
                _ => usage(),
            },
            "--no-browser" => browser = false,
            // For a program that runs the node: it ends when that program does,
            // even if it never got to stop it.
            "--exit-with-stdin" => {
                std::thread::spawn(|| {
                    let _ = std::io::copy(&mut std::io::stdin(), &mut std::io::sink());
                    link_core::log!("standard input closed; stopping");
                    std::process::exit(0);
                });
            }
            "--install" => login_item(true),
            "--uninstall" => login_item(false),
            "--version" => {
                println!("orbit-link-node {}", link_core::VERSION);
                return;
            }
            "--help" | "-h" => usage(),
            _ => usage(),
        }
    }
    let host_part = if bind.ip().is_unspecified() { "127.0.0.1".to_string() } else { bind.ip().to_string() };
    let page = format!("http://{host_part}:{}", bind.port());
    println!("Orbit Link {}: open {page}", link_core::VERSION);
    let opener = browser.then(|| {
        std::thread::spawn(move || {
            std::thread::sleep(std::time::Duration::from_millis(500));
            let _ = std::process::Command::new("open").arg(&page).status();
        })
    });
    let code = app::run(Arc::new(host), bind);
    // Only reached when another copy is already running: still show its page.
    if let Some(opener) = opener {
        let _ = opener.join();
    }
    std::process::exit(code);
}

fn usage() -> ! {
    eprintln!(
        "usage: orbit-link-node [--data DIR] [--listen ADDR:PORT] [--name NAME] [--serve] [--no-browser] [--exit-with-stdin]\n\
         \x20      orbit-link-node --install | --uninstall   (macOS: start at every login)\n\n\
         Open http://127.0.0.1:{} to join a network and forward ports.\n\
         --serve        let the other devices reach this computer's ports too\n\
         --no-browser   do not open the page on start (macOS)\n\
         --exit-with-stdin  stop when standard input closes (for programs that run the node)",
        link_core::SETUP_PORT
    );
    std::process::exit(2);
}
