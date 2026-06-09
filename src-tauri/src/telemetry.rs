//! Anonymized usage telemetry → PostHog (project 16058, US cloud).
//!
//! Design constraint #1, above everything: this must NEVER block or slow the
//! alarm path. The one unforgivable failure is a missed alert; a telemetry POST
//! must not be able to cause it. So `record()` only does a non-blocking
//! `try_send` onto a bounded queue and returns instantly — if the queue is full
//! (network wedged, PostHog down) events are DROPPED, never awaited. All network
//! I/O happens on a dedicated worker thread.
//!
//! Privacy: only behavioral data + hashes leave the device. Never event titles,
//! attendees, calendar names, or raw emails. The `distinct_id` is a random
//! per-install UUID (settings.device_id), not a hardware id, and shared with the
//! webview so JS + Rust events unify on one device. Events are marked anonymous
//! (`$process_person_profiles: false`) so no PostHog person profiles are created.
//!
//! The PostHog project API key below is a *public, write-only ingest token*
//! designed to be embedded in client apps — it cannot read data back out.

use serde_json::{json, Map, Value};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, TrySendError};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::OnceLock;
use std::time::Duration;

/// Public, write-only PostHog project token (safe to embed; see module docs).
const POSTHOG_KEY: &str = "phc_L2ELKHRyIWk4Ql8tQ01dude2RalWeJF1lgBF79SBqMY";
/// US-cloud ingest host (project 16058 lives on US).
const POSTHOG_HOST: &str = "https://us.i.posthog.com";

/// Bounded queue depth. Sized so a transient network stall buffers a little, but
/// a sustained one drops rather than growing memory — we'd rather lose telemetry
/// than ever apply backpressure to a caller on the alarm path.
const QUEUE_CAP: usize = 256;
/// Flush when this many events have accumulated…
const BATCH_MAX: usize = 20;
/// …or when this long has passed since the last event, whichever comes first.
const FLUSH_AFTER: Duration = Duration::from_secs(15);
/// Hard cap on any single network call so a hung socket can't pin the worker.
const HTTP_TIMEOUT: Duration = Duration::from_secs(10);

struct Sink {
    tx: SyncSender<Value>,
}

static SINK: OnceLock<Option<Sink>> = OnceLock::new();
/// Count of events dropped because the queue was full — surfaced as a property on
/// the next event that DOES get through, so a wedged pipeline is visible.
static DROPPED: AtomicU64 = AtomicU64::new(0);

/// Hex sha256 — the one hashing primitive telemetry needs (account grouping,
/// occurrence-key anonymization). Kept here so callers don't pull in `sha2`.
pub fn sha256_hex(bytes: &[u8]) -> String {
    use sha2::{Digest, Sha256};
    let mut h = Sha256::new();
    h.update(bytes);
    h.finalize().iter().map(|b| format!("{b:02x}")).collect()
}

/// Decide whether telemetry runs. Pure so the precedence is unit-tested:
/// 1. `ENTUCARA_TELEMETRY=off` → always off (kill-switch).
/// 2. `ENTUCARA_TELEMETRY=on` → always on (explicit override, e.g. to verify
///    ingestion during a test-mode run).
/// 3. test mode with no explicit env → off, so automated checkpoint runs never
///    pollute the production project.
/// 4. otherwise → follow the user's Settings toggle.
pub fn resolve_enabled(settings_enabled: bool, env: Option<&str>, test_mode: bool) -> bool {
    match env.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("off") | Some("false") | Some("0") => false,
        Some("on") | Some("true") | Some("1") => true,
        _ if test_mode => false,
        _ => settings_enabled,
    }
}

/// Resolve the enabled flag from live settings + environment (the non-pure wrapper
/// the app and the `telemetry_config` command both call).
pub fn is_enabled(settings_enabled: bool) -> bool {
    resolve_enabled(
        settings_enabled,
        std::env::var("ENTUCARA_TELEMETRY").ok().as_deref(),
        crate::testmode::is_test_mode(),
    )
}

