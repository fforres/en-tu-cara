//! Alarm scheduler — the production timing engine.
//!
//! Core principles (both verified by adversarial review + Apple docs):
//!   - Arm against WALL-CLOCK, never duration sleeps: mach timers pause in sleep
//!     and App Nap stretches them; we re-check the target time on short ticks.
//!   - Timer precision in a background Accessory app requires an
//!     `NSActivityLatencyCritical` assertion — `.userInitiated` alone does NOT
//!     defeat App Nap timer throttling. Held WINDOWED (≤2 min before fire), not 24/7.
//!
//! The CP1d latency-measurement spike lives in fire_spike.rs.

#![cfg(target_os = "macos")]

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
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

// ---------------------------------------------------------------------------
// Production loop (Phase 3)
// ---------------------------------------------------------------------------

use crate::alarm_core::{compute_actions, next_due, AlarmEvent};
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

/// The complete calendar-access health state (debounced announcer + repair
/// escalation + downtime counter) behind ONE lock — see access::AccessHealth.
/// Only real reads feed it (test/injected runs never do).
static ACCESS_HEALTH: Mutex<crate::access::AccessHealth> =
    Mutex::new(crate::access::AccessHealth::new());

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

/// Whether `tick()` should DISPATCH an overlay re-assert to the main thread this
/// pass: true exactly when an alarm is still presented. Drives the sleep/wake
/// self-heal — a fired alarm the user hasn't dismissed can lose its panels across
/// system sleep while the sound loop keeps beating, so we re-front (or recreate)
/// them every tick; an empty set must stay a no-op so an overlay never pops out
/// of nowhere. Pure, for testing.
fn should_dispatch_reassert(active_alarms: &[serde_json::Value]) -> bool {
    !active_alarms.is_empty()
}

/// The main-thread half of the re-assert guard, re-read at EXECUTION time.
///
/// Load-bearing, not redundant: `should_dispatch_reassert` runs on the SCHEDULER
/// thread and the show is queued onto the MAIN thread, so a dismiss/snooze can
/// land in between — clearing ACTIVE_ALARMS and closing the panels. The queued
/// closure would then find no live panels, BUILD FRESH ONES with an empty card
/// set, and restart the sound loop: an audible alarm the user just dismissed,
/// i.e. exactly the failure this self-heal exists to prevent. Re-reading the live
/// set here makes the closure a no-op in that interleaving. The reverse order is
/// already safe — we re-front, then the dismiss closes and silences.
fn reassert_still_warranted() -> bool {
    !lock_resilient(&ACTIVE_ALARMS).is_empty()
}

#[tauri::command]
pub fn get_active_alarms() -> Vec<serde_json::Value> {
    lock_resilient(&ACTIVE_ALARMS).clone()
}

