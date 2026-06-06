//! Native alert sound via NSSound (PLAN §1: never webview audio — WKWebView
//! autoplay gating makes it unfit for an alarm).

#![cfg(target_os = "macos")]

use objc2_app_kit::NSSound;
use objc2_foundation::NSString;

/// Play a named system sound (from /System/Library/Sounds). Must run on the
/// main thread. Defaults match the product's "unmissable" brief.
pub fn play(name: &str) {
    let ns_name = NSString::from_str(name);
    if let Some(sound) = NSSound::soundNamed(&ns_name) {
        sound.play();
    }
}

pub const DEFAULT_ALERT_SOUND: &str = "Sosumi";
