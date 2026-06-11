//! Last-good event snapshot for the ALARM path — the final line of defense for
//! the prime directive.
//!
//! The popover has always preserved last-known events on a failed read; the
//! scheduler did NOT (a failed read fed compute_actions an empty list), so
//! during an access-loss episode alarms silently starved — on the 2026-06-10
//! wedged machine, reads died 4m31s after every launch and every meeting after
//! that would have fired nothing. Serving the snapshot keeps alerts armed
//! through the episode, while the access machine (which still sees the Failed
//! outcome) shouts and repairs in parallel.
//!
//! TTL = the fetch window's forward reach (2 days): a snapshot can't usefully
//! promise more than it fetched. The known staleness trade — a meeting
//! deleted/moved AFTER the snapshot still alerts at its old time — is accepted:
//! a loud false alarm over a silent miss, and the tray/banner already say
//! events may be stale. Pure (clock injected) + tested.

use crate::alarm_core::AlarmEvent;
use chrono::{DateTime, Utc};

pub struct SnapshotCache {
    events: Vec<AlarmEvent>,
    fetched_at: Option<DateTime<Utc>>,
    /// Whether the previous read was served FROM the snapshot — owned here so
    /// the caller gets clean once-per-episode edges (`first` on entry,
    /// `store() == true` on recovery) instead of juggling its own latch static.
    serving_stale: bool,
}

/// One stale serve: the snapshot's events, their age, and whether this is the
/// FIRST stale serve of the episode (the caller's cue to log once, not per tick).
pub struct StaleServe {
    pub events: Vec<AlarmEvent>,
    pub age_secs: i64,
    pub first: bool,
}

impl SnapshotCache {
    const TTL_SECS: i64 = 48 * 3600;

    pub const fn new() -> Self {
        Self { events: Vec::new(), fetched_at: None, serving_stale: false }
    }

    /// Record a good read (wholesale replace — a meeting deleted upstream must
    /// vanish, not linger). Returns true when this read ends a stale-serving
    /// episode — the caller's cue to log the recovery once.
    pub fn store(&mut self, events: &[AlarmEvent], now: DateTime<Utc>) -> bool {
        self.events = events.to_vec();
        self.fetched_at = Some(now);
        std::mem::replace(&mut self.serving_stale, false)
    }

    /// What to serve when the live read failed: the snapshot while it exists
    /// and is fresh enough to trust, else `None` (never had a good read, or the
    /// snapshot outlived its window — serving past it would feign health).
    pub fn serve(&mut self, now: DateTime<Utc>) -> Option<StaleServe> {
        let age_secs = (now - self.fetched_at?).num_seconds();
        if age_secs > Self::TTL_SECS {
            return None;
        }
        let first = !std::mem::replace(&mut self.serving_stale, true);
        Some(StaleServe { events: self.events.clone(), age_secs, first })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::Duration as ChronoDuration;

    fn ev(key: &str) -> AlarmEvent {
        let start =
            DateTime::parse_from_rfc3339("2026-06-10T10:00:00Z").unwrap().with_timezone(&Utc);
        AlarmEvent {
            occurrence_key: key.into(),
            title: "t".into(),
            start,
            end: start + ChronoDuration::seconds(900),
            all_day: false,
            status: "confirmed".into(),
            my_rsvp: None,
            has_link: true,
        }
    }

    fn at(rfc3339: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc3339).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn serves_last_good_events_while_fresh() {
        // The prime-directive case: reads die mid-episode (the 4m31s wedge) —
        // the alarm path must keep the last good read, not starve.
        let mut s = SnapshotCache::new();
        assert!(s.serve(at("2026-06-10T09:00:00Z")).is_none(), "no good read yet → nothing");
        s.store(&[ev("a"), ev("b")], at("2026-06-10T09:00:00Z"));
        let serve = s.serve(at("2026-06-10T09:05:00Z")).expect("fresh snapshot serves");
        assert_eq!(serve.events.len(), 2);
        assert_eq!(serve.age_secs, 300);
    }

    #[test]
    fn expires_past_its_fetch_window() {
        // A snapshot only fetched +2 days of events — past 48h it can't promise
        // coverage and serving it would feign health. Cut it off.
        let mut s = SnapshotCache::new();
        s.store(&[ev("a")], at("2026-06-10T09:00:00Z"));
        assert!(s.serve(at("2026-06-12T09:00:00Z")).is_some(), "exactly 48h still serves");
        assert!(s.serve(at("2026-06-12T09:00:01Z")).is_none(), "past 48h expires");
    }

    #[test]
    fn store_replaces_wholesale() {
        // Every good read replaces the snapshot (no merge): a meeting deleted
        // upstream must vanish from the next snapshot, not linger.
        let mut s = SnapshotCache::new();
        s.store(&[ev("a"), ev("b")], at("2026-06-10T09:00:00Z"));
        s.store(&[ev("c")], at("2026-06-10T09:01:00Z"));
        let serve = s.serve(at("2026-06-10T09:02:00Z")).unwrap();
        assert_eq!(serve.events.iter().map(|e| e.occurrence_key.as_str()).collect::<Vec<_>>(), [
            "c"
        ]);
    }

    #[test]
    fn edges_fire_once_per_episode() {
        // The latch lives HERE so the scheduler logs exactly one "serving
        // stale" line on entry and one "recovered" line on exit — never
        // per-tick spam, never a missed episode boundary.
        let mut s = SnapshotCache::new();
        assert!(!s.store(&[ev("a")], at("2026-06-10T09:00:00Z")), "healthy store: no edge");
        assert!(s.serve(at("2026-06-10T09:01:00Z")).unwrap().first, "first stale serve flags");
        assert!(!s.serve(at("2026-06-10T09:02:00Z")).unwrap().first, "second is silent");
        assert!(s.store(&[ev("a")], at("2026-06-10T09:03:00Z")), "recovery store: edge");
        assert!(!s.store(&[ev("a")], at("2026-06-10T09:04:00Z")), "steady healthy: silent");
        assert!(s.serve(at("2026-06-10T09:05:00Z")).unwrap().first, "new episode re-arms");
    }
}
