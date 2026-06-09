//! Alarm scheduler — the production timing engine.
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

/// Events injected via test-mode IPC override EventKit.
pub static INJECTED_EVENTS: Mutex<Option<Vec<AlarmEvent>>> = Mutex::new(None);

/// Alarms currently presented in the overlay. The overlay webview boots AFTER
/// the fire emit, so it pulls these on mount (and also listens for later emits).
pub static ACTIVE_ALARMS: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

/// Last-seen "did the calendar pipeline yield ANY events" state, for the
/// transition log below. None = no tick has run yet.
static LAST_EVENT_PRESENCE: Mutex<Option<bool>> = Mutex::new(None);

/// Decide whether a per-tick event count warrants an INFO line. We log only on
/// the EDGE (had events → 0, or 0 → had events), never every tick: the steady
/// state is silent, but "my calendar went empty" / "events came back" — the
/// exact symptom from the access saga — always leaves a mark. Pure for testing.
fn presence_transition(prev: Option<bool>, count: usize) -> Option<String> {
    let now_has = count > 0;
    if prev == Some(now_has) {
        return None;
    }
    Some(if now_has {
        format!("calendar pipeline now yields {count} event(s)")
    } else {
        "calendar pipeline yields 0 events (was non-empty) — tray/alerts will be empty".to_string()
    })
}

/// Lock a mutex, recovering the guard even if a prior holder panicked. The alarm
/// path must degrade to "keep going" on a poisoned lock, never "panic forever":
/// a poisoned scheduler mutex would otherwise make every subsequent `.lock()`
/// panic and silently kill all future alarms — the one unforgivable failure.
fn lock_resilient<T>(m: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    m.lock().unwrap_or_else(|e| e.into_inner())
}

#[tauri::command]
pub fn get_active_alarms() -> Vec<serde_json::Value> {
    lock_resilient(&ACTIVE_ALARMS).clone()
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
                has_link: true,
            })
        })
        .collect();
    let parsed = parsed?;
    let n = parsed.len();
    *lock_resilient(&INJECTED_EVENTS) = Some(parsed);
    Ok(n)
}

/// ENTUCARA_TEST_EVENTS='[{"key":"e2e","title":"…","start_in":15,"duration":60}]'
/// — relative seconds from process launch; lets a shell script drive a full
/// alarm lifecycle in seconds without webview IPC.
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
                            has_link: s["has_link"].as_bool().unwrap_or(true),
                        }
                    })
                    .collect(),
            )
        })
        .clone()
}

