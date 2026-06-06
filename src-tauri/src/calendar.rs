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
    Ok(events.iter().map(event_dto).collect())
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
    let events = mgr
        .fetch_events(now - Duration::days(7), now + Duration::days(7), None)
        .map(|es| es.iter().map(event_dto).collect::<Vec<_>>())
        .unwrap_or_default();

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
        "event_count": events.len(),
        "expanded_series_proof": expanded_series,
        "events": events,
    })
}
