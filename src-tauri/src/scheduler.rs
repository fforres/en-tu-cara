//! Alarm scheduler (Phase 1d spike → grows into the production engine, PLAN §1).
//!
//! Core principles (both verified by adversarial review + Apple docs):
//!   - Arm against WALL-CLOCK, never duration sleeps: mach timers pause in sleep
//!     and App Nap stretches them; we re-check the target time on short ticks.
//!   - Timer precision in a background Accessory app requires an
//!     `NSActivityLatencyCritical` assertion — `.userInitiated` alone does NOT
//!     defeat App Nap timer throttling. Held WINDOWED (≤2 min before fire), not 24/7.
//!
//! CP1d spike: ENTUCARA_SPIKE_FIRE="<delay_secs>,<arm>" with arm ∈
//! none | userinitiated | latencycritical. Arms an alarm <delay_secs> out, holds the
//! requested assertion, fires, and appends a latency record to
//! ~/Library/Application Support/dev.fforres.entucara/fire-spike.jsonl, then exits.

#![cfg(target_os = "macos")]

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
use serde::Serialize;
use std::time::Duration;

/// RAII holder for an NSProcessInfo activity assertion.
pub struct ActivityAssertion {
    token: Retained<objc2::runtime::ProtocolObject<dyn NSObjectProtocol>>,
}

impl ActivityAssertion {
    pub fn begin(options: NSActivityOptions, reason: &str) -> Self {
        let info = NSProcessInfo::processInfo();
        let token = info.beginActivityWithOptions_reason(options, &NSString::from_str(reason));
        Self { token }
    }
}

impl Drop for ActivityAssertion {
    fn drop(&mut self) {
        let info = NSProcessInfo::processInfo();
        // SAFETY: token came from beginActivityWithOptions_reason on this process
        // and is ended exactly once (RAII).
        unsafe { info.endActivity(&self.token) };
    }
}

#[derive(Debug, Serialize)]
struct FireSpikeRecord {
    arm: String,
    scheduled_for: DateTime<Utc>,
    fired_at: DateTime<Utc>,
    latency_ms: i64,
    delay_secs: i64,
    on_battery: bool,
    macos: String,
}

fn on_battery() -> bool {
    std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("Battery Power"))
        .unwrap_or(false)
}