/// Start the telemetry worker. Call once, from app setup. When `enabled` is false
/// this is a no-op and every later `record()` cheaply does nothing.
///
/// `distinct_id` is the persisted device UUID; `account_hash` the coarse,
/// best-effort org key (may be None); `app_version` the running build.
pub fn init(enabled: bool, distinct_id: String, account_hash: Option<String>, app_version: String) {
    let sink = if enabled {
        let (tx, rx) = std::sync::mpsc::sync_channel::<Value>(QUEUE_CAP);
        let envelope = base_properties(&distinct_id, account_hash, &app_version);
        std::thread::Builder::new()
            .name("entucara-telemetry".into())
            .spawn(move || worker(rx, envelope))
            .ok();
        Some(Sink { tx })
    } else {
        None
    };
    // If init is somehow called twice, the first wins; the second's channel drops
    // and its (unstarted-from-our-view) state is discarded. Single call expected.
    let _ = SINK.set(sink);
}

/// Properties attached to every event: identity + environment + the anonymity
/// flag. Built once at init and merged into each event by the worker.
fn base_properties(distinct_id: &str, account_hash: Option<String>, app_version: &str) -> Map<String, Value> {
    let mut m = Map::new();
    m.insert("distinct_id".into(), json!(distinct_id));
    m.insert("$process_person_profiles".into(), json!(false)); // anonymous events
    m.insert("$lib".into(), json!("entucara-rust"));
    m.insert("$lib_version".into(), json!(app_version));
    m.insert("app_version".into(), json!(app_version));
    m.insert("os".into(), json!(os_version()));
    m.insert("session_id".into(), json!(uuid::Uuid::new_v4().to_string()));
    if let Some(h) = account_hash {
        m.insert("account_hash".into(), json!(h));
    }
    m
}

