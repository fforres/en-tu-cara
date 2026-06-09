//! EventKit access (Phase 1a spike → grows into the production calendar layer).
//!
//! What this module must deliver:
//!   1. Permission flow works from THIS bundle id and persists across relaunch.
//!   2. Recurring series come back EXPANDED: N rows, distinct occurrence starts,
//!      a usable composite key (identifier, occurrence_start).
//!   3. RSVP/organizer/canceled fields are populated.
//!
//! Spike automation: launching with ENTUCARA_SPIKE_DUMP=1 writes a JSON dump of
//! calendars + ±7 days of events to the app data dir and stdout, so
//! cp1a-auto.sh can validate the schema without driving the UI.

#![cfg(target_os = "macos")]

use chrono::{Duration, Local};
use eventkit::{AuthorizationStatus, CalendarInfo, EventItem, EventsManager};
use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CalendarDto {
    pub id: String,
    pub title: String,
    /// Account/source name (e.g. "iCloud", "felipe@skyward.ai") — tray groups by this.
    pub account: Option<String>,
    /// RGBA 0.0–1.0 — the color bar in the tray rows.
    pub color: Option<(f64, f64, f64, f64)>,
    pub calendar_type: String,
    pub is_subscribed: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct EventDto {
    /// EKEvent identifier — NOT unique per occurrence; see `occurrence_key`.
    pub id: String,
    /// Composite key "(id @ occurrence_start_rfc3339)" — THE identity used by
    /// fired-set / snooze / dedup everywhere.
    pub occurrence_key: String,
    pub title: String,
    pub start: String,
    pub end: String,
    pub all_day: bool,
    pub calendar_id: Option<String>,
    pub calendar_title: Option<String>,
    pub notes: Option<String>,
    pub location: Option<String>,
    pub url: Option<String>,
    /// Per-occurrence original date — present on recurring-series occurrences.
    pub occurrence_date: Option<String>,
    pub is_recurring_occurrence: bool,
    pub is_detached: bool,
    /// confirmed | tentative | canceled | none — canceled never alerts.
    pub status: String,
    /// Current user's RSVP: accepted | declined | tentative | pending | unknown…
    /// declined never alerts.
    pub my_rsvp: Option<String>,
    pub is_organizer: bool,
    pub availability: String,
    pub timezone: Option<String>,
    pub attendee_count: usize,
}

fn calendar_dto(c: &CalendarInfo) -> CalendarDto {
    CalendarDto {
        id: c.identifier.clone(),
        title: c.title.clone(),
        account: c.source.clone(),
        color: c.color,
        calendar_type: format!("{:?}", c.calendar_type),
        is_subscribed: c.is_subscribed,
    }
}

fn event_dto(e: &EventItem) -> EventDto {
    let start_rfc = e.start_date.to_rfc3339();
    let my_rsvp = e
        .attendees
        .iter()
        .find(|a| a.is_current_user)
        .map(|a| format!("{:?}", a.status).to_lowercase());
    let is_organizer = e
        .organizer
        .as_ref()
        .is_some_and(|o| o.is_current_user);
    EventDto {
        occurrence_key: format!("({} @ {})", e.identifier, start_rfc),
        id: e.identifier.clone(),
        title: e.title.clone(),
        start: start_rfc,
        end: e.end_date.to_rfc3339(),
        all_day: e.all_day,
        calendar_id: e.calendar_id.clone(),
        calendar_title: e.calendar_title.clone(),
        notes: e.notes.clone(),
        location: e.location.clone(),
        url: e.URL.clone(),
        occurrence_date: e.occurrence_date.map(|d| d.to_rfc3339()),
        is_recurring_occurrence: e.occurrence_date.is_some(),
        is_detached: e.is_detached,
        status: format!("{:?}", e.status).to_lowercase(),
        my_rsvp,
        is_organizer,
        availability: format!("{:?}", e.availability).to_lowercase(),
        timezone: e.timezone.clone(),
        attendee_count: e.attendees.len(),
    }
}

/// Collapse multi-calendar duplicates of the same real-world meeting.
///
/// CP1a finding (2026-06-05): the same Google event surfaces once per calendar that
/// can see it (subscribed colleague calendars, mirrored Gmail accounts) with an
/// IDENTICAL `(identifier @ start)` key — 45 of 209 events duplicated on real data.
/// One meeting must alert once, so we dedup by occurrence_key and keep the row most
/// likely to be the user's own copy: has my_rsvp (their attendance) > is_organizer >
/// first seen. The tray shows the deduped list too (reference popover has one row
/// per meeting).
pub fn dedup_events(events: Vec<EventDto>) -> Vec<EventDto> {
    let mut best: std::collections::HashMap<String, EventDto> = Default::default();
    let mut order: Vec<String> = Vec::new();
    let score = |e: &EventDto| (e.my_rsvp.is_some() as u8) * 2 + e.is_organizer as u8;
    for e in events {
        match best.get(&e.occurrence_key) {
            None => {
                order.push(e.occurrence_key.clone());
                best.insert(e.occurrence_key.clone(), e);
            }
            Some(existing) if score(&e) > score(existing) => {
                best.insert(e.occurrence_key.clone(), e);
            }
            _ => {}
        }
    }
    order.into_iter().filter_map(|k| best.remove(&k)).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ev(key: &str, rsvp: Option<&str>, organizer: bool) -> EventDto {
        EventDto {
            id: "id".into(),
            occurrence_key: key.into(),
            title: "t".into(),
            start: "2026-06-08T09:00:00-07:00".into(),
            end: "2026-06-08T09:15:00-07:00".into(),
            all_day: false,
            calendar_id: None,
            calendar_title: None,
            notes: None,
            location: None,
            url: None,
            occurrence_date: None,
            is_recurring_occurrence: false,
            is_detached: false,
            status: "confirmed".into(),
            my_rsvp: rsvp.map(String::from),
            is_organizer: organizer,
            availability: "busy".into(),
            timezone: None,
            attendee_count: 0,
        }
    }

    #[test]
    fn has_meeting_link_detects_known_hosts_across_all_fields() {
        // Drives the only_video_events alarm policy — a regression here silently
        // changes which meetings fire. Covers each scanned field + a few hosts.
        assert!(has_meeting_link(Some("https://us04web.zoom.us/j/123"), None, None));
        assert!(has_meeting_link(None, Some("meet.google.com/abc-defg-hij"), None));
        assert!(has_meeting_link(None, None, Some("Join: https://teams.microsoft.com/l/x")));
        // Generic meet./call. subdomain heuristic.
        assert!(has_meeting_link(Some("https://meet.example.com/room"), None, None));
        // No conferencing link anywhere → false (a phone-only / in-person event).
        assert!(!has_meeting_link(
            Some("https://docs.google.com/document/d/1"),
            Some("Room 4B"),
            Some("agenda attached")
        ));
        assert!(!has_meeting_link(None, None, None));
    }

    #[test]
    fn guard_eventkit_contains_a_panic_as_err() {
        // The whole point: an EventKit NULL-panic must become an Err, never an
        // unwind into the scheduler tick or a Tauri command handler.
        let ok: Result<i32, String> = guard_eventkit("x", || Ok::<_, String>(5));
        assert_eq!(ok, Ok(5));
        let err: Result<i32, String> = guard_eventkit("x", || Err::<i32, String>("nope".into()));
        assert_eq!(err, Err("nope".to_string()));
        let panicked: Result<i32, String> =
            guard_eventkit("probe", || -> Result<i32, String> { panic!("unexpected NULL") });
        assert!(panicked.is_err());
        assert!(panicked.unwrap_err().contains("probe"));
    }

    #[test]
    fn calendar_enabled_governs_what_the_user_sees() {
        // None enabled-set = every calendar shows (the default).
        assert!(calendar_enabled(&None, Some("work")));
        assert!(calendar_enabled(&None, None));
        // An explicit set includes only listed calendars…
        let enabled = Some(vec!["work".to_string(), "personal".to_string()]);
        assert!(calendar_enabled(&enabled, Some("work")));
        assert!(
            !calendar_enabled(&enabled, Some("buffer")),
            "a disabled calendar's events must be filtered out (the popover bug)"
        );
        // …but an event with no calendar id is never silently dropped.
        assert!(calendar_enabled(&enabled, None));
    }

    #[test]
    fn dedup_prefers_row_with_my_rsvp() {
        // CP1a real-data case: same meeting via a colleague's subscribed calendar
        // (no rsvp) and via the user's own calendar (rsvp present).
        let deduped = dedup_events(vec![
            ev("(a @ t1)", None, false),
            ev("(a @ t1)", Some("accepted"), false),
            ev("(b @ t1)", None, true),
        ]);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].my_rsvp.as_deref(), Some("accepted"));
        assert!(deduped[1].is_organizer);
    }

    #[test]
    fn dedup_keeps_first_seen_order_and_distinct_occurrences() {
        let deduped = dedup_events(vec![
            ev("(a @ t1)", None, false),
            ev("(a @ t2)", None, false), // same series, different occurrence — kept
            ev("(a @ t1)", None, false), // duplicate — dropped
        ]);
        assert_eq!(deduped.len(), 2);
        assert_eq!(deduped[0].occurrence_key, "(a @ t1)");
        assert_eq!(deduped[1].occurrence_key, "(a @ t2)");
    }
}

