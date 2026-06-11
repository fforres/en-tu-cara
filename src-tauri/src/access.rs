//! Calendar-access health state machine — the prime-directive guard.
//!
//! The unforgivable failure is "a meeting started and no alert fired." One way
//! that happens silently: the running process LOSES macOS calendar access
//! mid-session (a TCC reset when a differently-signed binary claims the same
//! bundle id; a stale EventKit store after sleep; the user revoking access). The
//! pipeline then yields 0 events and nothing fires — with no signal to the user.
//!
//! This module is the PURE core that decides, each scheduler tick, whether we're
//! healthy or have lost access, and emits an EDGE only on a transition — exactly
//! like `scheduler::presence_transition`, so the loud surfaces (notification,
//! menu-bar badge, settings banner) and self-heal fire ONCE per transition, not
//! every tick. All side effects live in the scheduler/calendar; this stays pure
//! and trivially testable.

/// Whether calendar reads are currently working for this process.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccessState {
    Ok,
    Lost,
}

/// The coarse authorization subset we act on. Kept free of the `eventkit` type so
/// this module is pure and testable; `calendar::authorization_status_kind` maps
/// the real `AuthorizationStatus` onto it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AuthKind {
    FullAccess,
    NotDetermined,
    DeniedOrRestricted,
}

/// Whether this tick's calendar read succeeded. `FullAccess` + `Failed` is the
/// subtle stale-store case (status says fine, reads return nothing) — the silent
/// failure the incident described — so the read outcome, not just the status,
/// drives the machine.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FetchOutcome {
    Ok,
    Failed,
}

/// The transition that just happened, if any. `Some` only on an `Ok`↔`Lost` edge.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AccessEdge {
    /// Just became (or started) unhealthy. `reason` is a stable, PII-free tag.
    Lost { reason: &'static str },
    /// Recovered to healthy.
    Restored,
}

/// Raw per-tick classification (no debounce): is this read healthy, and if not,
/// why. Pure. The scheduler rebuilds the event store on a `Lost` reading BEFORE
/// the debounce announces, so a transient stale store recovers silently.
pub fn classify(auth: AuthKind, fetch: FetchOutcome) -> (AccessState, &'static str) {
    match (auth, fetch) {
        (AuthKind::FullAccess, FetchOutcome::Ok) => (AccessState::Ok, ""),
        (AuthKind::FullAccess, FetchOutcome::Failed) => (AccessState::Lost, REASON_FETCH_FAILED),
        (AuthKind::NotDetermined, _) => (AccessState::Lost, "authorization_not_determined"),
        (AuthKind::DeniedOrRestricted, _) => (AccessState::Lost, "authorization_denied"),
    }
}

/// Confirm a state change only after this many consecutive opposite readings.
/// Debounces a transient blip — a single failed fetch, or a store-rebuild that
/// briefly succeeds then fails — into either nothing (self-heals first) or a
/// single notification (persistent loss). 2 ticks ≈ ≤60s confirmation.
const CONFIRM_TICKS: u32 = 2;

/// Debounced access announcer: tracks the announced (surfaced) state plus a
/// streak of consecutive readings disagreeing with it, and flips — emitting an
/// edge — only once the streak reaches `CONFIRM_TICKS`. So a stale store that the
/// self-heal rebuilds on the next tick never produces a notification, and a
/// flapping grant doesn't spam lost/restored. Pure + unit-tested.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessTracker {
    announced: AccessState,
    streak: u32,
    announced_reason: &'static str,
}

impl AccessTracker {
    /// Start announced-healthy: a healthy boot is silent; a process that boots
    /// without access takes `CONFIRM_TICKS` readings to announce (a brief delay,
    /// not a missed alert).
    pub const fn new() -> Self {
        Self { announced: AccessState::Ok, streak: 0, announced_reason: "" }
    }

    /// The currently-surfaced state (what the badge/banner reflect).
    pub fn announced(&self) -> AccessState {
        self.announced
    }

