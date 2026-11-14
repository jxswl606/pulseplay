//! Orbit Link device engine.
//!
//! A device joins a network on an Orbit Link server, learns the other devices
//! from it and connects to each of them directly, with hole punching through
//! home routers. Traffic between two devices is encrypted end to end with the
//! WireGuard protocol. No kernel tunnel device is needed: packets are handled
//! by an in-process network stack.

pub mod app;
pub mod disco;
pub mod http;
pub mod join;
pub mod keys;
pub mod log;
pub mod net;
pub mod node;
pub mod portmap;
pub mod state;
pub mod sys;
pub mod ui;

pub use link_proto as proto;

/// Port of the device's setup and status page.
pub const SETUP_PORT: u16 = 34180;

/// The payload loader. Blocked unless the owner allows it for this device.
pub const LOADER_PORT: u16 = 9021;

/// Product version, shared by every component built from this workspace.
pub const VERSION: &str = env!("CARGO_PKG_VERSION");

/// The public Orbit Link server offered as the default choice during setup.
/// Set at build time with `ORBIT_LINK_PUBLIC_SERVER=https://...`; without it,
/// setup only offers "your own server".
pub const PUBLIC_SERVER: Option<&str> = option_env!("ORBIT_LINK_PUBLIC_SERVER");

/// The public server, if this build has one.
pub fn public_server() -> Option<&'static str> {
    PUBLIC_SERVER.map(str::trim).filter(|s| !s.is_empty())
}