/// Current calendar-access health, for the Settings banner to read on mount (it
/// also listens for live `access-state-changed` events). `None`/`Ok` → "ok".
/// `reason` (only meaningful when lost) lets the banners differentiate a
/// revoked grant (user must re-grant) from reads failing despite a grant (ours
/// to repair) — matching the live event's payload shape.
#[tauri::command]
pub fn get_access_state() -> serde_json::Value {
    let health = lock_resilient(&ACCESS_HEALTH);
    let lost = health.announced() == crate::access::AccessState::Lost;
    serde_json::json!({
        "state": if lost { "lost" } else { "ok" },
        "reason": health.announced_reason(),
    })
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

/// Returns the upcoming alarm events AND, for REAL calendar reads, the fetch
/// outcome that drives the access-health machine. Injected/test-mode events
/// return `None` (no real read happened) so the access alarm never trips during
/// tests even on a binary that holds no TCC grant.
/// Test-only forced calendar-access reading (ENTUCARA_TEST_ACCESS), so the
/// access-health machine + loud surfaces are testable without real EventKit
/// failures. Format: `<reason>[,recover_after=<secs>]` where reason ∈
/// ok | fetch_failed | not_determined | denied. With `recover_after`, it flips to
/// healthy after that many seconds from launch — exercising the full
/// lost→(debounced announce)→recovered flow in one run. Returns None outside
/// test mode or when unset.
fn test_access_override() -> Option<(crate::access::AuthKind, crate::access::FetchOutcome)> {
    use crate::access::{AuthKind, FetchOutcome};
    if !crate::testmode::is_test_mode() {
        return None;
    }
    use std::sync::OnceLock;
    // (reason, recover_after_secs, launch_base)
    type AccessSpec = (String, Option<i64>, DateTime<Utc>);
    static SPEC: OnceLock<Option<AccessSpec>> = OnceLock::new();
    let spec = SPEC
        .get_or_init(|| {
            let raw = std::env::var("ENTUCARA_TEST_ACCESS").ok()?;
            let mut parts = raw.split(',');
            let reason = parts.next()?.trim().to_string();
            let recover_after = parts
                .find_map(|p| p.trim().strip_prefix("recover_after=").and_then(|v| v.parse().ok()));
            Some((reason, recover_after, Utc::now()))
        })
        .clone();
    let (reason, recover_after, base) = spec?;
    if recover_after.is_some_and(|s| (crate::testmode::clock::now() - base).num_seconds() >= s) {
        return Some((AuthKind::FullAccess, FetchOutcome::Ok));
    }
    Some(match reason.as_str() {
        "ok" => (AuthKind::FullAccess, FetchOutcome::Ok),
        "not_determined" => (AuthKind::NotDetermined, FetchOutcome::Failed),
        "denied" => (AuthKind::DeniedOrRestricted, FetchOutcome::Failed),
        _ => (AuthKind::FullAccess, FetchOutcome::Failed), // "fetch_failed" / default
    })
}

/// Last-good event snapshot for the ALARM path (see snapshot.rs — the final
/// line of defense for the prime directive: alarms must not starve when reads
/// fail). The cache owns its own once-per-episode log edges.
static SNAPSHOT: Mutex<crate::snapshot::SnapshotCache> =
    Mutex::new(crate::snapshot::SnapshotCache::new());

fn upcoming_alarm_events(
    settings: &crate::settings::Settings,
) -> (Vec<AlarmEvent>, Option<crate::access::FetchOutcome>) {
    if let Some(injected) = lock_resilient(&INJECTED_EVENTS).clone() {
        return (injected, None);
    }
    if crate::testmode::is_test_mode() {
        if let Some(env_events) = env_test_events() {
            return (env_events, None);
        }
    }
    // THE canonical event read (sync + dedup + enabled-calendar filter), shared
    // with the tray popover — so the menu-bar title, the popover list, and what
    // can alarm never disagree. EventKit window: 1 day back (ongoing events
    // started earlier) + 2 forward (2, not 1, so a DST "spring forward" 23h day
    // can't clip the next 24h); over-fetching is free, compute_actions filters by
    // time.
    let events = crate::calendar::active_events(&settings.enabled_calendar_ids, 1, 2);
    let now = crate::testmode::clock::now();
    match events {
        Ok(events) => {
            let parsed: Vec<AlarmEvent> = events
                .into_iter()
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
                .collect();
            if lock_resilient(&SNAPSHOT).store(&parsed, now) {
                log::info!("calendar reads recovered — back to live events (snapshot retired)");
            }
            (parsed, Some(crate::access::FetchOutcome::Ok))
        }
        Err(e) => {
            // Reads are failing — the access machine (fed Failed below) owns the
            // loud surfaces + repair. The ALARM path must not starve meanwhile:
            // serve the last good snapshot while it's within its window.
            crate::telemetry::record("calendar_sync_failed", serde_json::json!({ "reason": e }));
            let events = match lock_resilient(&SNAPSHOT).serve(now) {
                Some(stale) => {
                    if stale.first {
                        log::warn!(
                            "calendar read failed — serving {} event(s) from the last good \
                             snapshot ({}s old); alerts stay armed while access heals",
                            stale.events.len(),
                            stale.age_secs
                        );
                    }
                    stale.events
                }
                None => Vec::new(),
            };
            (events, Some(crate::access::FetchOutcome::Failed))
        }
    }
}

/// Evaluate calendar-access health for one real read and act on any transition.
/// Owns the access state + downtime statics so `tick` stays scannable. Pure
/// decision lives in `access::evaluate_access`; this is its stateful driver.
fn check_calendar_access(
    app: &tauri::AppHandle,
    auth: crate::access::AuthKind,
    fetch: crate::access::FetchOutcome,
) {
    let obs = lock_resilient(&ACCESS_HEALTH).observe(auth, fetch);
    // Self-heal BEFORE the debounce announces: on any failing read, force a fresh
    // event store for the next tick. If a stale store was all it was, the next
    // read succeeds and the debounce never announces — recovery is silent (no
    // useless "lost"/"Grant access" notification for an already-valid grant).
    if obs.raw == crate::access::AccessState::Lost {
        crate::calendar::invalidate_event_store();
    }
    if let Some(edge) = &obs.edge {
        on_access_edge(app, edge, obs.down_ticks);
    }
    // Escalation: per-tick store rebuilds (above) fix a stale store; when N
    // consecutive REBUILT stores still read nothing despite FullAccess, the TCC
    // record itself is unusable (the 2026-06-10 poisoned-grant incident) —
    // destroy + re-grant it. Fires once per loss episode; all rails (test mode,
    // cooldown, identity guard) live in grant_repair::attempt.
    if obs.repair_due {
        log::warn!(
            "calendar grant appears unusable (authorized, but reads kept failing through \
             store rebuilds) — attempting automatic TCC grant repair"
        );
        crate::grant_repair::attempt(app, "auto");
    }
}

/// React to a calendar-access transition. Edge-triggered (runs ONCE per Ok↔Lost
/// transition, not every tick). Everything here must be non-blocking — it runs
/// inside the tick, which must never stall the alarm path; the loud surfaces and
/// self-heal all dispatch via spawn / run_on_main_thread / try_send.
fn on_access_edge(app: &tauri::AppHandle, edge: &crate::access::AccessEdge, down_ticks: u32) {
    match edge {
        crate::access::AccessEdge::Lost { reason } => {
            log::warn!("calendar access lost ({reason}) — alerts are paused until it is restored");
            crate::telemetry::record("calendar_access_lost", serde_json::json!({ "reason": reason }));
            // Loud surfaces (notification + menu-bar badge + settings banner).
            crate::access::announce_lost(app, reason);
            // Self-heal: recreate the (possibly stale) event store and, if the
            // grant was reset, re-prompt. Spawns its own work — does not block.
            crate::calendar::attempt_self_heal(app, reason);
        }
        crate::access::AccessEdge::Restored => {
            log::info!(
                "calendar access restored — alerts active again (was down {down_ticks} tick(s))"
            );
            crate::telemetry::record(
                "calendar_access_restored",
                serde_json::json!({ "down_ticks": down_ticks }),
            );
            crate::access::announce_restored(app);
        }
    }
}

/// Build the pure alarm policy from persisted settings. The ONLY place user
/// settings become alarm timing: reminder offsets are minutes → seconds, and an
/// empty `reminders` list stays empty (no pre-event reminders; the mandatory T-0
/// still fires). Pure + `pub(crate)` so the settings→timing seam is unit-testable.
pub(crate) fn build_alarm_config(settings: &crate::settings::Settings) -> crate::alarm_core::AlarmConfig {
    crate::alarm_core::AlarmConfig {
        reminder_offsets_secs: settings.reminders.iter().map(|&m| i64::from(m) * 60).collect(),
        alert_tentative: settings.alert_tentative,
        alert_pending: settings.alert_pending,
        only_video_events: settings.only_video_events,
    }
}

/// One scheduler pass: fetch → decide → fire. Returns seconds until next wake.
fn tick(app: &tauri::AppHandle) -> u64 {
    let now = crate::testmode::clock::now();

    // Self-heal overlay visibility. A fired alarm stays "presented" in
    // ACTIVE_ALARMS until the user dismisses it, but the takeover nspanels can be
    // torn down out from under us — most importantly across SYSTEM SLEEP: on wake
    // the display reconfiguration drops the panels while the (idempotent) sound
    // loop keeps beating, leaving the user with an audible alarm and no way to
    // stop it but force-quitting the app (reported). Re-assert on every tick so an
    // active alarm ALWAYS has a visible, dismissable overlay: show_overlays snaps
    // surviving panels back onto the CURRENT display layout, re-fronts them, and
    // recreates lost ones (a freshly built overlay re-pulls ACTIVE_ALARMS on
    // mount), and is a cheap no-op when the panels are already up and correctly
    // placed. This rides the scheduler's existing tick-driven self-heal (first tick
    // post-wake) rather than a fragile NSWorkspace observer — same reasoning as
    // `looks_like_wake` below.
    //
    // The guard is checked TWICE on purpose: once here to avoid the main-thread
    // hop, and again inside the closure because a dismiss can land in between —
    // see `reassert_still_warranted`.
    if should_dispatch_reassert(&lock_resilient(&ACTIVE_ALARMS)) {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || {
            if !reassert_still_warranted() {
                return; // dismissed/snoozed between dispatch and here — stay down.
            }
            if let Err(e) = crate::overlay::show_overlays(&handle) {
                log::error!("overlay self-heal re-assert failed: {e}");
            }
        });
    }

    let settings = app.state::<crate::settings::SettingsStore>().get();
    let cfg = build_alarm_config(&settings);
    let (events, fetch_outcome) = upcoming_alarm_events(&settings);
    {
        // Edge-triggered "calendar went empty / came back" signal (INFO).
        let mut last = lock_resilient(&LAST_EVENT_PRESENCE);
        if let Some(msg) = presence_transition(*last, events.len()) {
            log::info!("{msg}");
            crate::telemetry::record(
                "event_presence_change",
                serde_json::json!({ "count": events.len() }),
            );
        }
        *last = Some(!events.is_empty());
    }
    // Edge-triggered calendar-access health. Real reads supply (live auth, fetch
    // outcome); test mode can force the reading via ENTUCARA_TEST_ACCESS so the
    // lost→recover flow + the loud surfaces are deterministically testable
    // without real EventKit failures. The guard against the silent "lost access →
    // 0 events → no alarm" failure; it shouts + self-heals on the transition and
    // never gates the fire path below.
    let access_inputs = test_access_override()
        .or_else(|| fetch_outcome.map(|f| (crate::calendar::authorization_status_kind(), f)));
    if let Some((auth, fetch)) = access_inputs {
        check_calendar_access(app, auth, fetch);
    }
    let state = app.state::<SharedState>();

    let actions = {
        let alarms = lock_resilient(&state.alarms);
        compute_actions(&events, now, &alarms, &cfg)
    };

    for action in &actions {
        crate::testmode::log_fire(&action.occurrence_key, &action.kind.tag(), action.due_at);
        // Suppressed actions carry no overlay (dedup/policy) — record them fired
        // so they aren't reconsidered, and move on.
        if action.suppressed {
            state.update(|a| a.mark_fired(&action.occurrence_key, action.kind, now));
            continue;
        }
        let event = events.iter().find(|e| e.occurrence_key == action.occurrence_key);
        let payload = serde_json::json!({
            "occurrence_key": action.occurrence_key,
            "kind": action.kind.tag(),
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
        let occurrence_hash = crate::telemetry::sha256_hex(action.occurrence_key.as_bytes());
        match rx.recv_timeout(Duration::from_secs(5)) {
            Ok(Ok(())) => {
                state.update(|a| a.mark_fired(&action.occurrence_key, action.kind, now));
                // The product's defining success event: an alert actually shown.
                // Logged at INFO (previously unlogged) so the local file shows a
                // clean "fired on time" timeline during triage. `late_ms` = how
                // far past the scheduled time it fired. occurrence_key is hashed —
                // never sent in the clear (it embeds the calendar event id).
                let late_ms = (now - action.due_at).num_milliseconds();
                log::info!(
                    "alarm fired: kind={:?} occurrence_hash={occurrence_hash} late_ms={late_ms}",
                    action.kind
                );
                crate::telemetry::record(
                    "alarm_fired",
                    serde_json::json!({
                        "occurrence_hash": occurrence_hash,
                        "kind": action.kind.tag(),
                        "late_ms": late_ms,
                    }),
                );
            }
            Ok(Err(e)) => {
                log::error!(
                    "overlay show failed for {}: {e}; leaving unmarked to retry next tick",
                    action.occurrence_key
                );
                crate::telemetry::record(
                    "overlay_show_failed",
                    serde_json::json!({ "occurrence_hash": occurrence_hash, "reason": e }),
                );
            }
            Err(_) => {
                log::error!(
                    "overlay show timed out for {}; leaving unmarked to retry next tick",
                    action.occurrence_key
                );
                crate::telemetry::record(
                    "overlay_show_failed",
                    serde_json::json!({ "occurrence_hash": occurrence_hash, "reason": "timeout" }),
                );
            }
        }
    }

    // Menu-bar next-event title (user req): "Title… · 12m". Refresh the snapshot
    // from THIS poll's events, then re-derive + apply. The snapshot also feeds the
    // short-interval `spawn_menu_bar_loop`, which re-derives the title against the
    // live clock every ~10s — so the countdown stays current and a finished event
    // drops off promptly, without waiting for (or re-running) this ≤30s calendar
    // poll. Opening the popover refreshes the same snapshot via refresh_popover.
    let snapshot: Vec<crate::tray::OwnedCandidate> = events
        .iter()
        .map(|e| crate::tray::OwnedCandidate {
            title: e.title.clone(),
            start: e.start,
            all_day: e.all_day,
            status: e.status.clone(),
        })
        .collect();
    crate::tray::set_menu_bar_snapshot(snapshot);
    {
        let handle = app.clone();
        let _ = app.run_on_main_thread(move || crate::tray::refresh_menu_bar_title(&handle));
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
                // log::error! auto-ships to PostHog via the obs tracing layer.
                log::error!("scheduler tick panicked; backing off 5s and continuing");
                5
            }
        };

        // Windowed precision assertion: close-in alarms get latencyCritical.
        // Bracket the sleep with WALL-CLOCK readings: thread::sleep is suspended
        // during system sleep, so if far more wall-clock elapsed than we asked
        // for, the Mac slept — and an EKEventStore's connection to the calendar
        // daemon often dies across sleep, then serves stale "no data" forever
        // (the lost-access incident). On detecting a wake, force the store to
        // rebuild so the next tick reads fresh instead of failing for ≤2 ticks.
        let before = crate::testmode::clock::now();
        if sleep_secs <= 120 {
            let _assertion = ActivityAssertion::begin(
                NSActivityOptions::UserInitiated | NSActivityOptions::LatencyCritical,
                "en-tu-cara: alarm imminent",
            );
            std::thread::sleep(Duration::from_secs(sleep_secs.max(1)));
        } else {
            std::thread::sleep(Duration::from_secs(sleep_secs.max(1)));
        }
        let actual = (crate::testmode::clock::now() - before).num_seconds();
        if looks_like_wake(sleep_secs.max(1), actual) {
            log::info!("woke from sleep (~{actual}s vs {sleep_secs}s asked) — refreshing calendar store");
            crate::calendar::invalidate_event_store();
        }
        }
    });
}