    /// The reason tag of the announced Lost state ("" when announced-Ok). Lets
    /// the banners say the RIGHT thing: a revoked grant needs the user to
    /// re-grant; reads failing despite a grant is ours to repair.
    pub fn announced_reason(&self) -> &'static str {
        self.announced_reason
    }

    /// Feed one raw reading; returns `Some(edge)` only when the announced state
    /// flips after `CONFIRM_TICKS` consecutive opposite readings.
    pub fn observe(&mut self, raw: AccessState, reason: &'static str) -> Option<AccessEdge> {
        if raw == self.announced {
            self.streak = 0;
            return None;
        }
        self.streak += 1;
        if self.streak < CONFIRM_TICKS {
            return None;
        }
        self.announced = raw;
        self.streak = 0;
        Some(match raw {
            AccessState::Lost => {
                self.announced_reason = reason;
                AccessEdge::Lost { reason }
            }
            AccessState::Ok => {
                self.announced_reason = "";
                AccessEdge::Restored
            }
        })
    }
}

impl Default for AccessTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// The reason tag for the one loss mode the per-tick store rebuild can't fix.
pub const REASON_FETCH_FAILED: &str = "fetch_failed_despite_authorized";

/// Consecutive `FullAccess + Failed` readings before we conclude the TCC grant
/// itself is unusable. Every Lost reading already rebuilds the event store, so
/// by N consecutive failures we have N fresh stores that ALL returned nothing —
/// that's not a stale-store blip (fixed by 1 rebuild) and not sleep/wake (fixed
/// by the next tick). 6 ticks ≈ ≤3 min: long enough to never false-positive on
/// a transient, short enough that alerts aren't dead for a whole meeting slot.
const REPAIR_TICKS: u32 = 6;

/// Escalation tracker for the poisoned-grant incident (2026-06-10): macOS held
/// a legacy-level Calendar record (TCC authValue=2) that calaccessd, wanting
/// the modern full-access level (4), refused to honor — `authorization_status`
/// said FullAccess while every read returned nothing, forever, across process
/// restarts and re-grants. No store rebuild can fix that; the only cure is
/// destroying the record (`tccutil reset Calendar <bundle id>`) and re-granting
/// fresh. This pure tracker decides WHEN that repair is warranted: persistent
/// `fetch_failed_despite_authorized` readings, once per loss episode (re-arms
/// only after a healthy reading, so a repair that doesn't take can't loop).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GrantRepairTracker {
    consecutive: u32,
    fired: bool,
}

impl GrantRepairTracker {
    pub const fn new() -> Self {
        Self { consecutive: 0, fired: false }
    }

    /// Feed one raw reading; returns true exactly once per loss episode, when
    /// `REPAIR_TICKS` consecutive readings say "authorized but reads fail".
    /// Other Lost reasons (NotDetermined/Denied) have their own recovery flows
    /// (re-prompt / System Settings deep-link) — they reset the streak but NOT
    /// the fired latch, so the post-repair NotDetermined phase can't re-arm it.
    pub fn observe(&mut self, raw: AccessState, reason: &str) -> bool {
        match (raw, reason) {
            (AccessState::Ok, _) => {
                self.consecutive = 0;
                self.fired = false;
                false
            }
            (AccessState::Lost, REASON_FETCH_FAILED) => {
                self.consecutive += 1;
                if !self.fired && self.consecutive >= REPAIR_TICKS {
                    self.fired = true;
                    true
                } else {
                    false
                }
            }
            (AccessState::Lost, _) => {
                self.consecutive = 0;
                false
            }
        }
    }
}

impl Default for GrantRepairTracker {
    fn default() -> Self {
        Self::new()
    }
}

/// The COMPLETE access-health state for one process — the debounced announcer,
/// the repair escalation, and the downtime counter — as one model behind one
/// lock. Before this existed, the scheduler smeared these across three statics
/// taken under three separate locks per tick (and re-locked one mid-edge, which
/// hid a real bug: the recovery tick zeroes the down-counter BEFORE the
/// debounce confirms Restored, so "was down N tick(s)" always reported 0).
/// Facets of one conceptual state travel together. Pure: the caller owns all
/// side effects (store rebuild, loud surfaces, repair execution).
pub struct AccessHealth {
    tracker: AccessTracker,
    repair: GrantRepairTracker,
    /// Consecutive raw-Lost ticks in the CURRENT dip (0 while healthy).
    down_ticks: u32,
    /// Length of the most recently ENDED dip — what a Restored edge reports
    /// (by the time the debounce confirms recovery, `down_ticks` is already 0).
    last_episode: u32,
}

