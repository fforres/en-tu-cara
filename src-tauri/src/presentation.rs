//! The takeover presentation — the single owner of "what is on screen right now".
//!
//! One rule: **the panels are a function of the card set.** Cards are added by the
//! fire path and removed by dismiss/snooze/ignore; every mutation goes through this
//! module, and this module is the only thing that opens or closes panels or starts
//! or stops the sound. Nothing else may touch either side.
//!
//! That rule is why this module exists. The card set and the panel lifecycle used
//! to be separate concerns with six call sites hand-rolling the ordering between
//! them, and every bug in the takeover's history was a state where the two
//! disagreed:
//!   - cards present, panels gone → an audible alarm with nothing to dismiss (the
//!     force-quit report; the sleep/wake self-heal below is the cure).
//!   - panels present, cards gone → a blank takeover, sound looping, resurrected
//!     every tick by that same self-heal.
//!
//! Neither is reachable when panels are derived from cards in one place.
//!
//! THREADING: every mutation runs on the MAIN THREAD — the fire path dispatches
//! via `run_on_main_thread`, and the `#[tauri::command]`s that reach here are all
//! sync, which Tauri dispatches inline on the main thread. That serialization is
//! load-bearing: it is what makes `reassert`'s read-then-act atomic against a
//! dismiss landing at the same moment. Marking one of those commands `async` would
//! move it to the async runtime and reopen exactly that race.

#![cfg(target_os = "macos")]

use crate::scheduler::lock_resilient;
use std::sync::Mutex;
use tauri::{AppHandle, Emitter};

/// The cards currently on screen. Outlives the panels on purpose: a panel rebuilt
/// by `reassert` repopulates from here (the webview pulls `get_active_alarms` on
/// mount), which is what lets the self-heal recover a LOST overlay rather than only
/// re-front a live one.
static PRESENTED: Mutex<Vec<serde_json::Value>> = Mutex::new(Vec::new());

/// The occurrence key a card belongs to, if it has one.
fn key_of(card: &serde_json::Value) -> Option<&str> {
    card.get("occurrence_key").and_then(|v| v.as_str())
}

/// Every card NOT belonging to `occurrence_key`. Pure, for testing: this is what
/// lets two overlapping meetings be actioned independently — dismissing one must
/// drop all of its cards (a reminder AND a T-0 can both be up) and leave the
/// other's untouched.
fn without_occurrence(cards: &[serde_json::Value], occurrence_key: &str) -> Vec<serde_json::Value> {
    cards.iter().filter(|c| key_of(c) != Some(occurrence_key)).cloned().collect()
}

/// The cards on screen, for the overlay webview to pull on mount.
///
/// The command NAME is load-bearing — `OverlayAlert.tsx` invokes
/// `get_active_alarms` — so it keeps the older wording even though this module
/// calls them cards.
#[tauri::command]
pub fn get_active_alarms() -> Vec<serde_json::Value> {
    lock_resilient(&PRESENTED).clone()
}

/// Is anything on screen? Drives the scheduler's per-tick self-heal.
pub fn any_presented() -> bool {
    !lock_resilient(&PRESENTED).is_empty()
}

/// Is this occurrence on screen? The ignore path uses it to decide whether it also
/// has to take a live card down.
pub fn is_presented(occurrence_key: &str) -> bool {
    lock_resilient(&PRESENTED).iter().any(|c| key_of(c) == Some(occurrence_key))
}

/// Put a fired alarm on screen. MAIN THREAD ONLY.
///
/// Panels first, card second, and that order is load-bearing: the scheduler marks
/// an alarm fired only when this returns `Ok`, so on a failed show the alarm stays
/// unmarked and the next tick retries it. Adding the card before knowing the panels
/// are up would leave that retry to push a SECOND card for the same alarm.
pub fn present(app: &AppHandle, card: serde_json::Value) -> Result<(), String> {
    crate::overlay::show_overlays(app).map_err(|e| e.to_string())?;
    lock_resilient(&PRESENTED).push(card.clone());
    // Panels already booted get the push; freshly created ones pull on mount.
    let _ = app.emit("alarm-fired", &card);
    Ok(())
}