fn upcoming_alarm_events(settings: &crate::settings::Settings) -> Vec<AlarmEvent> {
    if let Some(injected) = lock_resilient(&INJECTED_EVENTS).clone() {
        return injected;
    }
    if crate::testmode::is_test_mode() {
        if let Some(env_events) = env_test_events() {
            return env_events;
        }
    }
    // Sync the event store (pull remote changes + drop the stale local cache),
    // then read — so external deletes/edits are reflected (freshness; ≤1 min cadence).
    crate::calendar::sync_event_store();
    // EventKit fetch: 1 day back (ongoing events started earlier) + 2 forward.
    // The forward window is 2 days, not 1, so a DST "spring forward" day (a 23h
    // wall-clock day) can never clip the next 24h of events out of the query.
    // Over-fetching is free — compute_actions filters by time.
    crate::calendar::fetch_events(1, 2)
        .unwrap_or_default()
        .into_iter()
        .filter(|e| match (&settings.enabled_calendar_ids, &e.calendar_id) {
            (Some(enabled), Some(cal)) => enabled.contains(cal),
            (Some(_), None) => true, // no calendar id — never silently drop
            (None, _) => true,
        })
        .filter_map(|e| {
            Some(AlarmEvent {
                start: DateTime::parse_from_rfc3339(&e.start).ok()?.with_timezone(&Utc),
                end: DateTime::parse_from_rfc3339(&e.end).ok()?.with_timezone(&Utc),
                has_link: crate::calendar::has_meeting_link(
                    e.url.as_deref(),
                    e.location.as_deref(),
                    e.notes.as_deref(),
                ),
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
    let settings = app.state::<crate::settings::SettingsStore>().get();
    let cfg = crate::alarm_core::AlarmConfig {
        lead_secs: i64::from(settings.lead_minutes) * 60,
        alert_tentative: settings.alert_tentative,
        alert_pending: settings.alert_pending,
        only_video_events: settings.only_video_events,
    };
    let events = upcoming_alarm_events(&settings);
    {
        // Edge-triggered "calendar went empty / came back" signal (INFO).
        let mut last = lock_resilient(&LAST_EVENT_PRESENCE);
        if let Some(msg) = presence_transition(*last, events.len()) {
            log::info!("{msg}");
        }
        *last = Some(!events.is_empty());
    }
    let state = app.state::<SharedState>();

    let actions = {
        let alarms = lock_resilient(&state.alarms);
        compute_actions(&events, now, &alarms, &cfg)
    };

    for action in &actions {
        crate::testmode::log_fire(
            &action.occurrence_key,
            match action.kind {
                AlarmKind::TMinus5 => "t_minus_5",
                AlarmKind::TZero => "t_zero",
                AlarmKind::Snooze => "snooze",
            },
            action.due_at,
        );
        // Suppressed actions carry no overlay (dedup/policy) — record them fired
        // so they aren't reconsidered, and move on.
        if action.suppressed {
            state.update(|a| a.mark_fired(&action.occurrence_key, action.kind, now));
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

        // Show the overlay on the main thread and WAIT for the outcome. We mark
        // the alarm fired ONLY once the overlay is confirmed up — otherwise a
        // failed show would advance the fired-set and the alert would be
        // swallowed forever with no retry (the unforgivable failure). Waiting
        // synchronously here is safe: the next tick can't begin until this one
        // returns, so there is no window for a double-fire. On failure/timeout we
        // leave the alarm unmarked and the next tick retries it.
        let (tx, rx) = std::sync::mpsc::channel();
        let handle = app.clone();
        let shown_payload = payload.clone();
        let _ = app.run_on_main_thread(move || {
            let result = crate::overlay::show_overlays(&handle);
            if result.is_ok() {
                // Already-booted overlay windows get the push; freshly created
                // ones pull via get_active_alarms on mount.
                lock_resilient(&ACTIVE_ALARMS).push(shown_payload.clone());
                let _ = handle.emit("alarm-fired", &shown_payload);
            }
            let _ = tx.send(result.map(|_| ()).map_err(|e| e.to_string()));
        });
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {
                state.update(|a| a.mark_fired(&action.occurrence_key, action.kind, now));
            }
            Ok(Err(e)) => {
                log::error!(
                    "overlay show failed for {}: {e}; leaving unmarked to retry next tick",
                    action.occurrence_key
                );
            }
            Err(_) => {
                log::error!(
                    "overlay show timed out for {}; leaving unmarked to retry next tick",
                    action.occurrence_key
                );
            }
        }
    }

    // Menu-bar next-event title (user req): "Title… · 12m". Cleared when disabled
    // or nothing upcoming. Derived by the SAME tray::next_event_title the popover
    // refresh uses — one computation, so the menu-bar text and the popover list
    // can't drift. This is the background (popover-closed) trigger; opening the
    // popover refreshes it immediately via refresh_popover.
    let title = if settings.show_next_event_in_menu_bar {
        let candidates: Vec<crate::tray::MenuBarCandidate> = events
            .iter()
            .map(|e| crate::tray::MenuBarCandidate {
                title: &e.title,
                start: e.start,
                all_day: e.all_day,
                status: &e.status,
            })
            .collect();
        crate::tray::next_event_title(&candidates, now, settings.menu_bar_title_chars as usize)
    } else {
        None
    };
    {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || crate::tray::set_tray_title(&handle, title));
    }

    // Wall-clock arming: wake at the next due alarm (capped) or the poll backstop.
    let alarms = lock_resilient(&state.alarms);
    match next_due(&events, now, &alarms, &cfg) {
        Some(due) => ((due - now).num_seconds().clamp(1, 30)) as u64,
        None => 30,
    }
}

/// Spawn the production scheduler loop. Tick cadence ≤30 s (poll backstop —
/// catches calendar edits and self-heals after sleep: first tick post-wake runs
/// compute_actions and the fire-if-ongoing policy covers missed alarms).
/// Holds the latencyCritical assertion only while an alarm is ≤120 s out.
pub fn spawn_loop(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        // Startup grace: creating overlay windows (transparent + effects) while
        // Tauri setup is still settling aborts the app with a foreign ObjC
        // exception (CP3 regression, 2026-06-05). An alarm app gains nothing
        // from firing in the first 2 s of its life.
        std::thread::sleep(Duration::from_secs(2));
        loop {
        // A panic inside one tick must NEVER permanently kill the scheduler: a
        // dead loop thread = no alarm ever fires again, silently. Catch it, log,
        // back off briefly, and keep ticking — losing one tick self-heals on the
        // next pass (compute_actions re-runs), losing the thread does not.
        let sleep_secs = match std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| tick(&app))) {
            Ok(secs) => secs,
            Err(_) => {
                log::error!("scheduler tick panicked; backing off 5s and continuing");
                5
            }
        };

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
        }
    });
}