/// One tick's verdict: the raw reading plus which side effects are due.
pub struct Observation {
    /// Undebounced. Lost → the caller rebuilds the event store (the cheap,
    /// always-safe self-heal that fixes a merely-stale store by next tick).
    pub raw: AccessState,
    /// Debounced transition, if any — drive the loud surfaces once per edge.
    pub edge: Option<AccessEdge>,
    /// The grant itself looks unusable — fire the TCC repair (once/episode).
    pub repair_due: bool,
    /// On a `Restored` edge: how many ticks the ended episode lasted.
    /// Otherwise: the current dip's running count.
    pub down_ticks: u32,
}

impl AccessHealth {
    pub const fn new() -> Self {
        Self {
            tracker: AccessTracker::new(),
            repair: GrantRepairTracker::new(),
            down_ticks: 0,
            last_episode: 0,
        }
    }

    /// Feed one real read's (auth, fetch) pair; returns everything the caller
    /// must act on. The classification, both trackers, and the downtime counter
    /// advance together — one lock, one call, no partially-applied state.
    pub fn observe(&mut self, auth: AuthKind, fetch: FetchOutcome) -> Observation {
        let (raw, reason) = classify(auth, fetch);
        match raw {
            AccessState::Lost => self.down_ticks += 1,
            AccessState::Ok => {
                if self.down_ticks > 0 {
                    self.last_episode = self.down_ticks;
                }
                self.down_ticks = 0;
            }
        }
        let edge = self.tracker.observe(raw, reason);
        let repair_due = self.repair.observe(raw, reason);
        let down_ticks = if matches!(edge, Some(AccessEdge::Restored)) {
            self.last_episode
        } else {
            self.down_ticks
        };
        Observation { raw, edge, repair_due, down_ticks }
    }

    /// The currently-surfaced state (what the badge/banner reflect).
    pub fn announced(&self) -> AccessState {
        self.tracker.announced()
    }

    /// The reason tag of the announced Lost state ("" when announced-Ok).
    pub fn announced_reason(&self) -> &'static str {
        self.tracker.announced_reason()
    }
}

impl Default for AccessHealth {
    fn default() -> Self {
        Self::new()
    }
}

// --- Loud surfaces (macOS) --------------------------------------------------
// All three surfaces fired on the Lost/Restored edge. Each is non-blocking so the
// scheduler tick that calls these never stalls the alarm path.

/// Announce that calendar access was lost: menu-bar ⚠️ badge + Settings banner +
/// a macOS notification.
#[cfg(target_os = "macos")]
pub fn announce_lost(app: &tauri::AppHandle, reason: &str) {
    use tauri::Emitter;
    crate::tray::set_access_badge(app, Some("⚠\u{fe0f} Calendar access lost".to_string()));
    let _ = app.emit("access-state-changed", serde_json::json!({ "state": "lost", "reason": reason }));
    notify(
        app,
        "Calendar access lost",
        "En Tu Cara can't read your calendar — alerts are paused until access is restored.",
    );
}

/// Announce recovery: clear the badge + Settings banner + a macOS notification.
#[cfg(target_os = "macos")]
pub fn announce_restored(app: &tauri::AppHandle) {
    use tauri::Emitter;
    crate::tray::set_access_badge(app, None);
    let _ = app.emit("access-state-changed", serde_json::json!({ "state": "ok" }));
    notify(
        app,
        "Calendar access restored",
        "En Tu Cara is reading your calendar again — alerts are active.",
    );
}

