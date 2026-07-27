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

/// The side of the takeover this module drives: panels + sound + webview events.
///
/// A trait, not direct `overlay::`/`emit` calls, for ONE reason — testability. The
/// rules below (never show for an empty card set, never add a card the panels
/// didn't accept, never close while cards remain) are the entire safety story of
/// the takeover, and every one of them is about WHICH surface calls happen in
/// WHICH order. Against a real `AppHandle` those calls need a window server and a
/// running app, so they could only ever be verified by hand; behind this seam a
/// test asserts the exact call sequence. `Real` is the production implementation
/// and holds no logic of its own.
pub trait Surface {
    /// Ensure panels are up, correctly placed, and the alert loop is running.
    fn show(&self) -> Result<(), String>;
    /// Close every panel and silence the alert.
    fn close(&self);
    /// Tell live overlay webviews about a change to the card set.
    fn notify(&self, event: &str, payload: &serde_json::Value);
}

/// The production surface: Tauri panels + the native sound loop.
pub struct Real<'a>(pub &'a AppHandle);

impl Surface for Real<'_> {
    fn show(&self) -> Result<(), String> {
        crate::overlay::show_overlays(self.0).map(|_| ()).map_err(|e| e.to_string())
    }
    fn close(&self) {
        crate::overlay::close_overlays(self.0);
    }
    fn notify(&self, event: &str, payload: &serde_json::Value) {
        let _ = self.0.emit(event, payload);
    }
}

/// Put a fired alarm on screen. MAIN THREAD ONLY.
///
/// Panels first, card second, and that order is load-bearing: the scheduler marks
/// an alarm fired only when this returns `Ok`, so on a failed show the alarm stays
/// unmarked and the next tick retries it. Adding the card before knowing the panels
/// are up would leave that retry to push a SECOND card for the same alarm.
pub fn present_on(surface: &impl Surface, card: serde_json::Value) -> Result<(), String> {
    surface.show()?;
    lock_resilient(&PRESENTED).push(card.clone());
    // Panels already booted get the push; freshly created ones pull on mount.
    surface.notify("alarm-fired", &card);
    Ok(())
}

/// Re-establish the overlay for cards that are still on screen. MAIN THREAD ONLY.
///
/// The sleep/wake self-heal, run every scheduler tick. A fired alarm stays
/// presented until the user actions it, but its panels can be torn down or
/// stranded under it — most importantly across SYSTEM SLEEP, where display
/// reconfiguration on wake leaves the panels on coordinates no screen covers while
/// the sound loop keeps beating. `show` re-frames and re-fronts what survived and
/// rebuilds what didn't; it is a cheap no-op when everything is already correct.
///
/// The emptiness check is the whole safety story, and it must happen HERE rather
/// than only at the scheduler's dispatch site: a dismiss landing between the two
/// would otherwise have us rebuild panels for an empty card set and restart the
/// sound — resurrecting an alarm the user just dismissed.
pub fn reassert_on(surface: &impl Surface) {
    if !any_presented() {
        return;
    }
    if let Err(e) = surface.show() {
        log::error!("overlay self-heal re-assert failed: {e}");
    }
}

/// Take ONE occurrence off screen (dismiss / snooze / ignore of a single card).
pub fn finish_on(surface: &impl Surface, occurrence_key: &str) {
    let remaining = {
        let mut cards = lock_resilient(&PRESENTED);
        *cards = without_occurrence(&cards, occurrence_key);
        cards.clone()
    };
    settle(surface, remaining);
}

/// Take EVERYTHING off screen — Esc, and the zero-card safety Dismiss. The blunt
/// "get it all off my screen" escape hatch.
pub fn finish_all_on(surface: &impl Surface) {
    lock_resilient(&PRESENTED).clear();
    settle(surface, Vec::new());
}

/// Bring the panels in line with what is left: nothing → tear the takeover down;
/// otherwise re-render the still-open overlay against the reduced set.
///
/// The one place panels are derived from cards on removal, so "cards gone, panels
/// up" can't be built by accident.
fn settle(surface: &impl Surface, remaining: Vec<serde_json::Value>) {
    if remaining.is_empty() {
        surface.close();
    } else {
        surface.notify("alarms-updated", &serde_json::Value::Array(remaining));
    }
}

// --- The AppHandle-facing API the rest of the crate calls. ---

pub fn present(app: &AppHandle, card: serde_json::Value) -> Result<(), String> {
    present_on(&Real(app), card)
}

pub fn reassert(app: &AppHandle) {
    reassert_on(&Real(app));
}

pub fn finish(app: &AppHandle, occurrence_key: &str) {
    finish_on(&Real(app), occurrence_key);
}

