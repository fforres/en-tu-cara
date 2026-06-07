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
        let alarms = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { alarms: Mutex::new(alarms), path }
    }

    /// Mutate-and-persist. All writers go through here so disk never drifts.
    pub fn update<R>(&self, f: impl FnOnce(&mut AlarmState) -> R) -> R {
        let mut guard = self.alarms.lock().unwrap();
        let result = f(&mut guard);
        guard.gc(chrono::Utc::now());
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&*guard) {
            let _ = std::fs::write(&self.path, json);
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
}
