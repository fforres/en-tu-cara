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

/// Start the repeating alert sound. Idempotent — a second call while already
/// alerting does nothing (T-0 firing while the T-5 overlay is still up).
pub fn start_alert_loop(app: &tauri::AppHandle) {
    // Checkpoint scripts run dozens of overlay cycles — spare the human's ears.
    if std::env::var("ENTUCARA_SILENT").is_ok_and(|v| v == "1") {
        return;
    }
    if ALERTING.swap(true, Ordering::SeqCst) {
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
