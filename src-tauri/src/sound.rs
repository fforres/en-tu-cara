//! Native alert sound via NSSound (never webview audio — WKWebView
//! autoplay gating makes it unfit for an alarm).
//!
//! The alert sound is RECURRING (user requirement): it repeats for as long as
//! overlay panels are up, and stops the instant they're dismissed/snoozed.
//! Lifecycle is owned by overlay::show_overlays / close_overlays.

#![cfg(target_os = "macos")]

use objc2_app_kit::NSSound;
use objc2_foundation::NSString;
use std::sync::atomic::{AtomicBool, Ordering};


static ALERTING: AtomicBool = AtomicBool::new(false);

/// Play a named system sound once. Must run on the main thread.
pub fn play(name: &str) {
    let ns_name = NSString::from_str(name);
    if let Some(sound) = NSSound::soundNamed(&ns_name) {
        sound.play();
    }
}

/// Claim the right to run THE alert loop: true for the caller that flips the flag,
/// false for everyone after. The one guarantee that keeps exactly one loop thread
/// alive no matter how often `start_alert_loop` is called — load-bearing now that
/// the scheduler's overlay self-heal re-asserts (and so re-enters this) on EVERY
/// tick while an alarm is presented. N stacked loops would beat N× as fast and
/// each ignore the others' `stop_alert_loop`. Split out for testing.
fn claim_alert_loop() -> bool {
    !ALERTING.swap(true, Ordering::SeqCst)
}

/// Start the repeating alert sound. Idempotent — a second call while already
/// alerting does nothing (T-0 firing while the T-5 overlay is still up, or the
/// scheduler's per-tick overlay re-assert).
pub fn start_alert_loop(app: &tauri::AppHandle) {
    // Checkpoint scripts run dozens of overlay cycles — spare the human's ears.
    if std::env::var("ENTUCARA_SILENT").is_ok_and(|v| v == "1") {
        return;
    }
    if !claim_alert_loop() {
        return; // already looping
    }
    let app = app.clone();
    std::thread::spawn(move || {
        use tauri::Manager as _;
        while ALERTING.load(Ordering::SeqCst) {
            // Read per-iteration: sound/interval changes live-apply mid-alarm.
            let settings = app.state::<crate::settings::SettingsStore>().get();
            let name = settings.alert_sound.clone();
            // Re-check on the main thread before playing: a dismiss between the
            // while-check above and this closure running must NOT emit one last
            // beat after the overlay is already gone ("stops the instant they're
            // dismissed").
            let _ = app.run_on_main_thread(move || {
                if ALERTING.load(Ordering::SeqCst) {
                    play(&name);
                }
            });
            std::thread::sleep(std::time::Duration::from_secs(settings.sound_repeat_secs.max(2)));
        }
    });
}

/// Stop the repeating alert sound (dismiss/snooze/overlay close).
pub fn stop_alert_loop() {
    ALERTING.store(false, Ordering::SeqCst);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_one_alert_loop_can_ever_be_claimed() {
        // The scheduler's overlay self-heal calls start_alert_loop on EVERY tick
        // while an alarm is presented, so this flag is what stands between us and N
        // stacked loop threads — which would beat N× as fast and each ignore the
        // others' stop_alert_loop, leaving a sound the user cannot silence.
        stop_alert_loop(); // known state (the flag is process-global)
        assert!(claim_alert_loop(), "first caller runs the loop");
        for _ in 0..5 {
            assert!(!claim_alert_loop(), "re-entry while alerting must not start a second loop");
        }
        // Dismiss → the next alarm can claim it again (not a one-shot latch).
        stop_alert_loop();
        assert!(claim_alert_loop(), "after a stop, the next alarm starts the loop again");
        stop_alert_loop();
    }
}