/// Ask the system to refresh calendar sources (Google/iCloud/Exchange sync pull).
/// Called once per poll cycle. NOTE the honest physics: this nudges the macOS
/// sync daemon, but the actual server-fetch cadence for CalDAV accounts is the
/// per-account "Refresh Calendars" interval (Calendar.app ▸ Settings ▸ Accounts;
/// minimum "Every minute" is NOT offered — 5 min is the floor, 15 the default).
/// Documented in README + settings description.
pub fn sync_event_store() {
    use objc2::rc::Retained;
    use objc2_event_kit::EKEventStore;
    // EKEventStore is !Send. A thread_local gives each CALLING thread its own
    // instance (the scheduler tick AND the main-thread fetch_events command) —
    // never shared across threads, so the !Send invariant holds on both paths.
    thread_local! {
        static STORE: Retained<EKEventStore> = unsafe { EKEventStore::new() };
    }
    STORE.with(|store| unsafe {
        // Make a read a SYNC, not a stale pull (bug: an event deleted/edited in
        // Google/Calendar.app while we're running kept re-appearing in the tray
        // + alerts):
        //   1. refreshSourcesIfNecessary asks remote accounts (CalDAV/Exchange/
        //      Google) to push their latest down into the local store.
        //   2. reset drops THIS process's cached EKEvent objects so the next
        //      eventsMatchingPredicate re-reads the persistent store. Without it
        //      a long-running agent keeps serving the snapshot it cached on
        //      first access — Calendar.app looks correct (it resets on its own
        //      EKEventStoreChanged notifications) while we lag indefinitely.
        // We never write events (read-only agent), so reset discarding unsaved
        // changes is a no-op for us.
        store.refreshSourcesIfNecessary();
        store.reset();
    });
}

