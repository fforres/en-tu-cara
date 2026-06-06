//! Pure alarm decision core (PLAN §1): `compute_actions(events, now, state)`.
//!
//! Encodes EVERY policy as testable data, no clocks, no EventKit, no UI:
//!   - T-5m and T-0 alarms per event occurrence
//!   - declined / canceled / all-day events never alert
//!   - missed-while-asleep: fire on next tick if event still ongoing, skip if ended
//!   - event created inside the T-5 window → only T-0 (no stale T-5)
//!   - pause: decisions still advance fired-state, presentation suppressed
//!   - snoozes: persisted deadlines fire when due, survive restart
//!   - dedup via fired-set keyed by (occurrence_key, kind)

use chrono::{DateTime, Duration, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// Alarm policy knobs — sourced from Settings, injected per tick (pure core
/// stays clock-free AND config-free).
#[derive(Debug, Clone)]
pub struct AlarmConfig {
    pub lead_secs: i64,
    pub alert_tentative: bool,
    pub alert_pending: bool,
    pub only_video_events: bool,
}

impl Default for AlarmConfig {
    fn default() -> Self {
        Self { lead_secs: 5 * 60, alert_tentative: true, alert_pending: true, only_video_events: false }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AlarmEvent {
    pub occurrence_key: String,
    pub title: String,
    pub start: DateTime<Utc>,
    pub end: DateTime<Utc>,
    pub all_day: bool,
    /// confirmed | tentative | canceled | none
    pub status: String,
    /// accepted | declined | tentative | pending | … (None = no attendees / own event)
    pub my_rsvp: Option<String>,
    /// Event carries a video-conference link (Rust-side heuristic at ingestion).
    pub has_link: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Hash)]
#[serde(rename_all = "snake_case")]
pub enum AlarmKind {
    TMinus5,
    TZero,
    Snooze,
}

#[derive(Debug, Clone, PartialEq, Serialize)]
pub struct FireAction {
    pub occurrence_key: String,
    pub kind: AlarmKind,
    /// When this alarm was nominally due (for latency accounting).
    pub due_at: DateTime<Utc>,
    /// Presentation suppressed (paused) but fired-state still advances.
    pub suppressed: bool,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct AlarmState {
    /// (occurrence_key, kind) → fired-at. GC'd by `gc`.
    pub fired: HashMap<String, DateTime<Utc>>,
    /// occurrence_key → snooze deadline. Cleared when fired.
    pub snoozes: HashMap<String, DateTime<Utc>>,
    pub paused: bool,
}

fn fired_key(occurrence_key: &str, kind: AlarmKind) -> String {
    format!("{occurrence_key}#{kind:?}")
}

impl AlarmState {
    pub fn has_fired(&self, occurrence_key: &str, kind: AlarmKind) -> bool {
        self.fired.contains_key(&fired_key(occurrence_key, kind))
    }
    pub fn mark_fired(&mut self, occurrence_key: &str, kind: AlarmKind, at: DateTime<Utc>) {
        self.fired.insert(fired_key(occurrence_key, kind), at);
        if kind == AlarmKind::Snooze {
            self.snoozes.remove(occurrence_key);
        }
    }
    pub fn snooze(&mut self, occurrence_key: &str, until: DateTime<Utc>) {
        self.snoozes.insert(occurrence_key.to_string(), until);
        // Re-arming a snooze must allow it to fire again.
        self.fired.remove(&fired_key(occurrence_key, AlarmKind::Snooze));
    }
    /// Drop state for occurrences that ended > 48 h ago (PLAN §1 GC rule).
    /// Keys embed the occurrence; we GC by fired-at age as the proxy.
    pub fn gc(&mut self, now: DateTime<Utc>) {
        let cutoff = now - Duration::hours(48);
        self.fired.retain(|_, at| *at > cutoff);
        self.snoozes.retain(|_, until| *until > cutoff);
    }
}

fn alertable(event: &AlarmEvent, cfg: &AlarmConfig) -> bool {
    if event.all_day || event.status == "canceled" {
        return false;
    }
    if cfg.only_video_events && !event.has_link {
        return false;
    }
    match event.my_rsvp.as_deref() {
        Some("declined") => false,
        Some("tentative") => cfg.alert_tentative,
        Some("pending") => cfg.alert_pending,
        _ => true,
    }
}

/// THE decision function. Called on every scheduler tick. Mutates nothing —
/// the caller applies returned actions via `AlarmState::mark_fired`.
pub fn compute_actions(
    events: &[AlarmEvent],
    now: DateTime<Utc>,
    state: &AlarmState,
    cfg: &AlarmConfig,
) -> Vec<FireAction> {
    let mut actions = Vec::new();

    for event in events.iter().filter(|e| alertable(e, cfg)) {
        let t5_due = event.start - Duration::seconds(cfg.lead_secs);

        // T-5: due, not yet fired, and the MEETING HAS NOT STARTED — once the
        // event is under way a "starts in 5 minutes" alert is a lie; T-0 covers it.
        if now >= t5_due
            && now < event.start
            && !state.has_fired(&event.occurrence_key, AlarmKind::TMinus5)
        {
            actions.push(FireAction {
                occurrence_key: event.occurrence_key.clone(),
                kind: AlarmKind::TMinus5,
                due_at: t5_due,
                suppressed: state.paused,
            });
        }

        // T-0: due, not fired, event still ongoing (missed-while-asleep policy:
        // fire on the first tick after wake while ongoing; never after it ended).
        if now >= event.start
            && now < event.end
            && !state.has_fired(&event.occurrence_key, AlarmKind::TZero)
        {
            actions.push(FireAction {
                occurrence_key: event.occurrence_key.clone(),
                kind: AlarmKind::TZero,
                due_at: event.start,
                suppressed: state.paused,
            });
        }
    }

    // Snoozes: due and the event still ongoing.
    for (key, until) in &state.snoozes {
        if now >= *until && !state.has_fired(key, AlarmKind::Snooze) {
            if let Some(event) = events.iter().find(|e| &e.occurrence_key == key) {
                if now < event.end && alertable(event, cfg) {
                    actions.push(FireAction {
                        occurrence_key: key.clone(),
                        kind: AlarmKind::Snooze,
                        due_at: *until,
                        suppressed: state.paused,
                    });
                }
            }
        }
    }

    actions.sort_by_key(|a| a.due_at);
    actions
}

/// Next instant the scheduler must wake for (for wall-clock arming + the
/// windowed latencyCritical assertion). None = nothing pending in `events`.
pub fn next_due(
    events: &[AlarmEvent],
    now: DateTime<Utc>,
    state: &AlarmState,
    cfg: &AlarmConfig,
) -> Option<DateTime<Utc>> {
    let mut next: Option<DateTime<Utc>> = None;
    let mut consider = |t: DateTime<Utc>| {
        if t > now && next.is_none_or(|n| t < n) {
            next = Some(t);
        }
    };
    for e in events.iter().filter(|e| alertable(e, cfg)) {
        if !state.has_fired(&e.occurrence_key, AlarmKind::TMinus5) {
            consider(e.start - Duration::seconds(cfg.lead_secs));
        }
        if !state.has_fired(&e.occurrence_key, AlarmKind::TZero) {
            consider(e.start);
        }
    }
    for until in state.snoozes.values() {
        consider(*until);
    }
    next
}

#[cfg(test)]
mod tests {
    use super::*;

    fn t(secs_from_base: i64) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339("2026-06-08T16:00:00Z").unwrap().with_timezone(&Utc)
            + Duration::seconds(secs_from_base)
    }

    fn ev(key: &str, start_s: i64, end_s: i64) -> AlarmEvent {
        AlarmEvent {
            occurrence_key: key.into(),
            title: key.into(),
            start: t(start_s),
            end: t(end_s),
            all_day: false,
            status: "confirmed".into(),
            my_rsvp: Some("accepted".into()),
            has_link: true,
        }
    }

    fn cfg() -> AlarmConfig {
        AlarmConfig::default()
    }

    fn kinds(actions: &[FireAction]) -> Vec<(String, AlarmKind)> {
        actions.iter().map(|a| (a.occurrence_key.clone(), a.kind)).collect()
    }

    #[test]
    fn t5_then_t0_lifecycle() {
        let events = vec![ev("m", 600, 1500)]; // starts T+10min
        let mut state = AlarmState::default();

        assert!(compute_actions(&events, t(0), &state, &cfg()).is_empty(), "too early");

        let at_t5 = compute_actions(&events, t(300), &state, &cfg());
        assert_eq!(kinds(&at_t5), vec![("m".into(), AlarmKind::TMinus5)]);
        state.mark_fired("m", AlarmKind::TMinus5, t(300));

        assert!(compute_actions(&events, t(400), &state, &cfg()).is_empty(), "T-5 deduped");

        let at_t0 = compute_actions(&events, t(600), &state, &cfg());
        assert_eq!(kinds(&at_t0), vec![("m".into(), AlarmKind::TZero)]);
        state.mark_fired("m", AlarmKind::TZero, t(600));

        assert!(compute_actions(&events, t(700), &state, &cfg()).is_empty(), "all done");
    }

    #[test]
    fn declined_canceled_allday_never_fire() {
        let mut declined = ev("declined", 300, 900);
        declined.my_rsvp = Some("declined".into());
        let mut canceled = ev("canceled", 300, 900);
        canceled.status = "canceled".into();
        let mut allday = ev("allday", 300, 900);
        allday.all_day = true;
        let state = AlarmState::default();
        assert!(compute_actions(&[declined, canceled, allday], t(600), &state, &cfg()).is_empty());
    }

    #[test]
    fn created_inside_t5_window_gets_only_t0_at_start() {
        // Event appears in fetches 90s before start: T-5 fires immediately (late but
        // honest — "starts in 1.5m"), then T-0. If discovered AFTER start: only T-0.
        let events = vec![ev("late", 90, 990)];
        let state = AlarmState::default();
        let discovered_after_start = compute_actions(&events, t(120), &state, &cfg());
        assert_eq!(kinds(&discovered_after_start), vec![("late".into(), AlarmKind::TZero)]);
    }

    #[test]
    fn missed_while_asleep_fires_if_ongoing_skips_if_ended() {
        let events = vec![ev("during", 0, 1800), ev("ended", 0, 300)];
        let state = AlarmState::default();
        // Wake at T+600: "during" still ongoing → T-0 fires; "ended" → silence forever.
        let actions = compute_actions(&events, t(600), &state, &cfg());
        assert_eq!(kinds(&actions), vec![("during".into(), AlarmKind::TZero)]);
    }

    #[test]
    fn t5_suppressed_once_meeting_started() {
        let events = vec![ev("m", 0, 900)];
        let state = AlarmState::default();
        let actions = compute_actions(&events, t(60), &state, &cfg());
        // Only T-0 — a "starts in 5 min" banner after start would lie.
        assert_eq!(kinds(&actions), vec![("m".into(), AlarmKind::TZero)]);
    }

    #[test]
    fn pause_marks_suppressed_but_still_advances() {
        let events = vec![ev("m", 0, 900)];
        let mut state = AlarmState::default();
        state.paused = true;
        let actions = compute_actions(&events, t(10), &state, &cfg());
        assert_eq!(actions.len(), 1);
        assert!(actions[0].suppressed);
        // Caller marks fired even when suppressed → un-pause does NOT replay.
        state.mark_fired("m", AlarmKind::TZero, t(10));
        state.paused = false;
        assert!(compute_actions(&events, t(20), &state, &cfg()).is_empty());
    }

    #[test]
    fn snooze_fires_when_due_and_survives_restart() {
        let events = vec![ev("m", 0, 1800)];
        let mut state = AlarmState::default();
        state.mark_fired("m", AlarmKind::TZero, t(10));
        state.snooze("m", t(310)); // snooze 5m at T+10

        assert!(compute_actions(&events, t(200), &state, &cfg()).is_empty(), "not due yet");

        // Simulated restart: state round-trips through serde.
        let state: AlarmState =
            serde_json::from_str(&serde_json::to_string(&state).unwrap()).unwrap();

        let actions = compute_actions(&events, t(320), &state, &cfg());
        assert_eq!(kinds(&actions), vec![("m".into(), AlarmKind::Snooze)]);
    }

    #[test]
    fn snooze_past_event_end_never_fires() {
        let events = vec![ev("m", 0, 300)];
        let mut state = AlarmState::default();
        state.snooze("m", t(600)); // due after the meeting ended
        assert!(compute_actions(&events, t(700), &state, &cfg()).is_empty());
    }

    #[test]
    fn two_events_same_start_both_fire() {
        let events = vec![ev("a", 300, 1200), ev("b", 300, 900)];
        let state = AlarmState::default();
        let actions = compute_actions(&events, t(300), &state, &cfg());
        assert_eq!(actions.len(), 2, "one overlay, two cards — but two fire actions");
    }

    #[test]
    fn event_moved_after_t5_refires_nothing_until_new_times() {
        // T-5 fired for original start; event moved +1h → occurrence_key embeds the
        // NEW start, so it's a fresh key with fresh alarms. Old key simply never
        // matches future fetches.
        let mut state = AlarmState::default();
        state.mark_fired("(id @ old)", AlarmKind::TMinus5, t(0));
        let moved = vec![AlarmEvent {
            occurrence_key: "(id @ new)".into(),
            ..ev("ignored", 3600, 5400)
        }];
        let actions = compute_actions(&moved, t(3600 - 300), &state, &cfg());
        assert_eq!(kinds(&actions), vec![("(id @ new)".into(), AlarmKind::TMinus5)]);
    }

    #[test]
    fn next_due_picks_earliest_unfired() {
        let events = vec![ev("a", 600, 1200), ev("b", 900, 1500)];
        let mut state = AlarmState::default();
        assert_eq!(next_due(&events, t(0), &state, &cfg()), Some(t(300))); // a's T-5
        state.mark_fired("a", AlarmKind::TMinus5, t(300));
        assert_eq!(next_due(&events, t(301), &state, &cfg()), Some(t(600))); // a's T-0
    }

    #[test]
    fn config_lead_minutes_changes_t5_timing() {
        let events = vec![ev("m", 600, 1500)];
        let state = AlarmState::default();
        let one_min = AlarmConfig { lead_secs: 60, ..AlarmConfig::default() };
        assert!(compute_actions(&events, t(300), &state, &one_min).is_empty(), "5min lead disabled");
        let at_t1 = compute_actions(&events, t(540), &state, &one_min);
        assert_eq!(kinds(&at_t1), vec![("m".into(), AlarmKind::TMinus5)]);
        assert_eq!(next_due(&events, t(0), &state, &one_min), Some(t(540)));
    }

    #[test]
    fn config_tentative_and_pending_policies() {
        let mut tentative = ev("tent", 300, 900);
        tentative.my_rsvp = Some("tentative".into());
        let mut pending = ev("pend", 300, 900);
        pending.my_rsvp = Some("pending".into());
        let events = vec![tentative, pending];
        let state = AlarmState::default();

        let both_off = AlarmConfig { alert_tentative: false, alert_pending: false, ..AlarmConfig::default() };
        assert!(compute_actions(&events, t(400), &state, &both_off).is_empty());

        let defaults = cfg();
        assert_eq!(compute_actions(&events, t(400), &state, &defaults).len(), 2);
    }

    #[test]
    fn config_only_video_events() {
        let mut no_link = ev("nolink", 300, 900);
        no_link.has_link = false;
        let with_link = ev("link", 300, 900);
        let only_video = AlarmConfig { only_video_events: true, ..AlarmConfig::default() };
        let state = AlarmState::default();
        let actions = compute_actions(&[no_link, with_link], t(400), &state, &only_video);
        assert_eq!(kinds(&actions), vec![("link".into(), AlarmKind::TZero)]);
    }

    #[test]
    fn gc_drops_old_entries() {
        let mut state = AlarmState::default();
        state.mark_fired("old", AlarmKind::TZero, t(0));
        state.gc(t(49 * 3600));
        assert!(state.fired.is_empty());
    }
}