/// Re-establish the overlay for cards that are still on screen. MAIN THREAD ONLY.
///
/// The sleep/wake self-heal, run every scheduler tick. A fired alarm stays
/// presented until the user actions it, but its panels can be torn down or
/// stranded under it — most importantly across SYSTEM SLEEP, where display
/// reconfiguration on wake leaves the panels on coordinates no screen covers while
/// the sound loop keeps beating. `show_overlays` re-frames and re-fronts what
/// survived and rebuilds what didn't; it is a cheap no-op when everything is
/// already correct.
///
/// The `any_presented` check is the whole safety story, and it must happen HERE on
/// the main thread rather than at the scheduler's dispatch site: a dismiss landing
/// between the two would otherwise have us rebuild panels for an empty card set and
/// restart the sound — resurrecting an alarm the user just dismissed.
pub fn reassert(app: &AppHandle) {
    if !any_presented() {
        return;
    }
    if let Err(e) = crate::overlay::show_overlays(app) {
        log::error!("overlay self-heal re-assert failed: {e}");
    }
}

/// Take ONE occurrence off screen (dismiss / snooze / ignore of a single card).
pub fn finish(app: &AppHandle, occurrence_key: &str) {
    let remaining = {
        let mut cards = lock_resilient(&PRESENTED);
        *cards = without_occurrence(&cards, occurrence_key);
        cards.clone()
    };
    settle(app, remaining);
}

/// Take EVERYTHING off screen — Esc, and the zero-card safety Dismiss. The blunt
/// "get it all off my screen" escape hatch.
pub fn finish_all(app: &AppHandle) {
    lock_resilient(&PRESENTED).clear();
    settle(app, Vec::new());
}