fn macos_version() -> String {
    std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Wall-clock-targeted wait: short ticks, re-checking the target each time.
/// Under App Nap the TICKS get stretched — which is exactly the latency we measure.
/// (Production will also re-arm on NSWorkspace didWake; spike keeps it minimal.)
fn wait_until(target: DateTime<Utc>) {
    loop {
        let now = Utc::now();
        if now >= target {
            return;
        }
        let remaining = (target - now).num_milliseconds();
        // Coarse ticks far out, fine ticks close in.
        let tick = if remaining > 10_000 { 1_000 } else { 50 };
        std::thread::sleep(Duration::from_millis(tick.min(remaining.max(1)) as u64));
    }
}

// ---------------------------------------------------------------------------
// Production loop (Phase 3)
// ---------------------------------------------------------------------------

use crate::alarm_core::{compute_actions, next_due, AlarmEvent, AlarmKind};
use crate::state::SharedState;
use serde::Deserialize;
use std::sync::Mutex;
use tauri::Emitter;

/// Events injected via test-mode IPC override EventKit (PLAN §1 test mode).
pub static INJECTED_EVENTS: Mutex<Option<Vec<AlarmEvent>>> = Mutex::new(None);

/// Alarms currently presented in the overlay. The overlay webview boots AFTER
/// the fire emit, so it pulls these on mount (and also listens for later emits).
pub static ACTIVE_ALARMS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

#[tauri::command]
pub fn get_active_alarms() -> Vec<serde_json::Value> {
    ACTIVE_ALARMS.lock().unwrap().clone()
}

#[derive(Debug, Deserialize)]
pub struct InjectableEvent {
    pub occurrence_key: String,
    pub title: String,
    pub start: String, // RFC3339
    pub end: String,
    #[serde(default)]
    pub all_day: bool,
    #[serde(default = "default_status")]
    pub status: String,
    #[serde(default)]
    pub my_rsvp: Option<String>,
}
fn default_status() -> String {
    "confirmed".into()
}

#[tauri::command]
pub fn inject_events(events: Vec<InjectableEvent>) -> Result<usize, String> {
    if !crate::testmode::is_test_mode() {
        return Err("not in test mode".into());
    }
    let parsed: Result<Vec<AlarmEvent>, String> = events
        .into_iter()
        .map(|e| {
            Ok(AlarmEvent {
                start: DateTime::parse_from_rfc3339(&e.start)
                    .map_err(|err| err.to_string())?
                    .with_timezone(&Utc),
                end: DateTime::parse_from_rfc3339(&e.end)
                    .map_err(|err| err.to_string())?
                    .with_timezone(&Utc),
                occurrence_key: e.occurrence_key,
                title: e.title,
                all_day: e.all_day,
                status: e.status,
                my_rsvp: e.my_rsvp,
            })
        })
        .collect();
    let parsed = parsed?;
    let n = parsed.len();
    *INJECTED_EVENTS.lock().unwrap() = Some(parsed);
    Ok(n)
}

/// ENTUCARA_TEST_EVENTS='[{"key":"e2e","title":"…","start_in":15,"duration":60}]'
/// — relative seconds from process launch; lets a shell script drive a full
/// alarm lifecycle in seconds without webview IPC (PLAN test-mode substitute).
fn env_test_events() -> Option<Vec<AlarmEvent>> {
    use std::sync::OnceLock;
    static PARSED: OnceLock<Option<Vec<AlarmEvent>>> = OnceLock::new();
    PARSED
        .get_or_init(|| {
            let raw = std::env::var("ENTUCARA_TEST_EVENTS").ok()?;
            let base = Utc::now();
            let specs: Vec<serde_json::Value> = serde_json::from_str(&raw).ok()?;
            Some(
                specs
                    .iter()
                    .map(|s| {
                        let start_in = s["start_in"].as_i64().unwrap_or(15);
                        let duration = s["duration"].as_i64().unwrap_or(60);
                        let start = base + ChronoDuration::seconds(start_in);
                        AlarmEvent {
                            occurrence_key: s["key"].as_str().unwrap_or("e2e").to_string(),
                            title: s["title"].as_str().unwrap_or("E2E Test Meeting").to_string(),
                            start,
                            end: start + ChronoDuration::seconds(duration),
                            all_day: false,
                            status: s["status"].as_str().unwrap_or("confirmed").to_string(),
                            my_rsvp: s["my_rsvp"].as_str().map(String::from),
                        }
                    })
                    .collect(),
            )
        })
        .clone()
}

fn upcoming_alarm_events() -> Vec<AlarmEvent> {
    if let Some(injected) = INJECTED_EVENTS.lock().unwrap().clone() {
        return injected;
    }
    if crate::testmode::is_test_mode() {
        if let Some(env_events) = env_test_events() {
            return env_events;
        }
    }
    // EventKit fetch: 1 day back (ongoing events started earlier) + 1 forward.
    crate::calendar::fetch_events(1, 1)
        .unwrap_or_default()
        .into_iter()
        .filter_map(|e| {
            Some(AlarmEvent {
                start: DateTime::parse_from_rfc3339(&e.start).ok()?.with_timezone(&Utc),
                end: DateTime::parse_from_rfc3339(&e.end).ok()?.with_timezone(&Utc),
                occurrence_key: e.occurrence_key,
                title: e.title,
                all_day: e.all_day,
                status: e.status,
                my_rsvp: e.my_rsvp,
            })
        })
        .collect()
}

/// One scheduler pass: fetch → decide → fire. Returns seconds until next wake.
fn tick(app: &tauri::AppHandle) -> u64 {
    let now = crate::testmode::clock::now();
    let events = upcoming_alarm_events();
    let state = app.state::<SharedState>();

    let actions = {
        let alarms = state.alarms.lock().unwrap();
        compute_actions(&events, now, &alarms)
    };

    for action in &actions {
        state.update(|a| a.mark_fired(&action.occurrence_key, action.kind, now));
        crate::testmode::log_fire(
            &action.occurrence_key,
            match action.kind {
                AlarmKind::TMinus5 => "t_minus_5",
                AlarmKind::TZero => "t_zero",
                AlarmKind::Snooze => "snooze",
            },
            action.due_at,
        );
        if action.suppressed {
            continue;
        }
        let event = events.iter().find(|e| e.occurrence_key == action.occurrence_key);
        let payload = serde_json::json!({
            "occurrence_key": action.occurrence_key,
            "kind": action.kind,
            "title": event.map(|e| e.title.clone()).unwrap_or_default(),
            "start": event.map(|e| e.start.to_rfc3339()),
            "end": event.map(|e| e.end.to_rfc3339()),
        });
        ACTIVE_ALARMS.lock().unwrap().push(payload.clone());
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            match crate::overlay::show_overlays(&handle) {
                Ok(_) => {
                    crate::sound::play(crate::sound::DEFAULT_ALERT_SOUND);
                    // Already-booted overlay windows get the push; freshly created
                    // ones pull via get_active_alarms on mount.
                    let _ = handle.emit("alarm-fired", &payload);
                }
                Err(e) => eprintln!("overlay failed: {e}"),
            }
        });
    }

    // Wall-clock arming: wake at the next due alarm (capped) or the poll backstop.
    let alarms = state.alarms.lock().unwrap();
    match next_due(&events, now, &alarms) {
        Some(due) => ((due - now).num_seconds().clamp(1, 30)) as u64,
        None => 30,
    }
}

