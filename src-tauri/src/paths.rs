//! Skyward's shared per-user data root. We deliberately use the XDG-style
//! `~/.config/skyward/en-tu-cara` (honoring `$XDG_CONFIG_HOME`) instead of
//! `~/Library/Application Support/...` so the folder is easy to find and shared
//! across Skyward apps. Holds `logs/`, `exports/`, and future artifacts.
//!
//! State + settings (state.json, settings.json) live here too, alongside logs/
//! and exports/ (see lib.rs setup) — moved off the macOS app_data_dir since
//! there were no existing users to migrate.
use std::ffi::OsString;
use std::path::PathBuf;

/// `$XDG_CONFIG_HOME/skyward/en-tu-cara`, falling back to
/// `~/.config/skyward/en-tu-cara`.
pub fn data_dir() -> PathBuf {
    resolve_data_dir(std::env::var_os("XDG_CONFIG_HOME"), std::env::var_os("HOME"))
}

/// Pure resolver (testable): XDG_CONFIG_HOME wins when set and non-empty, else
/// `<HOME>/.config`, else a bare `.config` (last resort).
fn resolve_data_dir(xdg_config_home: Option<OsString>, home: Option<OsString>) -> PathBuf {
    let config_home = xdg_config_home
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
        .or_else(|| home.map(|h| PathBuf::from(h).join(".config")))
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prefers_xdg_config_home_when_set() {
        let dir = resolve_data_dir(Some("/x/cfg".into()), Some("/home/u".into()));
        assert_eq!(dir, PathBuf::from("/x/cfg/skyward/en-tu-cara"));
    }

    #[test]
    fn falls_back_to_home_dot_config() {
        let dir = resolve_data_dir(None, Some("/home/u".into()));
        assert_eq!(dir, PathBuf::from("/home/u/.config/skyward/en-tu-cara"));
    }

    #[test]
    fn empty_xdg_is_ignored() {
        let dir = resolve_data_dir(Some(OsString::new()), Some("/home/u".into()));
        assert_eq!(dir, PathBuf::from("/home/u/.config/skyward/en-tu-cara"));
    }

    #[test]
    fn last_resort_when_no_env() {
        let dir = resolve_data_dir(None, None);
        assert_eq!(dir, PathBuf::from(".config/skyward/en-tu-cara"));
    }

    #[test]
    fn logs_and_exports_nest_under_data_dir() {
        assert!(logs_dir().ends_with("skyward/en-tu-cara/logs"));
        assert!(exports_dir().ends_with("skyward/en-tu-cara/exports"));
        assert_eq!(logs_dir().parent(), Some(data_dir().as_path()));
    }
}