/// Bring the panels in line with what is left: nothing → tear the takeover down;
/// otherwise re-render the still-open overlay against the reduced set.
///
/// The one place panels are derived from cards on removal, so "cards gone, panels
/// up" can't be built by accident.
fn settle(app: &AppHandle, remaining: Vec<serde_json::Value>) {
    if remaining.is_empty() {
        crate::overlay::close_overlays(app);
    } else {
        let _ = app.emit("alarms-updated", remaining);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These tests own the PRESENTED global (cargo runs tests in parallel threads
    /// inside ONE process). Bind the guard — `let _x = exclusive()`, never
    /// `let _ = ` — so it lives for the whole test. Clearing on ACQUIRE needs no
    /// `Drop` impl and still can't leak a card into the next test if this one
    /// panics: the panic poisons TEST_LOCK and `lock_resilient` recovers it.
    static TEST_LOCK: Mutex<()> = Mutex::new(());

    fn exclusive() -> std::sync::MutexGuard<'static, ()> {
        let guard = lock_resilient(&TEST_LOCK);
        lock_resilient(&PRESENTED).clear();
        guard
    }

    fn card(key: &str, kind: &str) -> serde_json::Value {
        serde_json::json!({"occurrence_key": key, "kind": kind, "title": "Standup"})
    }

    #[test]
    fn nothing_presented_means_no_reassert() {
        // An empty set must stay a no-op: an overlay may never pop out of nowhere
        // on a machine with no alarm on screen.
        let _x = exclusive();
        assert!(!any_presented());
    }

    #[test]
    fn a_dismiss_racing_the_reassert_leaves_the_overlay_down() {
        // THE regression that produced this module. The scheduler decides to
        // re-assert on its own thread and queues the work onto the main thread; a
        // dismiss can land in the gap, clearing the cards and closing the panels.
        // `reassert` re-reads here, so it becomes a no-op — without that, it would
        // rebuild panels for zero cards and restart the sound loop, i.e. resurrect
        // an alarm the user just dismissed.
        let _x = exclusive();
        lock_resilient(&PRESENTED).push(card("(A @ t)", "t_zero"));
        assert!(any_presented(), "scheduler sees a card and dispatches a re-assert");

        lock_resilient(&PRESENTED).clear(); // …user hits Dismiss first.

        assert!(!any_presented(), "a just-dismissed overlay must NOT be resurrected");
    }

    #[test]
    fn a_reassert_still_fires_when_the_alarm_outlives_the_dispatch() {
        // The other half: nothing changed in the gap, so the re-assert must still
        // act — otherwise the self-heal never heals anything.
        let _x = exclusive();
        lock_resilient(&PRESENTED).push(card("(A @ t)", "t_zero"));
        assert!(any_presented());
    }

    #[test]
    fn finishing_one_occurrence_drops_all_its_cards_and_keeps_the_others() {
        // Two overlapping meetings, A with both a reminder and a T-0 card.
        // Actioning A must drop BOTH of A's cards and leave B alone — the old
        // behavior cleared everything on any action.
        let cards = vec![
            card("(A @ t)", "reminder_5"),
            card("(A @ t)", "t_zero"),
            card("(B @ t)", "t_zero"),
        ];
        let remaining = without_occurrence(&cards, "(A @ t)");
        assert_eq!(remaining.len(), 1, "only B survives");
        assert_eq!(key_of(&remaining[0]), Some("(B @ t)"));
    }

    #[test]
    fn the_takeover_comes_down_only_once_the_last_card_is_gone() {
        // `settle` tears the panels down exactly when the set empties, so an
        // overlapping meeting can never be silently taken down with the one the
        // user actioned — and the self-heal keeps asserting until then.
        let _x = exclusive();
        *lock_resilient(&PRESENTED) = vec![card("(A @ t)", "t_zero"), card("(B @ t)", "t_zero")];

        let remaining = without_occurrence(&lock_resilient(&PRESENTED), "(A @ t)");
        *lock_resilient(&PRESENTED) = remaining;
        assert!(any_presented(), "B is still up — the overlay stays, and stays asserted");

        let remaining = without_occurrence(&lock_resilient(&PRESENTED), "(B @ t)");
        *lock_resilient(&PRESENTED) = remaining;
        assert!(!any_presented(), "last card actioned → the takeover comes down for good");
    }

    #[test]
    fn is_presented_detects_a_live_card_by_occurrence() {
        // The ignore path branches on this: true → also take the live card down;
        // false → leave the overlay completely alone. ANY card for the key counts,
        // whatever its kind.
        let _x = exclusive();
        *lock_resilient(&PRESENTED) =
            vec![card("(A @ t)", "reminder_5"), card("(B @ t)", "t_zero")];
        assert!(is_presented("(A @ t)"));
        assert!(is_presented("(B @ t)"));
        assert!(!is_presented("(C @ t)"), "absent key is not presented");
    }

    #[test]
    fn a_rebuilt_panel_finds_every_card_still_there() {
        // What a panel rebuilt by the self-heal pulls on mount. If a re-assert ever
        // cleared or bypassed the set, the user would get a blank takeover with a
        // beating sound and nothing to dismiss.
        let _x = exclusive();
        let (a, b) = (card("(A @ t)", "t_zero"), card("(B @ t)", "reminder_5"));
        *lock_resilient(&PRESENTED) = vec![a.clone(), b.clone()];
        assert_eq!(get_active_alarms(), vec![a, b]);
    }

    #[test]
    fn a_card_without_an_occurrence_key_is_never_matched() {
        // Defensive: a malformed payload must not be treated as belonging to some
        // occurrence and silently dropped by an unrelated dismiss.
        let cards = vec![serde_json::json!({"kind": "t_zero"}), card("(A @ t)", "t_zero")];
        assert_eq!(without_occurrence(&cards, "(A @ t)").len(), 1, "the keyless card survives");
        assert_eq!(key_of(&cards[0]), None);
    }
}