pub fn finish_all(app: &AppHandle) {
    finish_all_on(&Real(app));
}


#[cfg(test)]
mod tests {
    use super::*;

    /// A stand-in for the real panels that RECORDS what was asked of it.
    ///
    /// Every rule this module enforces is about which surface calls happen, and in
    /// which order — so the call log IS the assertion. Against a real `AppHandle`
    /// none of this is reachable without a window server, which is exactly why the
    /// takeover's safety rules went untested until this seam existed.
    struct Fake {
        calls: Mutex<Vec<String>>,
        show_fails: bool,
    }

    impl Fake {
        fn working() -> Self {
            Self { calls: Mutex::new(Vec::new()), show_fails: false }
        }
        /// A surface that cannot bring panels up (no window server, ObjC failure).
        fn broken() -> Self {
            Self { calls: Mutex::new(Vec::new()), show_fails: true }
        }
        fn calls(&self) -> Vec<String> {
            lock_resilient(&self.calls).clone()
        }
        /// Drop the log so a later assertion sees only the calls it cares about.
        fn forget(&self) {
            lock_resilient(&self.calls).clear();
        }
    }

    impl Surface for Fake {
        fn show(&self) -> Result<(), String> {
            lock_resilient(&self.calls).push("show".into());
            if self.show_fails {
                return Err("no window server".into());
            }
            Ok(())
        }
        fn close(&self) {
            lock_resilient(&self.calls).push("close".into());
        }
        fn notify(&self, event: &str, payload: &serde_json::Value) {
            let n = payload.as_array().map_or(1, |a| a.len());
            lock_resilient(&self.calls).push(format!("notify:{event}:{n}"));
        }
    }

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

    // ---------------------------------------------------------------------
    // The resurrection bug: an overlay must NEVER come back after a dismiss.
    // ---------------------------------------------------------------------

    #[test]
    fn a_dismiss_racing_the_reassert_never_reopens_the_takeover() {
        // The exact interleaving that shipped broken: the scheduler sees a card on
        // its own thread and queues a re-assert onto the main thread; the user
        // dismisses in the gap. If the queued re-assert still calls show(), the
        // user gets rebuilt panels with ZERO cards and the sound loop restarts —
        // an alarm they just dismissed, and (before the card set was the source of
        // truth) no way to dismiss it again.
        let _x = exclusive();
        lock_resilient(&PRESENTED).push(card("(A @ t)", "t_zero"));
        assert!(any_presented(), "precondition: the scheduler had a reason to dispatch");

        let surface = Fake::working();
        finish_all_on(&surface); // …the dismiss lands first.
        surface.forget(); // ignore the dismiss's own close()

        reassert_on(&surface); // the in-flight re-assert finally runs

        assert!(
            surface.calls().is_empty(),
            "the re-assert must not touch the panels at all; got {:?}",
            surface.calls()
        );
    }

    #[test]
    fn a_dismissed_takeover_stays_down_across_many_later_ticks() {
        // Not just the racing tick — EVERY subsequent one. The self-heal runs on a
        // ≤30s cadence forever, so a single missing guard means the alarm returns
        // again and again until the app is force-quit (the original report).
        let _x = exclusive();
        lock_resilient(&PRESENTED).push(card("(A @ t)", "t_zero"));
        let surface = Fake::working();
        finish_on(&surface, "(A @ t)");
        surface.forget();

        for _ in 0..5 {
            reassert_on(&surface);
        }
        assert!(
            surface.calls().is_empty(),
            "a dismissed alarm must stay dismissed; got {:?}",
            surface.calls()
        );
    }

    #[test]
    fn a_reassert_rebuilds_the_takeover_while_a_card_is_still_up() {
        // The other direction, and the reason the feature exists: an undismissed
        // alarm whose panels were lost across sleep MUST get them back. A guard so
        // eager that it never heals would pass the tests above and still leave the
        // user with an audible, invisible alarm.
        let _x = exclusive();
        lock_resilient(&PRESENTED).push(card("(A @ t)", "t_zero"));

        let surface = Fake::working();
        reassert_on(&surface);

        assert_eq!(surface.calls(), ["show"], "the panels must be re-asserted");
    }

    #[test]
    fn a_reassert_survives_a_surface_that_cannot_show() {
        // A failing show must not panic the scheduler tick (it runs inside
        // catch_unwind, but a panic there costs a tick) and must leave the card in
        // place so the NEXT tick tries again.
        let _x = exclusive();
        lock_resilient(&PRESENTED).push(card("(A @ t)", "t_zero"));

        let surface = Fake::broken();
        reassert_on(&surface);

        assert_eq!(surface.calls(), ["show"]);
        assert!(any_presented(), "the card stays so the next tick retries");
    }