/// Cheap video-link presence check for the alarm policy (only_video_events).
/// The TS extractor (meeting-links.ts) remains canonical for display/Join.
pub fn has_meeting_link(url: Option<&str>, location: Option<&str>, notes: Option<&str>) -> bool {
    const HOSTS: [&str; 10] = [
        "zoom.us", "zoomgov.com", "meet.google.com", "teams.microsoft.com",
        "teams.live.com", "webex.com", "meet.jit.si", "whereby.com", "around.co",
        "discord.gg",
    ];
    [url, location, notes].iter().flatten().any(|field| {
        HOSTS.iter().any(|h| field.contains(h))
            || field.contains("https://meet.")
            || field.contains("https://call.")
    })
}

#[tauri::command]
pub fn calendar_authorization_status() -> String {
    format!("{:?}", EventsManager::authorization_status())
}

/// Open System Settings → Privacy & Security → Calendars.
fn open_calendar_privacy_settings() {
    let _ = std::process::Command::new("open")
        .arg("x-apple.systempreferences:com.apple.preference.security?Privacy_Calendars")
        .spawn();
}

#[tauri::command]
pub fn request_calendar_access(app: tauri::AppHandle) -> Result<bool, String> {
    let status = EventsManager::authorization_status();
    log::info!("request_calendar_access: status={status:?}");
    match status {
        AuthorizationStatus::FullAccess => Ok(true),
        // macOS only shows the system prompt from NotDetermined. Once the user
        // is Denied/Restricted, requestAccess is a SILENT no-op — which is why
        // the in-app "Grant calendar access" button looked like it did nothing.
        // Deep-link to System Settings (the only place to re-enable) instead.
        AuthorizationStatus::Denied | AuthorizationStatus::Restricted => {
            log::info!("request_calendar_access: denied/restricted → opening System Settings");
            open_calendar_privacy_settings();
            Ok(false)
        }
        // NotDetermined: show the real prompt — but OFF the main thread. This
        // command runs on the main thread (Tauri IPC); request_access blocks on
        // the EventKit completion which needs the main run loop, so calling it
        // here directly DEADLOCKS the UI (beach ball). Spawn it.
        _ => {
            log::info!("request_calendar_access: NotDetermined → prompting off-main");
            prompt_access_off_main(app);
            Ok(false)
        }
    }
}

