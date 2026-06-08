//! Persisted alarm state: fired-set, snoozes, pause flag.
//! Plain JSON in app_data_dir/state.json — written on every mutation (tiny file),
//! GC'd on write so it never grows unbounded.

use crate::alarm_core::AlarmState;
use std::path::PathBuf;
use std::sync::Mutex;

pub struct SharedState {
    pub alarms: Mutex<AlarmState>,
    path: PathBuf,
}

impl SharedState {
    pub fn load(dir: PathBuf) -> Self {
        let path = dir.join("state.json");
        let alarms = crate::paths::load_json_or_default(&path);
        Self { alarms: Mutex::new(alarms), path }
    }

    /// Mutate-and-persist. All writers go through here so disk never drifts.
    /// The write is atomic (temp+rename) so a crash can never truncate the
    /// fired-set/snoozes into a file that loads as empty on next launch.
    pub fn update<R>(&self, f: impl FnOnce(&mut AlarmState) -> R) -> R {
        // Recover from a poisoned lock rather than panicking: a prior panic must
        // not permanently brick alarm persistence.
        let mut guard = self.alarms.lock().unwrap_or_else(|e| e.into_inner());
        let result = f(&mut guard);
        guard.gc(chrono::Utc::now());
        if let Ok(json) = serde_json::to_vec_pretty(&*guard) {
            let _ = crate::paths::atomic_write(&self.path, &json);
        }
        result
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::alarm_core::AlarmKind;

    #[test]
    fn state_round_trips_through_disk() {
        let dir = std::env::temp_dir().join(format!("entucara-test-{}", std::process::id()));
        let s = SharedState::load(dir.clone());
        s.update(|a| a.mark_fired("(e @ t)", AlarmKind::TZero, chrono::Utc::now()));

        let reloaded = SharedState::load(dir.clone());
        assert!(reloaded.alarms.lock().unwrap().has_fired("(e @ t)", AlarmKind::TZero));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn corrupt_state_is_preserved_not_silently_wiped() {
        let dir = std::env::temp_dir().join(format!("entucara-state-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        // Simulate a crash mid-write: a truncated state.json on disk.
        std::fs::write(dir.join("state.json"), b"{\"fired\":[{\"key\":\"e @ t").unwrap();
        // Loading recovers to defaults (no panic) AND keeps the bad bytes around.
        let s = SharedState::load(dir.clone());
        assert!(!s.alarms.lock().unwrap().has_fired("(e @ t)", AlarmKind::TZero));
        let has_backup = std::fs::read_dir(&dir)
            .unwrap()
            .filter_map(|e| e.ok())
            .any(|e| e.file_name().to_string_lossy().contains(".corrupt-"));
        assert!(has_backup, "corrupt state.json must be preserved as a .corrupt-* backup");
        let _ = std::fs::remove_dir_all(dir);
    }
}