    // ---------------------------------------------------------------------
    // Firing: a card may exist only once its panels are confirmed up.
    // ---------------------------------------------------------------------

    #[test]
    fn a_failed_show_records_no_card_so_the_retry_cannot_duplicate_it() {
        // The scheduler marks an alarm fired only when present() returns Ok, so a
        // failed show leaves it unmarked and compute_actions re-fires it next tick.
        // If the card had been recorded anyway, that retry would add a SECOND card
        // for one alarm — two identical cards stacked on the takeover.
        let _x = exclusive();
        let surface = Fake::broken();

        assert!(present_on(&surface, card("(A @ t)", "t_zero")).is_err());

        assert!(get_active_alarms().is_empty(), "no card for an alert that never appeared");
        assert_eq!(
            surface.calls(),
            ["show"],
            "and no alarm-fired event either — the webview must not hear about it"
        );
    }

    #[test]
    fn present_brings_the_panels_up_before_it_records_the_card() {
        // Order is the invariant, so the call log is the assertion.
        let _x = exclusive();
        let surface = Fake::working();

        present_on(&surface, card("(A @ t)", "t_zero")).expect("show works");

        assert_eq!(surface.calls(), ["show", "notify:alarm-fired:1"]);
        assert_eq!(get_active_alarms().len(), 1, "the card is recorded for a rebuilt panel to pull");
    }

    // ---------------------------------------------------------------------
    // Finishing: panels come down exactly when the last card does.
    // ---------------------------------------------------------------------

    #[test]
    fn finishing_one_of_two_keeps_the_takeover_up_and_broadcasts_the_rest() {
        // Two overlapping meetings. Actioning one must NOT close the overlay — the
        // old code cleared everything on any action, taking the other meeting's
        // alert down with it.
        let _x = exclusive();
        *lock_resilient(&PRESENTED) = vec![card("(A @ t)", "t_zero"), card("(B @ t)", "t_zero")];

        let surface = Fake::working();
        finish_on(&surface, "(A @ t)");

        assert_eq!(
            surface.calls(),
            ["notify:alarms-updated:1"],
            "re-render with B only, and NO close"
        );
        assert!(is_presented("(B @ t)"), "B is still on screen");
        assert!(!is_presented("(A @ t)"));
    }

    #[test]
    fn finishing_a_meetings_last_card_drops_its_other_cards_too() {
        // A meeting can have a reminder AND a T-0 card up. Actioning it must clear
        // both, or the takeover stays up showing a stale card for a handled meeting.
        let _x = exclusive();
        *lock_resilient(&PRESENTED) = vec![
            card("(A @ t)", "reminder_5"),
            card("(A @ t)", "t_zero"),
            card("(B @ t)", "t_zero"),
        ];

        let surface = Fake::working();
        finish_on(&surface, "(A @ t)");

        assert_eq!(surface.calls(), ["notify:alarms-updated:1"]);
        assert!(!is_presented("(A @ t)"), "BOTH of A's cards are gone");
    }

    #[test]
    fn finishing_the_last_card_takes_the_takeover_down_and_silences_it() {
        let _x = exclusive();
        lock_resilient(&PRESENTED).push(card("(A @ t)", "t_zero"));

        let surface = Fake::working();
        finish_on(&surface, "(A @ t)");

        assert_eq!(surface.calls(), ["close"], "panels closed and the alert silenced");
        assert!(get_active_alarms().is_empty());
    }

    #[test]
    fn finish_all_clears_every_card_and_closes_once() {
        // Esc / the zero-card safety Dismiss. One close, not one per card.
        let _x = exclusive();
        *lock_resilient(&PRESENTED) = vec![card("(A @ t)", "t_zero"), card("(B @ t)", "t_zero")];

        let surface = Fake::working();
        finish_all_on(&surface);

        assert_eq!(surface.calls(), ["close"]);
        assert!(get_active_alarms().is_empty());
    }

    #[test]
    fn finishing_an_occurrence_that_is_not_on_screen_leaves_the_takeover_alone() {
        // The ignore path can fire for a meeting that isn't presented. That must not
        // close a takeover showing something else.
        let _x = exclusive();
        lock_resilient(&PRESENTED).push(card("(A @ t)", "t_zero"));

        let surface = Fake::working();
        finish_on(&surface, "(C @ t)");

        assert_eq!(surface.calls(), ["notify:alarms-updated:1"], "A is untouched, nothing closed");
        assert!(is_presented("(A @ t)"));
    }

    // ---------------------------------------------------------------------
    // Reads the rest of the app depends on.
    // ---------------------------------------------------------------------

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
