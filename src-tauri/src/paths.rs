//! Skyward's shared per-user data root. We deliberately use the XDG-style
//! `~/.config/skyward/en-tu-cara` (honoring `$XDG_CONFIG_HOME`) instead of
//! `~/Library/Application Support/...` so the folder is easy to find and shared
//! across Skyward apps. Holds `logs/`, `exports/`, and future artifacts.
//!
//! NOTE: the calendar/alarm STATE + settings stay in the macOS app_data_dir
//! (see lib.rs setup) — moving them would drop users' existing data.
use std::path::PathBuf;

/// `$XDG_CONFIG_HOME/skyward/en-tu-cara`, falling back to
/// `~/.config/skyward/en-tu-cara`.
pub fn data_dir() -> PathBuf {
    let config_home = std::env::var_os("XDG_CONFIG_HOME")
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".config")))
        .unwrap_or_else(|| PathBuf::from(".config"));
    config_home.join("skyward").join("en-tu-cara")
}

pub fn logs_dir() -> PathBuf {
    data_dir().join("logs")
}

pub fn exports_dir() -> PathBuf {
    data_dir().join("exports")
}

/// Create the directory tree up front. Best-effort — if it fails, file logging
/// simply won't have a destination (stdout still works).
pub fn ensure() {
    let _ = std::fs::create_dir_all(logs_dir());
    let _ = std::fs::create_dir_all(exports_dir());
}