/// Show the system calendar prompt OFF the main thread and, on grant, relaunch.
///
/// Off-main is mandatory: `request_access` blocks on an EventKit completion that
/// needs the main run loop, so calling it on the main thread (Tauri IPC) beach-
/// balls the UI. Relaunch-on-grant is also mandatory: EventKit caches the auth
/// status in the GRANTING process, so this process keeps reading "not authorized"
/// until a fresh one starts (verified: events only appeared after a restart).
fn prompt_access_off_main(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        let mgr = EventsManager::new();
        match mgr.request_access() {
            Ok(true) => {
                log::info!("calendar access granted → relaunching to apply");
                app.restart();
            }
            Ok(false) => log::info!("calendar prompt denied"),
            Err(e) => log::warn!("calendar prompt error: {e}"),
        }
    });
}

/// Startup pre-flight so a RETURNING user never has to hunt for the "Grant
/// calendar access" button. The common trigger: a rebuild re-signs the app
/// ad-hoc with a NEW code identity, so macOS/TCC treats it as a different app
/// and resets the grant to NotDetermined — even though the bundle id is
/// unchanged. (A stable Developer ID signature would make the grant survive
/// rebuilds; until then this papers over it.)
///
/// NotDetermined → auto-prompt (the same off-main + relaunch-on-grant flow as
/// the button). Denied is left ALONE: re-opening System Settings on every launch
/// would nag; the in-tray button still routes there on demand. FullAccess is
/// already good. The caller gates this on `onboarded` so a brand-new user is
/// driven by onboarding (which requests access with context) instead.
pub fn preflight_calendar_access(app: tauri::AppHandle) {
    std::thread::spawn(move || {
        // Grace so the main run loop (which request_access's completion needs)
        // is pumping before we prompt — mirrors the scheduler's startup grace.
        std::thread::sleep(std::time::Duration::from_secs(2));
        let status = EventsManager::authorization_status();
        log::info!("preflight calendar access: status={status:?}");
        if matches!(status, AuthorizationStatus::NotDetermined) {
            log::info!("preflight: NotDetermined → auto-prompting (no button needed)");
            prompt_access_off_main(app);
        }
    });
}

/// Run an EventKit query that may PANIC rather than error. `eventkit-rs` /
/// `objc2-event-kit` panic when an EventKit call returns NULL — which happens
/// when the process isn't truly calendar-authorized even though
/// `authorization_status` reports access (notably a bare `tauri dev` binary
/// whose code identity holds no TCC grant — see gotcha #5). Containing the
/// unwind here is load-bearing: this same code runs both on the scheduler tick
/// AND, via `invoke`, on the Tauri command thread for the tray popover — an
/// unguarded panic there tears down the popover (and starves the UI) instead of
/// degrading to "no events".
fn guard_eventkit<T, E: std::fmt::Display>(
    what: &str,
    f: impl FnOnce() -> Result<T, E>,
) -> Result<T, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(Ok(v)) => Ok(v),
        Ok(Err(e)) => Err(e.to_string()),
        Err(_) => Err(format!(
            "{what}: EventKit returned no data — calendar access is unavailable to this process"
        )),
    }
}

/// Can we READ events RIGHT NOW — checked WITHOUT requesting access.
///
/// CRITICAL: the read commands (fetch_events / list_calendars) run on the MAIN
/// thread via Tauri's WebKit IPC. `ensure_authorized()` auto-calls the BLOCKING
/// `request_access()` when status is NotDetermined; that blocks the main thread
/// on the EventKit completion, which itself needs the main run loop → DEADLOCK /
/// beach ball (confirmed via `sample`: main wedged in request_access →
/// pthread_cond_wait). So reads must only QUERY status here and never request.
/// The user grants via the explicit `request_calendar_access` command, which
/// runs the prompt off-main.
fn authorized_to_read() -> bool {
    matches!(EventsManager::authorization_status(), AuthorizationStatus::FullAccess)
}

