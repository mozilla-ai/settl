//! File-based debug logger.
//!
//! Writes to `~/.settl/debug.log`. Automatically enabled in debug builds;
//! in release builds, set `SETTL_DEBUG=1` to enable. Uses the `log` crate
//! facade so any module can call `log::debug!()`, `log::info!()`, etc.

use std::fs::OpenOptions;
use std::io::Write;
use std::path::PathBuf;
use std::sync::Mutex;

use log::{LevelFilter, Log, Metadata, Record};

static LOGGER: FileLogger = FileLogger;

struct FileLogger;

impl Log for FileLogger {
    fn enabled(&self, _metadata: &Metadata) -> bool {
        true
    }

    fn log(&self, record: &Record) {
        if !self.enabled(record.metadata()) {
            return;
        }
        if let Ok(mut guard) = LOG_FILE.lock() {
            if let Some(file) = guard.as_mut() {
                let ts = std::time::SystemTime::now()
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_secs();
                let _ = writeln!(
                    file,
                    "[{ts}] [{level}] {target}: {msg}",
                    level = record.level(),
                    target = record.target(),
                    msg = record.args(),
                );
                let _ = file.flush();
            }
        }
    }

    fn flush(&self) {}
}

static LOG_FILE: Mutex<Option<std::fs::File>> = Mutex::new(None);

/// Initialize the logger. Call once at startup.
///
/// Enabled automatically in debug builds. In release builds, set
/// `SETTL_DEBUG=1` to enable. Can be explicitly disabled in debug builds
/// with `SETTL_DEBUG=0`.
pub fn init() {
    let env_override = std::env::var("SETTL_DEBUG").ok();
    let enabled = match env_override.as_deref() {
        Some("0" | "false") => false,
        Some("1" | "true") => true,
        _ => cfg!(debug_assertions),
    };

    if enabled {
        if let Some(path) = log_path() {
            // Truncate if the log file exceeds 5 MB to prevent unbounded growth.
            const MAX_LOG_SIZE: u64 = 5 * 1024 * 1024;
            if let Ok(meta) = std::fs::metadata(&path) {
                if meta.len() > MAX_LOG_SIZE {
                    let _ = std::fs::remove_file(&path);
                }
            }
            if let Ok(file) = OpenOptions::new().create(true).append(true).open(&path) {
                if let Ok(mut guard) = LOG_FILE.lock() {
                    *guard = Some(file);
                }
                let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Debug));
                log::info!("debug logging enabled, writing to {}", path.display());
                return;
            }
        }
    }

    // No-op: set max level to Off so log macros are completely elided.
    let _ = log::set_logger(&LOGGER).map(|()| log::set_max_level(LevelFilter::Off));
}

fn log_path() -> Option<PathBuf> {
    let home = std::env::var("HOME").ok().filter(|s| !s.is_empty())?;
    let dir = PathBuf::from(home).join(".settl");
    let _ = std::fs::create_dir_all(&dir);
    Some(dir.join("debug.log"))
}