/// Theme/sound preview (settings ▸ Appearance ▸ Show Demo Alert): fire the real
/// overlay + sound loop with a fake meeting. Dismiss works like any alarm.
#[tauri::command]
pub fn demo_alert(app: tauri::AppHandle) {
    let now = crate::testmode::clock::now();
    let payload = serde_json::json!({
        "occurrence_key": "(demo @ now)",
        "kind": "t_minus5",
        "title": "Hello, I'm a demo event",
        "start": (now + ChronoDuration::minutes(45)).to_rfc3339(),
        "end": (now + ChronoDuration::minutes(90)).to_rfc3339(),
    });
    lock_resilient(&ACTIVE_ALARMS).push(payload.clone());
    let handle = app.clone();
    let _ = app.run_on_main_thread(move || {
        match crate::overlay::show_overlays(&handle) {
            Ok(_) => {
                let _ = handle.emit("alarm-fired", &payload);
            }
            Err(e) => eprintln!("demo overlay failed: {e}"),
        }
    });
}

/// The active-alarm payloads NOT belonging to `occurrence_key`. Pure over the
/// payload vec so the per-occurrence isolation is unit-testable.
fn retain_other_occurrences(
    active: &[serde_json::Value],
    occurrence_key: &str,
) -> Vec<serde_json::Value> {
    active
        .iter()
        .filter(|p| p.get("occurrence_key").and_then(|v| v.as_str()) != Some(occurrence_key))
        .cloned()
        .collect()
}

/// Is `occurrence_key` currently presented in the overlay? Pure over the payload
/// vec so the "drop the live card on ignore" guard is unit-testable.
fn is_occurrence_active(active: &[serde_json::Value], occurrence_key: &str) -> bool {
    active
        .iter()
        .any(|p| p.get("occurrence_key").and_then(|v| v.as_str()) == Some(occurrence_key))
}

/// Finish ONE occurrence: drop its card(s) from the active set, then close the
/// overlay only if nothing remains — otherwise tell the still-open overlay to
/// re-render the reduced set. This is what lets two overlapping meetings be
/// actioned independently: dismissing/snoozing one must never silently take the
/// other down with it (the old code cleared ALL active alarms on any action).
fn finish_one(app: &tauri::AppHandle, occurrence_key: &str) {
    let remaining = {
        let mut active = lock_resilient(&ACTIVE_ALARMS);
        *active = retain_other_occurrences(&active, occurrence_key);
        active.clone()
    };
    if remaining.is_empty() {
        crate::overlay::close_overlays(app.clone());
    } else {
        let _ = app.emit("alarms-updated", remaining);
    }
}

#[tauri::command]
pub fn snooze_alarm(app: tauri::AppHandle, occurrence_key: String, minutes: i64) {
    let state = app.state::<SharedState>();
    let until = crate::testmode::clock::now() + ChronoDuration::minutes(minutes);
    state.update(|a| a.snooze(&occurrence_key, until));
    finish_one(&app, &occurrence_key);
}

/// Dismiss ONE occurrence (when `occurrence_key` is supplied by a card button) or
/// ALL active alarms (Esc and the zero-card safety Dismiss pass nothing).
/// Dismiss-all stays the blunt "get everything off my screen" escape hatch.
#[tauri::command]
pub fn dismiss_alarms(app: tauri::AppHandle, occurrence_key: Option<String>) {
    match occurrence_key {
        Some(key) => finish_one(&app, &key),
        None => {
            lock_resilient(&ACTIVE_ALARMS).clear();
            crate::overlay::close_overlays(app);
        }
    }
}

#[tauri::command]
pub fn set_paused(app: tauri::AppHandle, paused: bool) {
    let state = app.state::<SharedState>();
    state.update(|a| a.paused = paused);
}

#[tauri::command]
pub fn get_paused(app: tauri::AppHandle) -> bool {
    let state = app.state::<SharedState>();
    let paused = lock_resilient(&state.alarms).paused;
    paused
}

/// Ignore a single occurrence so it never alerts (per-occurrence — a recurring
/// series' other instances are untouched). Right-click → Ignore in the tray.
/// `ends_at` is the occurrence's END (RFC3339): the ignore is GC'd 48h after
/// the occurrence ends, NOT 48h after the click — events ignored up to 7 days
/// out used to silently re-arm (bug H1).
#[tauri::command]
pub fn ignore_occurrence(app: tauri::AppHandle, occurrence_key: String, ends_at: String) {
    // Fall back to "now" if the end can't be parsed — better a slightly-early
    // GC than dropping the ignore entirely. The tray always supplies the end.
    let ends_at = DateTime::parse_from_rfc3339(&ends_at)
        .map(|d| d.with_timezone(&Utc))
        .unwrap_or_else(|_| crate::testmode::clock::now());
    let state = app.state::<SharedState>();
    state.update(|a| a.ignore(&occurrence_key, ends_at));

    // If that occurrence is CURRENTLY on screen, ignoring it must also drop the
    // live card (and stop the sound / close the overlay if it was the last one) —
    // otherwise the card stays up and the alert loops despite being ignored.
    // Guard on "is it actually active" so we never needlessly close_overlays /
    // stop the loop when nothing for this key is showing.
    if is_occurrence_active(&lock_resilient(&ACTIVE_ALARMS), &occurrence_key) {
        finish_one(&app, &occurrence_key);
    }
}

