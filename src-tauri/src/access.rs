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
        (AuthKind::FullAccess, FetchOutcome::Failed) => {
            (AccessState::Lost, "fetch_failed_despite_authorized")
        }
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
}

impl AccessTracker {
    /// Start announced-healthy: a healthy boot is silent; a process that boots
    /// without access takes `CONFIRM_TICKS` readings to announce (a brief delay,
    /// not a missed alert).
    pub const fn new() -> Self {
        Self { announced: AccessState::Ok, streak: 0 }
    }

    /// The currently-surfaced state (what the badge/banner reflect).
    pub fn announced(&self) -> AccessState {
        self.announced
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
            AccessState::Lost => AccessEdge::Lost { reason },
            AccessState::Ok => AccessEdge::Restored,
        })
    }
}

impl Default for AccessTracker {
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
}
