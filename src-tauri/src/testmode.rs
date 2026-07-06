//! Test-mode harness: mock clock + fire log.
//!
//! tauri-driver does not support macOS, so e2e verification happens through this
//! debug IPC instead. Gated on ENTUCARA_TEST_MODE=1 —
//! the commands exist but refuse to act outside test mode.
//!
//! The clock is the ONLY time source the app may use. Production code calls
//! `clock::now()`; tests steer it via `set_mock_now` / `advance_clock`.

use chrono::{DateTime, Duration, Utc};
use serde::Serialize;
use std::sync::Mutex;

pub fn is_test_mode() -> bool {
    std::env::var("ENTUCARA_TEST_MODE").is_ok_and(|v| v == "1")
}

static MOCK_NOW: Mutex<Option<DateTime<Utc>>> = Mutex::new(None);

pub mod clock {
    use super::*;

    /// The app-wide time source. Respects the mock when test mode is active.
    /// Consumed by the scheduler from Phase 1d/3 on; allow until then.
    #[allow(dead_code)]
    pub fn now() -> DateTime<Utc> {
        if is_test_mode() {
            if let Some(mocked) = *MOCK_NOW.lock().unwrap() {
                return mocked;
            }
        }
        Utc::now()
    }
}

/// Append-only log of alarm fires. The scheduler writes here on every fire;
/// CP1d/CP3 scripts read it back to assert exact timing/sequence.
#[derive(Debug, Clone, Serialize)]
pub struct FireRecord {
    /// Composite occurrence key: "(event_id, occurrence_start)".
    pub key: String,
    /// "reminder_<minutes>" (e.g. "reminder_5") | "t_zero" | "snooze"
    pub kind: String,
    /// Wall-clock when the fire decision executed (real clock, for latency math).
    pub fired_at_wall: DateTime<Utc>,
    /// App clock at fire time (mock clock in test mode).
    pub fired_at_app: DateTime<Utc>,
    /// When the alarm was scheduled to fire.
    pub scheduled_for: DateTime<Utc>,
}

static FIRE_LOG: Mutex<Vec<FireRecord>> = Mutex::new(Vec::new());

pub fn log_fire(key: &str, kind: &str, scheduled_for: DateTime<Utc>) {
    let record = FireRecord {
        key: key.to_string(),
        kind: kind.to_string(),
        fired_at_wall: Utc::now(),
        fired_at_app: clock::now(),
        scheduled_for,
    };
    // Test mode: mirror to disk so checkpoint scripts can assert the sequence
    // without webview IPC (Tauri commands aren't externally invokable).
    if is_test_mode() {
        if let Some(home) = std::env::var_os("HOME") {
            let dir = std::path::Path::new(&home)
                .join("Library/Application Support/dev.fforres.entucara");
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("fire-log.jsonl"))
            {
                use std::io::Write;
                let _ = writeln!(f, "{}", serde_json::to_string(&record).unwrap_or_default());
            }
        }
    }
    // Poison-tolerant (this runs on the scheduler thread on every fire) and
    // bounded: this app runs for weeks, so cap the in-memory log to the most
    // recent entries — it backs test assertions + recent diagnostics, never the
    // full history (test mode mirrors everything to disk above).
    let mut log = FIRE_LOG.lock().unwrap_or_else(|e| e.into_inner());
    log.push(record);
    const MAX_RECORDS: usize = 256;
    if log.len() > MAX_RECORDS {
        let excess = log.len() - MAX_RECORDS;
        log.drain(0..excess);
    }
}

#[tauri::command]
pub fn set_mock_now(iso: String) -> Result<(), String> {
    if !is_test_mode() {
        return Err("not in test mode".into());
    }
    let parsed = DateTime::parse_from_rfc3339(&iso)
        .map_err(|e| format!("bad timestamp {iso:?}: {e}"))?
        .with_timezone(&Utc);
    *MOCK_NOW.lock().unwrap() = Some(parsed);
    Ok(())
}

#[tauri::command]
pub fn advance_clock(seconds: i64) -> Result<String, String> {
    if !is_test_mode() {
        return Err("not in test mode".into());
    }
    let mut guard = MOCK_NOW.lock().unwrap();
    let base = guard.unwrap_or_else(Utc::now);
    let next = base + Duration::seconds(seconds);
    *guard = Some(next);
    Ok(next.to_rfc3339())
}

#[tauri::command]
pub fn get_fired_log() -> Vec<FireRecord> {
    FIRE_LOG.lock().unwrap_or_else(|e| e.into_inner()).clone()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fire_log_records_in_order() {
        let t = Utc::now();
        log_fire("(ev1, 2026-06-08T09:00:00Z)", "reminder_5", t);
        log_fire("(ev1, 2026-06-08T09:00:00Z)", "t_zero", t);
        let log = FIRE_LOG.lock().unwrap();
        assert!(log.len() >= 2);
        let kinds: Vec<_> = log.iter().map(|r| r.kind.as_str()).collect();
        let pos5 = kinds.iter().position(|k| *k == "reminder_5").unwrap();
        let pos0 = kinds.iter().position(|k| *k == "t_zero").unwrap();
        assert!(pos5 < pos0);
    }

    #[test]
    fn clock_returns_real_time_outside_test_mode() {
        // Unit tests don't set ENTUCARA_TEST_MODE; clock::now() must track Utc::now().
        let a = clock::now();
        let b = Utc::now();
        assert!((b - a).num_seconds().abs() < 2);
    }
}