/// Undo an ignore (right-click → Stop ignoring).
#[tauri::command]
pub fn unignore_occurrence(app: tauri::AppHandle, occurrence_key: String) {
    let state = app.state::<SharedState>();
    state.update(|a| a.unignore(&occurrence_key));
}

/// The occurrence_keys currently ignored — the tray reads this to dim them.
#[tauri::command]
pub fn get_ignored(app: tauri::AppHandle) -> Vec<String> {
    let state = app.state::<SharedState>();
    let keys: Vec<String> = lock_resilient(&state.alarms).ignored.keys().cloned().collect();
    keys
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn presence_transition_only_logs_on_the_edge() {
        // First observation always speaks (None → known).
        assert!(presence_transition(None, 5).is_some());
        assert!(presence_transition(None, 0).is_some());
        // Steady state is silent — no per-tick spam.
        assert!(presence_transition(Some(true), 5).is_none());
        assert!(presence_transition(Some(false), 0).is_none());
        // The two edges that matter both speak.
        let went_empty = presence_transition(Some(true), 0).expect("empty edge logs");
        assert!(went_empty.contains("0 events"), "got: {went_empty}");
        let came_back = presence_transition(Some(false), 3).expect("non-empty edge logs");
        assert!(came_back.contains('3'), "got: {came_back}");
    }

    #[test]
    fn lock_resilient_recovers_from_poison() {
        let m: Mutex<i32> = Mutex::new(7);
        // Poison it the way a real panic-under-lock would.
        let _ = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            let _g = m.lock().unwrap();
            panic!("boom while holding the alarm lock");
        }));
        assert!(m.lock().is_err(), "lock should be poisoned by the panic");
        // The alarm path must still recover the guard + value, never panic.
        assert_eq!(*lock_resilient(&m), 7);
    }

    #[test]
    fn loop_guard_survives_a_panicking_tick() {
        // Mirrors spawn_loop's catch_unwind guard: a panicking iteration is
        // caught and the loop keeps running instead of the thread dying forever.
        let mut completed = 0;
        for i in 0..4 {
            let r = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                if i == 1 {
                    panic!("tick boom");
                }
                i
            }));
            if r.is_ok() {
                completed += 1;
            }
        }
        assert_eq!(completed, 3, "every non-panicking tick ran despite the panic at i==1");
    }

    #[test]
    fn dismissing_one_occurrence_keeps_the_other() {
        // Two overlapping meetings on screen, meeting A with both T-5 and T-0
        // cards. Dismissing A must drop BOTH A cards and leave B untouched.
        let active = vec![
            serde_json::json!({"occurrence_key": "(A @ t)", "kind": "t_minus_5"}),
            serde_json::json!({"occurrence_key": "(A @ t)", "kind": "t_zero"}),
            serde_json::json!({"occurrence_key": "(B @ t)", "kind": "t_zero"}),
        ];
        let remaining = retain_other_occurrences(&active, "(A @ t)");
        assert_eq!(remaining.len(), 1, "only B should remain after dismissing A");
        assert_eq!(remaining[0]["occurrence_key"], "(B @ t)");
    }

    #[test]
    fn is_occurrence_active_detects_a_live_card() {
        // The ignore path uses this to decide whether to also drop the live card:
        // true → finish_one (close/re-render + stop sound); false → leave overlay
        // untouched. A key with ANY card on screen (even a different kind) is active.
        let active = vec![
            serde_json::json!({"occurrence_key": "(A @ t)", "kind": "t_minus_5"}),
            serde_json::json!({"occurrence_key": "(B @ t)", "kind": "t_zero"}),
        ];
        assert!(is_occurrence_active(&active, "(A @ t)"));
        assert!(is_occurrence_active(&active, "(B @ t)"));
        assert!(!is_occurrence_active(&active, "(C @ t)"), "absent key is not active");
        assert!(!is_occurrence_active(&[], "(A @ t)"), "empty set: nothing is active");
    }

    #[test]
    fn dismissing_the_only_occurrence_empties_the_set() {
        let active = vec![serde_json::json!({"occurrence_key": "(A @ t)", "kind": "t_zero"})];
        assert!(
            retain_other_occurrences(&active, "(A @ t)").is_empty(),
            "removing the last occurrence empties the set so the overlay closes"
        );
    }
}

use tauri::Manager;
