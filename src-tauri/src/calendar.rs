//! EventKit access (Phase 1a spike → grows into the production calendar layer).
//!
//! CP1a proofs this module must deliver (PLAN §2):
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
    /// fired-set / snooze / dedup everywhere (PLAN §1).
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
    /// declined never alerts (PLAN §0).
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

#[tauri::command]
pub fn request_calendar_access() -> Result<bool, String> {
    let mgr = EventsManager::new();
    mgr.request_access().map_err(|e| e.to_string())
}

#[tauri::command]
pub fn list_calendars() -> Result<Vec<CalendarDto>, String> {
    let mgr = EventsManager::new();
    mgr.ensure_authorized().map_err(|e| e.to_string())?;
    Ok(mgr
        .list_calendars()
        .map_err(|e| e.to_string())?
        .iter()
        .map(calendar_dto)
        .collect())
}

#[tauri::command]
pub fn fetch_events(days_back: i64, days_forward: i64) -> Result<Vec<EventDto>, String> {
    let mgr = EventsManager::new();
    mgr.ensure_authorized().map_err(|e| e.to_string())?;
    let now = Local::now();
    let events = mgr
        .fetch_events(
            now - Duration::days(days_back),
            now + Duration::days(days_forward),
            None,
        )
        .map_err(|e| e.to_string())?;
    Ok(dedup_events(events.iter().map(event_dto).collect()))
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

    // First run: this triggers the TCC prompt (bundle identity matters — PLAN CP1a).
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

    // Occurrence-identity proof (PLAN CP1a): group recurring occurrences by event id —
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