#[tauri::command]
pub fn list_calendars() -> Result<Vec<CalendarDto>, String> {
    let t0 = std::time::Instant::now();
    log::debug!("list_calendars: enter");
    if !authorized_to_read() {
        log::warn!("list_calendars: not authorized (no request — avoids main-thread deadlock)");
        return Err("calendar access not granted".to_string());
    }
    let mgr = EventsManager::new();
    let calendars = guard_eventkit("list_calendars", || mgr.list_calendars());
    match &calendars {
        Ok(c) => log::debug!("list_calendars: {} calendars in {}ms", c.len(), t0.elapsed().as_millis()),
        Err(e) => log::warn!("list_calendars: failed in {}ms: {e}", t0.elapsed().as_millis()),
    }
    Ok(calendars?.iter().map(calendar_dto).collect())
}

/// Best-effort, non-reversible identifier for the user's calendar "org": the
/// sha256 of the first calendar account that looks like an email (its `source`,
/// e.g. "felipe@skyward.ai"). Used ONLY as a coarse telemetry grouping key — we
/// never send the raw email. Returns None when access isn't granted yet or no
/// account carries an email (self-heals on a later launch once access exists).
pub fn primary_account_hash() -> Option<String> {
    let calendars = list_calendars().ok()?;
    let email = calendars
        .into_iter()
        .filter_map(|c| c.account)
        .find(|a| a.contains('@'))?;
    Some(crate::telemetry::sha256_hex(email.trim().to_ascii_lowercase().as_bytes()))
}

#[tauri::command]
pub fn fetch_events(days_back: i64, days_forward: i64) -> Result<Vec<EventDto>, String> {
    let t0 = std::time::Instant::now();
    log::debug!("fetch_events: enter back={days_back} fwd={days_forward}");
    if !authorized_to_read() {
        log::warn!("fetch_events: not authorized (no request — avoids main-thread deadlock)");
        return Err("calendar access not granted".to_string());
    }
    // Sync before reading so externally deleted/edited events don't linger.
    // EVERY event read (popover and scheduler) goes through here via
    // `active_events`, so this is the one sync point — callers must not sync
    // again. Cheap: refreshSourcesIfNecessary is a no-op when nothing changed,
    // reset is local.
    sync_event_store();
    let mgr = EventsManager::new();
    let now = Local::now();
    let events = guard_eventkit("fetch_events", || {
        mgr.fetch_events(now - Duration::days(days_back), now + Duration::days(days_forward), None)
    });
    match &events {
        Ok(e) => log::debug!("fetch_events: {} raw events in {}ms", e.len(), t0.elapsed().as_millis()),
        Err(e) => log::warn!("fetch_events: failed in {}ms: {e}", t0.elapsed().as_millis()),
    }
    Ok(dedup_events(events?.iter().map(event_dto).collect()))
}

/// Is an event's calendar enabled for alerts/listing? `None` enabled-set means
/// "all calendars"; an event with no calendar id is never silently dropped. Pure
/// so the one rule that decides what the user sees is unit-tested.
pub fn calendar_enabled(enabled: &Option<Vec<String>>, calendar_id: Option<&str>) -> bool {
    match (enabled, calendar_id) {
        (Some(enabled), Some(cal)) => enabled.iter().any(|c| c == cal),
        (Some(_), None) => true,
        (None, _) => true,
    }
}

/// THE canonical event read for the whole app: `fetch_events` (sync + dedup) then
/// drop anything from a calendar the user disabled. The tray popover, the
/// menu-bar title, and the alarm scheduler all read through here — so the list
/// you see, the "next event" countdown, and what can fire an alert can never
/// disagree about which events exist. Disabling a calendar takes effect the next
/// time any of them reads (e.g. reopening the tray).
pub fn active_events(
    enabled_calendar_ids: &Option<Vec<String>>,
    days_back: i64,
    days_forward: i64,
) -> Result<Vec<EventDto>, String> {
    Ok(fetch_events(days_back, days_forward)?
        .into_iter()
        .filter(|e| calendar_enabled(enabled_calendar_ids, e.calendar_id.as_deref()))
        .collect())
}

