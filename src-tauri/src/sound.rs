//! Native alert sound via NSSound (PLAN §1: never webview audio — WKWebView
//! autoplay gating makes it unfit for an alarm).
//!
//! The alert sound is RECURRING (user requirement): it repeats for as long as
//! overlay panels are up, and stops the instant they're dismissed/snoozed.
//! Lifecycle is owned by overlay::show_overlays / close_overlays.

#![cfg(target_os = "macos")]

use objc2_app_kit::NSSound;
use objc2_foundation::NSString;
use std::sync::atomic::{AtomicBool, Ordering};

pub const DEFAULT_ALERT_SOUND: &str = "Sosumi";
const REPEAT_EVERY_SECS: u64 = 4;

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
    if ALERTING.swap(true, Ordering::SeqCst) {
        return; // already looping
    }
    let app = app.clone();
    std::thread::spawn(move || {
        while ALERTING.load(Ordering::SeqCst) {
            let _ = app.run_on_main_thread(|| play(DEFAULT_ALERT_SOUND));
            std::thread::sleep(std::time::Duration::from_secs(REPEAT_EVERY_SECS));
        }
    });
}

/// Stop the repeating alert sound (dismiss/snooze/overlay close).
pub fn stop_alert_loop() {
    ALERTING.store(false, Ordering::SeqCst);
}
