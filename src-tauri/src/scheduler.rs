//! Alarm scheduler (Phase 1d spike → grows into the production engine, PLAN §1).
//!
//! Core principles (both verified by adversarial review + Apple docs):
//!   - Arm against WALL-CLOCK, never duration sleeps: mach timers pause in sleep
//!     and App Nap stretches them; we re-check the target time on short ticks.
//!   - Timer precision in a background Accessory app requires an
//!     `NSActivityLatencyCritical` assertion — `.userInitiated` alone does NOT
//!     defeat App Nap timer throttling. Held WINDOWED (≤2 min before fire), not 24/7.
//!
//! CP1d spike: ENTUCARA_SPIKE_FIRE="<delay_secs>,<arm>" with arm ∈
//! none | userinitiated | latencycritical. Arms an alarm <delay_secs> out, holds the
//! requested assertion, fires, and appends a latency record to
//! ~/Library/Application Support/dev.fforres.entucara/fire-spike.jsonl, then exits.

#![cfg(target_os = "macos")]

use chrono::{DateTime, Duration as ChronoDuration, Utc};
use objc2::rc::Retained;
use objc2::runtime::NSObjectProtocol;
use objc2_foundation::{NSActivityOptions, NSProcessInfo, NSString};
use serde::Serialize;
use std::time::Duration;

/// RAII holder for an NSProcessInfo activity assertion.
pub struct ActivityAssertion {
    token: Retained<objc2::runtime::ProtocolObject<dyn NSObjectProtocol>>,
}

impl ActivityAssertion {
    pub fn begin(options: NSActivityOptions, reason: &str) -> Self {
        let info = NSProcessInfo::processInfo();
        let token = info.beginActivityWithOptions_reason(options, &NSString::from_str(reason));
        Self { token }
    }
}

impl Drop for ActivityAssertion {
    fn drop(&mut self) {
        let info = NSProcessInfo::processInfo();
        // SAFETY: token came from beginActivityWithOptions_reason on this process
        // and is ended exactly once (RAII).
        unsafe { info.endActivity(&self.token) };
    }
}

#[derive(Debug, Serialize)]
struct FireSpikeRecord {
    arm: String,
    scheduled_for: DateTime<Utc>,
    fired_at: DateTime<Utc>,
    latency_ms: i64,
    delay_secs: i64,
    on_battery: bool,
    macos: String,
}

fn on_battery() -> bool {
    std::process::Command::new("pmset")
        .args(["-g", "batt"])
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).contains("Battery Power"))
        .unwrap_or(false)
}

fn macos_version() -> String {
    std::process::Command::new("sw_vers")
        .arg("-productVersion")
        .output()
        .map(|o| String::from_utf8_lossy(&o.stdout).trim().to_string())
        .unwrap_or_default()
}

/// Wall-clock-targeted wait: short ticks, re-checking the target each time.
/// Under App Nap the TICKS get stretched — which is exactly the latency we measure.
/// (Production will also re-arm on NSWorkspace didWake; spike keeps it minimal.)
fn wait_until(target: DateTime<Utc>) {
    loop {
        let now = Utc::now();
        if now >= target {
            return;
        }
        let remaining = (target - now).num_milliseconds();
        // Coarse ticks far out, fine ticks close in.
        let tick = if remaining > 10_000 { 1_000 } else { 50 };
        std::thread::sleep(Duration::from_millis(tick.min(remaining.max(1)) as u64));
    }
}

/// CP1d: ENTUCARA_SPIKE_FIRE="<delay_secs>,<arm>" → measure fire latency, exit app.
pub fn maybe_run_fire_spike(app: &tauri::AppHandle) {
    let Ok(spec) = std::env::var("ENTUCARA_SPIKE_FIRE") else {
        return;
    };
    let (delay_str, arm) = spec.split_once(',').unwrap_or((spec.as_str(), "none"));
    let delay_secs: i64 = delay_str.parse().unwrap_or(60);
    let arm = arm.to_lowercase();
    let app = app.clone();

    std::thread::spawn(move || {
        let scheduled_for = Utc::now() + ChronoDuration::seconds(delay_secs);

        // Hold the requested assertion for the whole wait. (Production holds it
        // windowed ≤2 min before fire; the spike holds it throughout so the arm
        // under test is unambiguous.)
        let _assertion = match arm.as_str() {
            "userinitiated" => Some(ActivityAssertion::begin(
                NSActivityOptions::UserInitiated,
                "en-tu-cara fire spike: userInitiated",
            )),
            "latencycritical" => Some(ActivityAssertion::begin(
                NSActivityOptions::UserInitiated | NSActivityOptions::LatencyCritical,
                "en-tu-cara fire spike: userInitiated|latencyCritical",
            )),
            _ => None,
        };

        println!(
            "SPIKE_FIRE armed: arm={arm} delay={delay_secs}s target={}",
            scheduled_for.to_rfc3339()
        );
        wait_until(scheduled_for);
        let fired_at = Utc::now();

        let record = FireSpikeRecord {
            arm: arm.clone(),
            scheduled_for,
            fired_at,
            latency_ms: (fired_at - scheduled_for).num_milliseconds(),
            delay_secs,
            on_battery: on_battery(),
            macos: macos_version(),
        };
        println!("SPIKE_FIRE fired: {}", serde_json::to_string(&record).unwrap());

        if let Ok(dir) = app.path().app_data_dir() {
            let _ = std::fs::create_dir_all(&dir);
            if let Ok(mut f) = std::fs::OpenOptions::new()
                .create(true)
                .append(true)
                .open(dir.join("fire-spike.jsonl"))
            {
                use std::io::Write;
                let _ = writeln!(f, "{}", serde_json::to_string(&record).unwrap());
            }
        }
        app.exit(0);
    });
}

use tauri::Manager;
