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

/// Result of one evaluation: the new state to store, plus an edge to act on.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AccessEval {
    pub state: AccessState,
    pub edge: Option<AccessEdge>,
}

/// Decide access health from this tick's authorization status + read outcome,
/// emitting an edge ONLY on a transition. `prev == None` is the first
/// observation: a healthy start is a silent baseline, but starting/observing
/// `Lost` for the first time DOES shout (a process running with no access is a
/// real, surfaceable problem). Pure — no clock, no I/O.
pub fn evaluate_access(prev: Option<AccessState>, auth: AuthKind, fetch: FetchOutcome) -> AccessEval {
    let (state, lost_reason) = match (auth, fetch) {
        (AuthKind::FullAccess, FetchOutcome::Ok) => (AccessState::Ok, ""),
        (AuthKind::FullAccess, FetchOutcome::Failed) => {
            (AccessState::Lost, "fetch_failed_despite_authorized")
        }
        (AuthKind::NotDetermined, _) => (AccessState::Lost, "authorization_not_determined"),
        (AuthKind::DeniedOrRestricted, _) => (AccessState::Lost, "authorization_denied"),
    };

    let edge = match (prev, state) {
        // Steady state (including a healthy first observation) → silent.
        (Some(p), s) if p == s => None,
        (None, AccessState::Ok) => None,
        // Became, or started, Lost → shout once.
        (_, AccessState::Lost) => Some(AccessEdge::Lost { reason: lost_reason }),
        // Recovered → announce once.
        (_, AccessState::Ok) => Some(AccessEdge::Restored),
    };

    AccessEval { state, edge }
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

    fn eval(prev: Option<AccessState>, auth: AuthKind, fetch: FetchOutcome) -> AccessEval {
        evaluate_access(prev, auth, fetch)
    }

    #[test]
    fn healthy_start_is_a_silent_baseline() {
        let r = eval(None, AuthKind::FullAccess, FetchOutcome::Ok);
        assert_eq!(r.state, AccessState::Ok);
        assert_eq!(r.edge, None, "a healthy first tick must not shout");
    }

    #[test]
    fn starting_without_access_shouts_immediately() {
        let r = eval(None, AuthKind::NotDetermined, FetchOutcome::Failed);
        assert_eq!(r.state, AccessState::Lost);
        assert_eq!(
            r.edge,
            Some(AccessEdge::Lost { reason: "authorization_not_determined" }),
            "a process that starts with no access is a real problem — surface it"
        );
    }

    #[test]
    fn losing_access_mid_session_shouts_once_then_stays_silent() {
        // The incident: was healthy, authorization flips to NotDetermined.
        let lost = eval(Some(AccessState::Ok), AuthKind::NotDetermined, FetchOutcome::Failed);
        assert_eq!(lost.state, AccessState::Lost);
        assert_eq!(lost.edge, Some(AccessEdge::Lost { reason: "authorization_not_determined" }));
        // Steady Lost → no repeat notifications every 30s.
        let still_lost = eval(Some(AccessState::Lost), AuthKind::NotDetermined, FetchOutcome::Failed);
        assert_eq!(still_lost.edge, None, "steady Lost must not re-shout each tick");
    }

    #[test]
    fn authorized_but_reads_fail_is_lost_the_stale_store_case() {
        // Status reports FullAccess yet reads return nothing (stale EKEventStore
        // after sleep) — the silent-0-events failure. Must be treated as Lost.
        let r = eval(Some(AccessState::Ok), AuthKind::FullAccess, FetchOutcome::Failed);
        assert_eq!(r.state, AccessState::Lost);
        assert_eq!(r.edge, Some(AccessEdge::Lost { reason: "fetch_failed_despite_authorized" }));
    }

    #[test]
    fn denied_is_lost() {
        let r = eval(Some(AccessState::Ok), AuthKind::DeniedOrRestricted, FetchOutcome::Failed);
        assert_eq!(r.edge, Some(AccessEdge::Lost { reason: "authorization_denied" }));
    }

    #[test]
    fn recovery_announces_once() {
        let restored = eval(Some(AccessState::Lost), AuthKind::FullAccess, FetchOutcome::Ok);
        assert_eq!(restored.state, AccessState::Ok);
        assert_eq!(restored.edge, Some(AccessEdge::Restored));
        // Steady healthy afterwards → silent.
        let healthy = eval(Some(AccessState::Ok), AuthKind::FullAccess, FetchOutcome::Ok);
        assert_eq!(healthy.edge, None);
    }

    #[test]
    fn full_access_with_a_successful_read_is_always_ok() {
        // Even if a transient earlier failure happened, a good read clears it.
        assert_eq!(eval(Some(AccessState::Lost), AuthKind::FullAccess, FetchOutcome::Ok).state, AccessState::Ok);
    }
}
