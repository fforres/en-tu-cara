//! Skyward's shared per-user data root. We deliberately use the XDG-style
//! `~/.config/skyward/en-tu-cara` (honoring `$XDG_CONFIG_HOME`) instead of
//! `~/Library/Application Support/...` so the folder is easy to find and shared
//! across Skyward apps. Holds `logs/`, `exports/`, and future artifacts.
//!
//! State + settings (state.json, settings.json) live here too, alongside logs/
//! and exports/ (see lib.rs setup) — moved off the macOS app_data_dir since
//! there were no existing users to migrate.
use std::ffi::OsString;
use std::path::{Path, PathBuf};

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

/// Atomically replace `path`'s contents with `bytes`: write a sibling `.tmp`
/// file then rename it over the target (rename is atomic within a filesystem).
/// A crash mid-write can leave a stray `.tmp` but NEVER a truncated/half-written
/// `path` — which is what `std::fs::write` (truncate-then-write) risks, and what
/// would silently wipe the fired-set/snoozes on the next load. Callers hold their
/// state lock across this so concurrent writes stay ordered (no lost updates).
pub fn atomic_write(path: &Path, bytes: &[u8]) -> std::io::Result<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let mut tmp = path.as_os_str().to_owned();
    tmp.push(".tmp");
    let tmp = PathBuf::from(tmp);
    std::fs::write(&tmp, bytes)?;
    std::fs::rename(&tmp, path)
}

/// Load + deserialize JSON from `path`, tolerant of first-run and corruption.
///   - Missing file (first run) → `T::default()`, silently.
///   - Present but UNPARSEABLE (e.g. a crash truncated it) → log an error,
///     preserve the bad bytes as `<path>.corrupt-<unix_ts>` for diagnosis, then
///     fall back to `T::default()`. Without the rename the next write would
///     overwrite the corrupt file with defaults — silent, unrecoverable loss.
pub fn load_json_or_default<T>(path: &Path) -> T
where
    T: serde::de::DeserializeOwned + Default,
{
    let Ok(bytes) = std::fs::read(path) else {
        return T::default();
    };
    match serde_json::from_slice(&bytes) {
        Ok(value) => value,
        Err(e) => {
            let ts = chrono::Utc::now().timestamp();
            let mut backup = path.as_os_str().to_owned();
            backup.push(format!(".corrupt-{ts}"));
            let backup = PathBuf::from(backup);
            log::error!(
                "failed to parse {}: {e}; preserving as {} and using defaults",
                path.display(),
                backup.display(),
            );
            let _ = std::fs::rename(path, &backup);
            T::default()
        }
    }
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

    fn scratch(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("entucara-paths-{tag}-{}", std::process::id()))
    }

    #[test]
    fn atomic_write_creates_parent_and_leaves_no_tmp() {
        let dir = scratch("aw");
        let path = dir.join("nested").join("state.json");
        atomic_write(&path, b"{\"a\":1}").unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), b"{\"a\":1}");
        // The temp sibling must not linger after a successful write.
        let mut tmp = path.as_os_str().to_owned();
        tmp.push(".tmp");
        assert!(!PathBuf::from(tmp).exists(), "leftover .tmp after atomic_write");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn atomic_write_replaces_existing_fully() {
        let dir = scratch("aw-replace");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("s.json");
        atomic_write(&path, b"old-and-longer-content").unwrap();
        atomic_write(&path, b"new").unwrap();
        // No truncation/append artifacts: exactly the new bytes.
        assert_eq!(std::fs::read(&path).unwrap(), b"new");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_json_missing_file_is_default_no_backup() {
        let dir = scratch("load-missing");
        let path = dir.join("absent.json");
        let v: Vec<u32> = load_json_or_default(&path);
        assert_eq!(v, Vec::<u32>::new());
        assert!(!path.exists(), "missing file must not be created");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_json_valid_round_trips() {
        let dir = scratch("load-valid");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("v.json");
        std::fs::write(&path, b"[1,2,3]").unwrap();
        let v: Vec<u32> = load_json_or_default(&path);
        assert_eq!(v, vec![1, 2, 3]);
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn load_json_corrupt_is_preserved_then_default() {
        let dir = scratch("load-corrupt");
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("c.json");
        std::fs::write(&path, b"{ truncated mid-wri").unwrap();
        let v: Vec<u32> = load_json_or_default(&path);
        assert_eq!(v, Vec::<u32>::new(), "corrupt file falls back to default");
        // Original is moved aside (not silently overwritten) for diagnosis.
        assert!(!path.exists(), "corrupt file should be renamed away");
        let preserved: Vec<_> = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .filter(|e| e.file_name().to_string_lossy().contains(".corrupt-"))
            .collect();
        assert_eq!(preserved.len(), 1, "exactly one .corrupt-<ts> backup");
        assert_eq!(std::fs::read(preserved[0].path()).unwrap(), b"{ truncated mid-wri");
        let _ = std::fs::remove_dir_all(dir);
    }
}