/// Real-pipeline e2e (user-authorized fast test): create a REAL EventKit event
/// starting +`start_in`s on a dedicated "En Tu Cara Test" calendar, let the
/// production loop discover it (EventKit → dedup → alarm → overlay), then clean
/// up both event and calendar. Enabled via ENTUCARA_SPIKE_REAL_E2E="<start_in>".
pub fn maybe_run_real_e2e() {
    let Ok(start_in) = std::env::var("ENTUCARA_SPIKE_REAL_E2E") else {
        return;
    };
    let start_in: i64 = start_in.parse().unwrap_or(70);
    std::thread::spawn(move || {
        let mgr = EventsManager::new();
        if mgr.ensure_authorized().is_err() {
            eprintln!("REAL_E2E: not authorized");
            return;
        }
        // Prefer a dedicated local test calendar; many default sources (Google)
        // refuse calendar creation (EKErrorDomain 17) → fall back to the default
        // calendar. The event auto-deletes either way.
        let test_cal = mgr.create_event_calendar("En Tu Cara Test").ok();
        let cal_title = test_cal.as_ref().map(|_| "En Tu Cara Test");

        let start = Local::now() + Duration::seconds(start_in);
        let draft = eventkit::EventDraft {
            title: "En Tu Cara REAL E2E (auto-deletes)",
            start: Some(start),
            end: Some(start + Duration::seconds(90)),
            notes: Some("Join: https://us04web.zoom.us/j/000000000?pwd=e2e"),
            calendar_title: cal_title,
            ..Default::default()
        };
        match mgr.create_event(&draft) {
            Ok(event) => {
                println!("REAL_E2E created: {} start={}", event.identifier, start.to_rfc3339());
                // Leave it alive through T-0 + margin, then clean up.
                let wait = (start_in + 120).max(0) as u64;
                std::thread::sleep(std::time::Duration::from_secs(wait));
                let _ = mgr.delete_event(&event.identifier, false);
                if let Some(cal) = &test_cal {
                    let _ = mgr.delete_event_calendar(&cal.identifier);
                }
                println!("REAL_E2E cleaned up");
            }
            Err(e) => {
                eprintln!("REAL_E2E: create event failed: {e}");
                if let Some(cal) = &test_cal {
                    let _ = mgr.delete_event_calendar(&cal.identifier);
                }
            }
        }
    });
}

/// CP1a automation: dump auth status + calendars + ±7d events as JSON.
/// Invoked on startup when ENTUCARA_SPIKE_DUMP=1 (see lib.rs setup).
pub fn spike_dump() -> serde_json::Value {
    let status = format!("{:?}", EventsManager::authorization_status());
    let mgr = EventsManager::new();

    // First run: this triggers the TCC prompt (bundle identity matters).
    let granted = if matches!(
        EventsManager::authorization_status(),
        AuthorizationStatus::NotDetermined
    ) {
        mgr.request_access().unwrap_or(false)
    } else {
        matches!(
            EventsManager::authorization_status(),
            AuthorizationStatus::FullAccess
        )
    };

    if !granted {
        return serde_json::json!({
            "auth_status_at_launch": status,
            "granted": false,
        });
    }

    let calendars = mgr
        .list_calendars()
        .map(|cs| cs.iter().map(calendar_dto).collect::<Vec<_>>())
        .unwrap_or_default();
    let now = Local::now();
    let raw_events = mgr
        .fetch_events(now - Duration::days(7), now + Duration::days(7), None)
        .map(|es| es.iter().map(event_dto).collect::<Vec<_>>())
        .unwrap_or_default();
    let raw_count = raw_events.len();
    let events = dedup_events(raw_events);

    // Occurrence-identity proof: group recurring occurrences by event id —
    // any id with >1 row must have all-distinct starts.
    let mut by_id: std::collections::HashMap<&str, Vec<&str>> = Default::default();
    for e in events.iter().filter(|e| e.is_recurring_occurrence) {
        by_id.entry(&e.id).or_default().push(&e.start);
    }
    let expanded_series: Vec<_> = by_id
        .iter()
        .filter(|(_, starts)| starts.len() > 1)
        .map(|(id, starts)| {
            let distinct: std::collections::HashSet<_> = starts.iter().collect();
            serde_json::json!({
                "event_id": id,
                "occurrences": starts.len(),
                "distinct_starts": distinct.len(),
            })
        })
        .collect();

    serde_json::json!({
        "auth_status_at_launch": status,
        "granted": true,
        "calendar_count": calendars.len(),
        "calendars": calendars,
        "raw_event_count": raw_count,
        "event_count": events.len(),
        "deduped_away": raw_count - events.len(),
        "expanded_series_proof": expanded_series,
        "events": events,
    })
}