/// Fire a macOS notification off-thread (the show() call is local but kept off
/// the tick to honor "never block the alarm path").
#[cfg(target_os = "macos")]
fn notify(app: &tauri::AppHandle, title: &str, body: &str) {
    use tauri_plugin_notification::NotificationExt;
    let app = app.clone();
    let title = title.to_string();
    let body = body.to_string();
    std::thread::spawn(move || {
        let _ = app.notification().builder().title(title).body(body).show();
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_maps_each_case() {
        assert_eq!(classify(AuthKind::FullAccess, FetchOutcome::Ok), (AccessState::Ok, ""));
        assert_eq!(
            classify(AuthKind::FullAccess, FetchOutcome::Failed),
            (AccessState::Lost, "fetch_failed_despite_authorized"),
            "authorized but reads fail = the stale-store case"
        );
        assert_eq!(
            classify(AuthKind::NotDetermined, FetchOutcome::Failed).0,
            AccessState::Lost
        );
        assert_eq!(
            classify(AuthKind::DeniedOrRestricted, FetchOutcome::Ok),
            (AccessState::Lost, "authorization_denied")
        );
    }

    // Convenience: feed a sequence of raw states, collect the edges emitted.
    fn run(seq: &[AccessState]) -> Vec<AccessEdge> {
        let mut t = AccessTracker::new();
        seq.iter().filter_map(|s| t.observe(*s, "fetch_failed_despite_authorized")).collect()
    }

    #[test]
    fn healthy_steady_state_is_silent() {
        assert!(run(&[AccessState::Ok, AccessState::Ok, AccessState::Ok]).is_empty());
    }

    #[test]
    fn persistent_loss_announces_once_after_confirmation() {
        // Two consecutive Lost confirms → one Lost edge, then steady-silent.
        let edges = run(&[AccessState::Lost, AccessState::Lost, AccessState::Lost, AccessState::Lost]);
        assert_eq!(edges, vec![AccessEdge::Lost { reason: "fetch_failed_despite_authorized" }]);
    }

    #[test]
    fn a_self_healing_blip_never_announces() {
        // The realistic case: one failed read (stale store), rebuilt next tick →
        // Ok. Below the confirm threshold → ZERO notifications. This is what stops
        // the flapping lost/restored spam we saw live.
        assert!(run(&[AccessState::Lost, AccessState::Ok, AccessState::Ok]).is_empty());
    }

    #[test]
    fn flapping_does_not_spam() {
        // Lost/Ok/Lost/Ok never reaches 2 consecutive → never announces.
        assert!(run(&[
            AccessState::Lost, AccessState::Ok, AccessState::Lost, AccessState::Ok, AccessState::Lost
        ])
        .is_empty());
    }

    #[test]
    fn lost_then_genuine_recovery_announces_both_once() {
        let edges = run(&[
            AccessState::Lost, AccessState::Lost, // confirm Lost
            AccessState::Ok, AccessState::Ok, // confirm Restored
        ]);
        assert_eq!(
            edges,
            vec![AccessEdge::Lost { reason: "fetch_failed_despite_authorized" }, AccessEdge::Restored]
        );
    }

    #[test]
    fn announced_reflects_confirmed_state() {
        let mut t = AccessTracker::new();
        assert_eq!(t.announced(), AccessState::Ok);
        t.observe(AccessState::Lost, "x");
        assert_eq!(t.announced(), AccessState::Ok, "not yet confirmed");
        t.observe(AccessState::Lost, "x");
        assert_eq!(t.announced(), AccessState::Lost, "confirmed after 2");
    }

    #[test]
    fn announced_reason_tracks_the_confirmed_loss_and_clears_on_recovery() {
        // The banners branch on this: revoked grant → "grant access" CTA;
        // reads-failing-despite-grant → "repairing" copy. It must reflect the
        // CONFIRMED state only, and clear when healthy again.
        let mut t = AccessTracker::new();
        assert_eq!(t.announced_reason(), "");
        t.observe(AccessState::Lost, REASON_FETCH_FAILED);
        assert_eq!(t.announced_reason(), "", "unconfirmed loss must not leak a reason");
        t.observe(AccessState::Lost, REASON_FETCH_FAILED);
        assert_eq!(t.announced_reason(), REASON_FETCH_FAILED);
        t.observe(AccessState::Ok, "");
        t.observe(AccessState::Ok, "");
        assert_eq!(t.announced_reason(), "", "recovery clears the reason");
    }

    // --- GrantRepairTracker (the poisoned-grant escalation) -----------------

    fn lost_fetch(t: &mut GrantRepairTracker, n: u32) -> bool {
        (0..n).map(|_| t.observe(AccessState::Lost, REASON_FETCH_FAILED)).any(|f| f)
    }

    #[test]
    fn repair_fires_once_after_persistent_authorized_failures() {
        let mut t = GrantRepairTracker::new();
        assert!(!lost_fetch(&mut t, 5), "below threshold — store rebuilds may still fix it");
        assert!(t.observe(AccessState::Lost, REASON_FETCH_FAILED), "6th consecutive fires");
        assert!(!lost_fetch(&mut t, 20), "fired latch: never again within the episode");
    }

    #[test]
    fn a_single_healthy_reading_resets_the_streak_and_rearms() {
        let mut t = GrantRepairTracker::new();
        lost_fetch(&mut t, 5);
        t.observe(AccessState::Ok, "");
        assert!(!lost_fetch(&mut t, 5), "streak restarted after recovery");
        assert!(t.observe(AccessState::Lost, REASON_FETCH_FAILED), "re-armed: a NEW episode fires");
    }

    // --- AccessHealth (the one-lock aggregate) -------------------------------

    #[test]
    fn access_health_drives_all_three_facets_from_one_observation() {
        // One Lost episode end-to-end: store-rebuild cue every Lost tick, one
        // debounced Lost edge, repair due exactly once, and the Restored edge
        // reporting the episode length.
        let mut h = AccessHealth::new();
        let mut lost_edges = 0;
        let mut repairs = 0;
        for tick in 1..=8 {
            let obs = h.observe(AuthKind::FullAccess, FetchOutcome::Failed);
            assert_eq!(obs.raw, AccessState::Lost, "every failed read cues a store rebuild");
            assert_eq!(obs.down_ticks, tick, "running dip count");
            lost_edges += matches!(obs.edge, Some(AccessEdge::Lost { .. })) as u32;
            repairs += obs.repair_due as u32;
        }
        assert_eq!(lost_edges, 1, "loud surfaces fire once per episode");
        assert_eq!(repairs, 1, "repair fires once per episode");
        assert_eq!(h.announced(), AccessState::Lost);
        assert_eq!(h.announced_reason(), REASON_FETCH_FAILED);
    }

    #[test]
    fn restored_edge_reports_the_episode_length_not_zero() {
        // The bug the aggregate fixed: with separate statics, the recovery tick
        // zeroed the down-counter BEFORE the debounce confirmed Restored, so the
        // "was down N tick(s)" log/telemetry always said 0. The Restored edge
        // must carry the ended episode's real length.
        let mut h = AccessHealth::new();
        for _ in 0..5 {
            h.observe(AuthKind::FullAccess, FetchOutcome::Failed);
        }
        let first_ok = h.observe(AuthKind::FullAccess, FetchOutcome::Ok);
        assert!(first_ok.edge.is_none(), "recovery not yet confirmed");
        let second_ok = h.observe(AuthKind::FullAccess, FetchOutcome::Ok);
        assert_eq!(second_ok.edge, Some(AccessEdge::Restored));
        assert_eq!(second_ok.down_ticks, 5, "Restored reports the 5-tick episode, not 0");
    }

    #[test]
    fn other_loss_reasons_never_trigger_repair_nor_rearm_it() {
        // NotDetermined/Denied have their own flows (re-prompt / System
        // Settings); resetting TCC on them would be destructive. And after a
        // repair fires, the grant goes NotDetermined while the user is prompted
        // — that phase must not re-arm the latch.
        let mut t = GrantRepairTracker::new();
        for _ in 0..20 {
            assert!(!t.observe(AccessState::Lost, "authorization_not_determined"));
        }
        assert!(!lost_fetch(&mut t, 5), "not_determined readings reset the fetch-failed streak");
        assert!(t.observe(AccessState::Lost, REASON_FETCH_FAILED));
        // Post-repair: NotDetermined while prompting, then fetch-failures again
        // (say the user dismissed the prompt) — still latched, no second reset.
        t.observe(AccessState::Lost, "authorization_not_determined");
        assert!(!lost_fetch(&mut t, 10), "latch survives the NotDetermined phase");
    }
}