/// Spawn the production scheduler loop. Tick cadence ≤30 s (poll backstop —
/// catches calendar edits and self-heals after sleep: first tick post-wake runs
/// compute_actions and the fire-if-ongoing policy covers missed alarms).
/// Holds the latencyCritical assertion only while an alarm is ≤120 s out (PLAN §1).
pub fn spawn_loop(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || loop {
        let sleep_secs = tick(&app);

        // Windowed precision assertion: close-in alarms get latencyCritical.
        if sleep_secs <= 120 {
            let _assertion = ActivityAssertion::begin(
                NSActivityOptions::UserInitiated | NSActivityOptions::LatencyCritical,
                "en-tu-cara: alarm imminent",
            );
            std::thread::sleep(Duration::from_secs(sleep_secs.max(1)));
        } else {
            std::thread::sleep(Duration::from_secs(sleep_secs.max(1)));
        }
    });
}

#[tauri::command]
pub fn snooze_alarm(app: tauri::AppHandle, occurrence_key: String, minutes: i64) {
    let state = app.state::<SharedState>();
    let until = crate::testmode::clock::now() + ChronoDuration::minutes(minutes);
    state.update(|a| a.snooze(&occurrence_key, until));
    ACTIVE_ALARMS.lock().unwrap().clear();
    crate::overlay::close_overlays(app.clone());
}

#[tauri::command]
pub fn dismiss_alarms(app: tauri::AppHandle) {
    ACTIVE_ALARMS.lock().unwrap().clear();
    crate::overlay::close_overlays(app);
}

#[tauri::command]
pub fn set_paused(app: tauri::AppHandle, paused: bool) {
    let state = app.state::<SharedState>();
    state.update(|a| a.paused = paused);
}

#[tauri::command]
pub fn get_paused(app: tauri::AppHandle) -> bool {
    let state = app.state::<SharedState>();
    let paused = state.alarms.lock().unwrap().paused;
    paused
}

// ---------------------------------------------------------------------------
// CP1d spike (kept for latency re-measurement)
// ---------------------------------------------------------------------------

/// CP1d: ENTUCARA_SPIKE_FIRE="<delay_secs>,<arm>" → measure fire latency, exit app.
pub fn maybe_run_fire_spike(app: &tauri::AppHandle) {
    let Ok(spec) = std::env::var("ENTUCARA_SPIKE_FIRE") else {
        return;
    };
    let (delay_str, arm) = spec.split_once(',').unwrap_or((spec.as_str(), "none"));
    let delay_secs: i64 = delay_str.parse().unwrap_or(60);
    let arm = arm.to_lowercase();
    let app = app.clone();

    std::thread::spawn(move || {
        let scheduled_for = Utc::now() + ChronoDuration::seconds(delay_secs);

        // Hold the requested assertion for the whole wait. (Production holds it
        // windowed ≤2 min before fire; the spike holds it throughout so the arm
        // under test is unambiguous.)
        let _assertion = match arm.as_str() {
            "userinitiated" => Some(ActivityAssertion::begin(
                NSActivityOptions::UserInitiated,
                "en-tu-cara fire spike: userInitiated",
            )),
            "latencycritical" => Some(ActivityAssertion::begin(
                NSActivityOptions::UserInitiated | NSActivityOptions::LatencyCritical,
                "en-tu-cara fire spike: userInitiated|latencyCritical",
            )),
            _ => None,
        };

        println!(
            "SPIKE_FIRE armed: arm={arm} delay={delay_secs}s target={}",
            scheduled_for.to_rfc3339()
        );
        wait_until(scheduled_for);
        let fired_at = Utc::now();

        let record = FireSpikeRecord {
            arm: arm.clone(),
            scheduled_for,
            fired_at,
            latency_ms: (fired_at - scheduled_for).num_milliseconds(),
            delay_secs,
            on_battery: on_battery(),
            macos: macos_version(),
        };
        println!("SPIKE_FIRE fired: {}", serde_json::to_string(&record).unwrap());

        if let Ok(dir) = app.path().app_data_dir() {
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("fire-spike.jsonl"))
            {
                use std::io::Write;
                let _ = writeln!(f, "{}", serde_json::to_string(&record).unwrap());
            }
        }
        app.exit(0);
    });
}

use tauri::Manager;