fn os_version() -> String {
    #[cfg(target_os = "macos")]
    {
        let v = std::process::Command::new("sw_vers")
            .arg("-productVersion")
            .output()
            .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
            .unwrap_or_default();
        if v.is_empty() {
            "macos".into()
        } else {
            format!("macOS {v}")
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        std::env::consts::OS.to_string()
    }
}

/// Record one event. Non-blocking and infallible from the caller's view: safe to
/// call from any thread, including the scheduler tick. `props` should be a JSON
/// object (use `serde_json::json!({...})`, or `json!({})` for none).
pub fn record(event: &str, props: Value) {
    let Some(Some(sink)) = SINK.get() else {
        return; // disabled or not initialized
    };
    let entry = json!({
        "event": event,
        "properties": props,
        "timestamp": chrono::Utc::now().to_rfc3339(),
    });
    match sink.tx.try_send(entry) {
        Ok(()) => {}
        Err(TrySendError::Full(_)) => {
            DROPPED.fetch_add(1, Ordering::Relaxed);
        }
        Err(TrySendError::Disconnected(_)) => {}
    }
}

/// Forward a Rust log line as a `rust_log` event. Phase-1 "logs from Rust": we
/// emit this at the operationally meaningful warn/error sites (overlay-show
/// failure, calendar-sync failure, panics) rather than bridging the whole `log`
/// facade — tauri-plugin-log already owns the global logger, and the full log
/// stream is the job of the phase-2 OTLP pipeline. Never sends user content.
pub fn log_event(level: &str, target: &str, message: impl Into<String>) {
    record(
        "rust_log",
        json!({ "level": level, "target": target, "message": message.into() }),
    );
}

/// The worker: owns the only network code. Batches events and flushes on size or
/// time. All failures are swallowed (telemetry must never crash the app).
fn worker(rx: Receiver<Value>, envelope: Map<String, Value>) {
    let agent: ureq::Agent = ureq::Agent::config_builder()
        .timeout_global(Some(HTTP_TIMEOUT))
        .build()
        .into();
    let url = format!("{POSTHOG_HOST}/batch/");
    let mut batch: Vec<Value> = Vec::new();

    loop {
        match rx.recv_timeout(FLUSH_AFTER) {
            Ok(mut entry) => {
                enrich(&mut entry, &envelope);
                batch.push(entry);
                if batch.len() >= BATCH_MAX {
                    flush(&agent, &url, &mut batch);
                }
            }
            Err(RecvTimeoutError::Timeout) => {
                if !batch.is_empty() {
                    flush(&agent, &url, &mut batch);
                }
            }
            Err(RecvTimeoutError::Disconnected) => {
                flush(&agent, &url, &mut batch);
                return;
            }
        }
    }
}

/// Merge the per-install envelope into an event's `properties` (without clobbering
/// event-specific keys) and stamp the running dropped-count so a wedged queue is
/// observable in the data.
fn enrich(entry: &mut Value, envelope: &Map<String, Value>) {
    let Some(props) = entry.get_mut("properties").and_then(Value::as_object_mut) else {
        return;
    };
    for (k, v) in envelope {
        props.entry(k.clone()).or_insert_with(|| v.clone());
    }
    let dropped = DROPPED.swap(0, Ordering::Relaxed);
    if dropped > 0 {
        props.insert("dropped_since_last".into(), json!(dropped));
    }
}

fn flush(agent: &ureq::Agent, url: &str, batch: &mut Vec<Value>) {
    if batch.is_empty() {
        return;
    }
    let body = json!({ "api_key": POSTHOG_KEY, "batch": std::mem::take(batch) });
    match agent.post(url).header("content-type", "application/json").send_json(&body) {
        Ok(_) => {}
        Err(e) => log::debug!("telemetry flush failed (dropped, will not retry): {e}"),
    }
}

/// Tauri command: the frontend reads this once on load to decide whether to init
/// PostHog and, if so, with what `distinct_id` (shared with Rust so JS + Rust
/// events unify). The `posthog_key`/`api_host` are public ingest values.
#[tauri::command]
pub fn telemetry_config(app: tauri::AppHandle) -> Value {
    use tauri::Manager;
    let settings = app.state::<crate::settings::SettingsStore>().get();
    let enabled = is_enabled(settings.telemetry_enabled);
    json!({
        "enabled": enabled,
        "distinct_id": settings.device_id,
        "posthog_key": POSTHOG_KEY,
        "api_host": POSTHOG_HOST,
        "app_version": app.package_info().version.to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enabled_precedence_off_beats_everything() {
        assert!(!resolve_enabled(true, Some("off"), false));
        assert!(!resolve_enabled(true, Some("OFF"), true));
        assert!(!resolve_enabled(true, Some("0"), false));
    }

    #[test]
    fn enabled_precedence_on_overrides_settings_and_test_mode() {
        assert!(resolve_enabled(false, Some("on"), false));
        assert!(resolve_enabled(false, Some("1"), true), "explicit on works during test mode (verification)");
    }

    #[test]
    fn enabled_test_mode_is_off_unless_forced() {
        // Automated checkpoint runs (test mode, no explicit env) must not ship to prod.
        assert!(!resolve_enabled(true, None, true));
    }

    #[test]
    fn enabled_follows_settings_when_no_env_and_not_test_mode() {
        assert!(resolve_enabled(true, None, false));
        assert!(!resolve_enabled(false, None, false));
    }

    #[test]
    fn record_is_a_noop_when_uninitialized() {
        // SINK is unset in the unit-test process: record must not panic or block.
        record("never_sent", json!({"a": 1}));
        log_event("warn", "test", "no sink, no crash");
    }

    #[test]
    fn sha256_hex_is_stable_and_lowercase_hex() {
        let a = sha256_hex(b"felipe@skyward.ai");
        let b = sha256_hex(b"felipe@skyward.ai");
        assert_eq!(a, b, "deterministic");
        assert_eq!(a.len(), 64);
        assert!(a.chars().all(|c| c.is_ascii_hexdigit()));
        assert_ne!(a, sha256_hex(b"someone@else.com"));
    }

    #[test]
    fn enrich_merges_envelope_without_clobbering_event_props() {
        let mut env = Map::new();
        env.insert("distinct_id".into(), json!("dev-123"));
        env.insert("app_version".into(), json!("0.4.4"));
        let mut entry = json!({
            "event": "alarm_fired",
            "properties": { "kind": "t_zero", "app_version": "override-me-not" },
        });
        enrich(&mut entry, &env);
        let props = entry["properties"].as_object().unwrap();
        assert_eq!(props["distinct_id"], json!("dev-123"), "envelope key added");
        assert_eq!(props["kind"], json!("t_zero"), "event prop preserved");
        assert_eq!(props["app_version"], json!("override-me-not"), "event prop wins over envelope");
    }

    #[test]
    fn enrich_stamps_dropped_count_when_nonzero() {
        DROPPED.store(3, Ordering::Relaxed);
        let mut entry = json!({ "event": "x", "properties": {} });
        enrich(&mut entry, &Map::new());
        assert_eq!(entry["properties"]["dropped_since_last"], json!(3));
        // And it resets, so the next event doesn't double-count.
        let mut entry2 = json!({ "event": "y", "properties": {} });
        enrich(&mut entry2, &Map::new());
        assert!(entry2["properties"].get("dropped_since_last").is_none());
    }
}
