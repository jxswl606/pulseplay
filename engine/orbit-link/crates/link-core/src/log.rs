//! Small file logger. The log is capped and rotated once to `<name>.1`.

use std::fs::{self, File, OpenOptions};
use std::io::Write;
use std::path::PathBuf;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{SystemTime, UNIX_EPOCH};

const MAX_LOG_BYTES: u64 = 512 * 1024;

struct Sink {
    path: PathBuf,
    file: Option<File>,
    written: u64,
}

static SINK: Mutex<Option<Sink>> = Mutex::new(None);
static STDERR: AtomicBool = AtomicBool::new(true);

/// Keeps log lines off standard error, for programs whose standard error may
/// be a connection nobody reads (a PS5 payload's loader).
pub fn file_only() {
    STDERR.store(false, Ordering::Relaxed);
}

/// Starts writing log lines to `path` in addition to standard error.
pub fn init(path: PathBuf) {
    let file = OpenOptions::new().create(true).append(true).open(&path).ok();
    let written = file.as_ref().and_then(|f| f.metadata().ok()).map_or(0, |m| m.len());
    *SINK.lock().unwrap_or_else(|e| e.into_inner()) = Some(Sink { path, file, written });
}

pub fn write(line: &str) {
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |d| d.as_secs());
    let text = format!("[{secs}] {line}\n");
    if STDERR.load(Ordering::Relaxed) {
        // Never panic on a closed standard error (eprint! would).
        let _ = std::io::stderr().write_all(text.as_bytes());
    }
    let mut guard = SINK.lock().unwrap_or_else(|e| e.into_inner());
    let Some(sink) = guard.as_mut() else { return };
    if sink.written + text.len() as u64 > MAX_LOG_BYTES {
        let mut old = sink.path.clone().into_os_string();
        old.push(".1");
        sink.file = None;
        let _ = fs::rename(&sink.path, old);
        sink.file = OpenOptions::new().create(true).append(true).open(&sink.path).ok();
        sink.written = 0;
    }
    if let Some(file) = sink.file.as_mut() {
        if file.write_all(text.as_bytes()).is_ok() {
            sink.written += text.len() as u64;
        }
    }
}

#[macro_export]
macro_rules! log {
    ($($arg:tt)*) => { $crate::log::write(&format!($($arg)*)) };
}