/// True when far more wall-clock time elapsed across a sleep than requested — the
/// signal the machine was asleep (App Nap jitter stays well under the margin; a
/// real sleep adds minutes/hours). Pure, for testing. Drives the post-wake store
/// rebuild instead of a fragile NSWorkspace observer.
fn looks_like_wake(requested_secs: u64, actual_secs: i64) -> bool {
    actual_secs > requested_secs as i64 + 60
}

/// Dedicated menu-bar title refresher. Re-derives the "next event" title from the
/// latest calendar snapshot against the LIVE clock every 10s — independent of the
/// ≤30s alarm poll. This is what keeps the countdown current and drops a finished
/// event within ~10s; previously the title only changed when the alarm poll
/// happened to re-run, so a finished event could linger in the menu bar. Cheap:
/// no EventKit access, just a recompute over the cached snapshot + a title set.
pub fn spawn_menu_bar_loop(app: &tauri::AppHandle) {
    let app = app.clone();
    std::thread::spawn(move || {
        // Match the scheduler's setup grace — no window/tray work in the first 2s.
        std::thread::sleep(Duration::from_secs(2));
        loop {
            let handle = app.clone();
            let _ = app.run_on_main_thread(move || crate::tray::refresh_menu_bar_title(&handle));
            std::thread::sleep(Duration::from_secs(10));
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
        "kind": "reminder_45",
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
    crate::telemetry::record(
        "alarm_snoozed",
        serde_json::json!({
            "occurrence_hash": crate::telemetry::sha256_hex(occurrence_key.as_bytes()),
            "minutes": minutes,
        }),
    );
    finish_one(&app, &occurrence_key);
}

/// Dismiss ONE occurrence (when `occurrence_key` is supplied by a card button) or
/// ALL active alarms (Esc and the zero-card safety Dismiss pass nothing).
/// Dismiss-all stays the blunt "get everything off my screen" escape hatch.
#[tauri::command]
pub fn dismiss_alarms(app: tauri::AppHandle, occurrence_key: Option<String>) {
    match occurrence_key {
        Some(key) => {
            crate::telemetry::record(
                "alarm_dismissed",
                serde_json::json!({
                    "occurrence_hash": crate::telemetry::sha256_hex(key.as_bytes()),
                    "scope": "one",
                }),
            );
            finish_one(&app, &key);
        }
        None => {
            crate::telemetry::record("alarm_dismissed", serde_json::json!({ "scope": "all" }));
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
    fn looks_like_wake_detects_sleep_not_jitter() {
        // Normal cadence / App Nap jitter must NOT look like a wake…
        assert!(!looks_like_wake(30, 30));
        assert!(!looks_like_wake(30, 45));
        assert!(!looks_like_wake(30, 89), "just under the 60s margin");
        // …a real multi-minute/hour sleep must.
        assert!(looks_like_wake(30, 120));
        assert!(looks_like_wake(30, 3600));
        assert!(looks_like_wake(1, 600));
    }

    /// The overlay self-heal (`tick`'s sleep/wake re-assert) is the one place that
    /// reads AND is raced against the ACTIVE_ALARMS global, so its tests must own
    /// that static exclusively. Cargo runs tests in parallel threads inside ONE
    /// process — without this they'd leak phantom cards into each other. Clears on
    /// acquire AND on drop so neither an ordering nor a panicking test can strand a
    /// card in the global.
    mod overlay_self_heal {
        use super::*;

        static TEST_LOCK: Mutex<()> = Mutex::new(());

        struct Exclusive(
            // Held for the lifetime of the guard; never read.
            #[allow(dead_code)] std::sync::MutexGuard<'static, ()>,
        );
        impl Drop for Exclusive {
            fn drop(&mut self) {
                lock_resilient(&ACTIVE_ALARMS).clear();
            }
        }
        fn exclusive() -> Exclusive {
            let g = TEST_LOCK.lock().unwrap_or_else(|e| e.into_inner());
            lock_resilient(&ACTIVE_ALARMS).clear();
            Exclusive(g)
        }

        fn card(key: &str) -> serde_json::Value {
            serde_json::json!({"occurrence_key": key, "kind": "t_zero", "title": "Standup"})
        }

        #[test]
        fn dispatches_a_reassert_only_when_an_alarm_is_presented() {
            // IFF a fired alarm is still on screen. Empty set → no-op (an overlay
            // must never pop out of nowhere); any presented card → re-assert, so a
            // panel dropped across sleep comes back instead of leaving an audible,
            // un-dismissable looping alarm.
            assert!(!should_dispatch_reassert(&[]), "nothing presented → no re-assert");
            assert!(
                should_dispatch_reassert(&[card("(A @ t)")]),
                "a presented alarm → re-assert"
            );
        }

        #[test]
        fn a_dismiss_between_dispatch_and_the_main_thread_cancels_the_reassert() {
            // THE regression this guards: the dispatch decision happens on the
            // scheduler thread, the show runs later on the main thread. A dismiss
            // landing in that window clears ACTIVE_ALARMS and closes the panels —
            // if the queued closure still ran, show_overlays would find no live
            // panels, BUILD FRESH ONES with zero cards, and restart the sound loop.
            // That is an audible alarm the user just dismissed: the exact failure
            // this self-heal exists to prevent.
            let _x = exclusive();
            lock_resilient(&ACTIVE_ALARMS).push(card("(A @ t)"));
            assert!(
                should_dispatch_reassert(&lock_resilient(&ACTIVE_ALARMS)),
                "scheduler thread sees a presented alarm and dispatches"
            );

            // …user hits Dismiss before the main thread drains the queue.
            lock_resilient(&ACTIVE_ALARMS).clear();

            assert!(
                !reassert_still_warranted(),
                "a just-dismissed overlay must NOT be resurrected by an in-flight re-assert"
            );
        }

        #[test]
        fn a_reassert_still_fires_when_the_alarm_outlives_the_dispatch() {
            // The other half: nothing changed in the gap, so the queued closure
            // must still act — otherwise the self-heal never heals anything.
            let _x = exclusive();
            lock_resilient(&ACTIVE_ALARMS).push(card("(A @ t)"));
            assert!(reassert_still_warranted(), "alarm still presented → go ahead and re-assert");
        }

        #[test]
        fn snoozing_the_last_card_also_stops_the_reassert() {
            // Snooze/ignore route through the same ACTIVE_ALARMS removal as dismiss
            // (finish_one), so they must equally stop the self-heal from bringing a
            // snoozed alarm back on the next tick.
            let _x = exclusive();
            *lock_resilient(&ACTIVE_ALARMS) = vec![card("(A @ t)"), card("(B @ t)")];

            let remaining = retain_other_occurrences(&lock_resilient(&ACTIVE_ALARMS), "(A @ t)");
            *lock_resilient(&ACTIVE_ALARMS) = remaining;
            assert!(
                reassert_still_warranted(),
                "B is still on screen — the overlay must keep being re-asserted"
            );

            let remaining = retain_other_occurrences(&lock_resilient(&ACTIVE_ALARMS), "(B @ t)");
            *lock_resilient(&ACTIVE_ALARMS) = remaining;
            assert!(
                !reassert_still_warranted(),
                "last card snoozed → overlay closes and stays closed"
            );
        }

        #[test]
        fn a_reasserted_overlay_repulls_its_cards_from_active_alarms() {
            // Pins the claim the self-heal rests on: when the panels were LOST
            // (not just hidden), show_overlays rebuilds them and the fresh webview
            // pulls the card set via get_active_alarms on mount. If a re-assert
            // ever cleared or bypassed ACTIVE_ALARMS, the user would get a blank
            // takeover with a beating sound and nothing to dismiss.
            let _x = exclusive();
            let (a, b) = (card("(A @ t)"), card("(B @ t)"));
            *lock_resilient(&ACTIVE_ALARMS) = vec![a.clone(), b.clone()];
            assert_eq!(
                get_active_alarms(),
                vec![a, b],
                "a rebuilt panel must find every presented card still there"
            );
        }
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
        // Two overlapping meetings on screen, meeting A with both a reminder and a
        // T-0 card. Dismissing A must drop BOTH A cards and leave B untouched.
        let active = vec![
            serde_json::json!({"occurrence_key": "(A @ t)", "kind": "reminder_5"}),
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
            serde_json::json!({"occurrence_key": "(A @ t)", "kind": "reminder_5"}),
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

    #[test]
    fn get_access_state_default_shape_is_ok_with_empty_reason() {
        // Contract test for the Settings / tray-popover banner. The UI does
        // `s?.reason ?? ""` — the "reason" key MUST be present (not just absent)
        // so the null-coalescing works correctly in both the ok and lost paths.
        // ACCESS_TRACKER starts announced-Ok (see AccessTracker::new), so a
        // freshly-started process always returns { state: "ok", reason: "" }.
        // We test the SHAPE here (key presence + value), not a Lost transition
        // (the pure AccessTracker machine is exhaustively tested in access.rs).
        let state = get_access_state();
        assert_eq!(
            state.get("state").and_then(|v| v.as_str()),
            Some("ok"),
            "default state must be 'ok'"
        );
        assert!(
            state.get("reason").is_some(),
            "\"reason\" key must always be present — the UI does `s?.reason ?? \"\"`"
        );
        assert_eq!(
            state.get("reason").and_then(|v| v.as_str()),
            Some(""),
            "reason must be \"\" when state is ok"
        );
    }

    // The settings→timing seam: persisted `reminders` (minutes) must become
    // AlarmConfig offsets (seconds), and drive real firing end to end. This is the
    // one seam the alarm_core + settings unit tests can't reach on their own.
    mod settings_to_alarms {
        use super::*;
        use crate::alarm_core::{compute_actions, AlarmEvent, AlarmKind, AlarmState};
        use crate::settings::Settings;
        use chrono::{DateTime, Duration, Utc};

        fn base() -> DateTime<Utc> {
            DateTime::parse_from_rfc3339("2026-07-06T16:00:00Z").unwrap().with_timezone(&Utc)
        }

        fn event_starting_in(secs: i64) -> AlarmEvent {
            let start = base() + Duration::seconds(secs);
            AlarmEvent {
                occurrence_key: "(m @ t)".into(),
                title: "Meeting".into(),
                start,
                end: start + Duration::minutes(30),
                all_day: false,
                status: "confirmed".into(),
                my_rsvp: Some("accepted".into()),
                has_link: true,
            }
        }

        #[test]
        fn reminder_minutes_become_offset_seconds() {
            let cfg = build_alarm_config(&Settings {
                reminders: vec![20, 5],
                ..Settings::default()
            });
            assert_eq!(cfg.reminder_offsets_secs, vec![1200, 300], "minutes × 60");
        }

        #[test]
        fn multiple_reminders_plus_mandatory_start_fire_end_to_end() {
            // A meeting 30 min out with reminders at 20 and 5 min before.
            let cfg = build_alarm_config(&Settings {
                reminders: vec![20, 5],
                ..Settings::default()
            });
            let events = vec![event_starting_in(30 * 60)];
            let mut state = AlarmState::default();

            // T-20 (10 min in): the 20-min reminder is due.
            let at_20 = compute_actions(&events, base() + Duration::minutes(10), &state, &cfg);
            assert_eq!(at_20.len(), 1);
            assert_eq!(at_20[0].kind, AlarmKind::Reminder(1200));
            state.mark_fired("(m @ t)", AlarmKind::Reminder(1200), base());

            // T-5 (25 min in): the 5-min reminder is due, independently of the first.
            let at_5 = compute_actions(&events, base() + Duration::minutes(25), &state, &cfg);
            assert_eq!(at_5[0].kind, AlarmKind::Reminder(300));
            state.mark_fired("(m @ t)", AlarmKind::Reminder(300), base());

            // Start: the mandatory T-0 fires.
            let at_0 = compute_actions(&events, base() + Duration::minutes(30), &state, &cfg);
            assert_eq!(at_0[0].kind, AlarmKind::TZero);
        }

        #[test]
        fn zero_reminders_still_fires_only_the_mandatory_start() {
            // User removed every pre-event reminder.
            let cfg = build_alarm_config(&Settings { reminders: vec![], ..Settings::default() });
            let events = vec![event_starting_in(5 * 60)];
            let state = AlarmState::default();

            // No pre-event reminder ever fires…
            assert!(compute_actions(&events, base() + Duration::minutes(4), &state, &cfg).is_empty());
            // …but the start alarm is mandatory and DOES fire.
            let at_0 = compute_actions(&events, base() + Duration::minutes(5), &state, &cfg);
            assert_eq!(at_0.len(), 1);
            assert_eq!(at_0[0].kind, AlarmKind::TZero);
        }
    }
}

use tauri::Manager;
