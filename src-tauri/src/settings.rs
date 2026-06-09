//! User settings: typed, persisted, live-applied.
//!
//! Flat JSON at <Skyward data dir>/settings.json (see paths.rs). Unknown fields
//! are ignored and missing fields take defaults (serde defaults) — old/new app
//! versions can share the file in both directions.

use serde::{Deserialize, Serialize};
use std::path::PathBuf;
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(default)]
pub struct Settings {
    /// None = all calendars alert. Some(ids) = only these calendar ids.
    pub enabled_calendar_ids: Option<Vec<String>>,
    /// Minutes before start for the early alert (the "T-5").
    pub lead_minutes: u32,
    /// System sound name (from /System/Library/Sounds).
    pub alert_sound: String,
    /// Seconds between sound repeats while the overlay is up.
    pub sound_repeat_secs: u64,
    /// Snooze button durations, minutes, in display order.
    pub snooze_minutes: Vec<u32>,
    /// Alert for events you've marked tentative.
    pub alert_tentative: bool,
    /// Alert for invitations you haven't answered yet.
    pub alert_pending: bool,
    /// Only alert for events that carry a video-conference link.
    pub only_video_events: bool,
    /// Show all-day events in the tray list (they never alert either way).
    pub show_all_day_in_tray: bool,
    /// Auto-close an unactioned overlay after `auto_close_minutes`.
    /// Default OFF: hiding an unactioned alarm contradicts "never miss".
    pub auto_close_enabled: bool,
    pub auto_close_minutes: u32,
    pub launch_at_login: bool,
    /// Show the next meeting's title + countdown beside the menu-bar icon.
    pub show_next_event_in_menu_bar: bool,
    /// Truncate the menu-bar title to this many characters.
    pub menu_bar_title_chars: u32,
    /// Overlay theme id (themes.ts is the registry; unknown ids fall back).
    pub theme: String,
    /// Menu-bar tray icon style: "auto" (template, adapts) | "light" | "dark".
    pub tray_icon: String,
    /// Whether the first-run onboarding window has been completed. Drives whether
    /// we show onboarding on launch (see tray::maybe_show_onboarding).
    pub onboarded: bool,
    /// Anonymized usage telemetry → PostHog (see telemetry.rs). Default ON, with a
    /// Settings toggle and the `ENTUCARA_TELEMETRY=off` env kill-switch. Only
    /// behavioral data + hashes ever leave the device — never event titles,
    /// attendees, calendar names, or raw emails.
    pub telemetry_enabled: bool,
    /// Stable, random per-install id used as the telemetry `distinct_id` so JS and
    /// Rust events unify on one device. Generated once on first load (see
    /// `SettingsStore::load`); not a hardware id. Empty string = "not yet minted".
    pub device_id: String,
}

impl Settings {
    /// Clamp/repair values arriving from the UI — the trust boundary in a local
    /// app with no server. Out-of-range input must never break alerting or leave
    /// an empty snooze row. Pure (no I/O) so it's unit-testable; `alert_sound` is
    /// validated separately against the real system list in `set_settings`.
    fn sanitized(mut self) -> Self {
        self.sound_repeat_secs = self.sound_repeat_secs.max(2);
        self.lead_minutes = self.lead_minutes.min(120);
        self.auto_close_minutes = self.auto_close_minutes.clamp(1, 24 * 60);
        self.menu_bar_title_chars = self.menu_bar_title_chars.clamp(4, 60);
        self.snooze_minutes.retain(|&m| m >= 1);
        self.snooze_minutes.truncate(4);
        if self.snooze_minutes.is_empty() {
            self.snooze_minutes = Settings::default().snooze_minutes;
        }
        self
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled_calendar_ids: None,
            lead_minutes: 5,
            alert_sound: "Sosumi".into(),
            sound_repeat_secs: 4,
            snooze_minutes: vec![1, 5],
            alert_tentative: true,
            alert_pending: true,
            only_video_events: false,
            show_all_day_in_tray: true,
            auto_close_enabled: false,
            auto_close_minutes: 15,
            launch_at_login: true,
            show_next_event_in_menu_bar: true,
            menu_bar_title_chars: 20,
            theme: "frost-dark".into(),
            tray_icon: "auto".into(),
            onboarded: false,
            telemetry_enabled: true,
            device_id: String::new(),
        }
    }
}

pub struct SettingsStore {
    current: Mutex<Settings>,
    path: PathBuf,
}

impl SettingsStore {
    pub fn load(dir: PathBuf) -> Self {
        let path = dir.join("settings.json");
        let current = crate::paths::load_json_or_default(&path);
        let store = Self { current: Mutex::new(current), path };
        // Mint the stable telemetry device id once, on first load, and persist it.
        // Done here (not in Default) so it survives across launches via the same
        // atomic-write path as every other setting.
        if store.get().device_id.is_empty() {
            store.update(|s| s.device_id = uuid::Uuid::new_v4().to_string());
        }
        store
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap_or_else(|e| e.into_inner()).clone()
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> R {
        let mut guard = self.current.lock().unwrap_or_else(|e| e.into_inner());
        let result = f(&mut guard);
        if let Ok(json) = serde_json::to_vec_pretty(&*guard) {
            let _ = crate::paths::atomic_write(&self.path, &json);
        }
        result
    }
}

// ---------------------------------------------------------------------------
// Commands
// ---------------------------------------------------------------------------

#[tauri::command]
pub fn get_settings(app: tauri::AppHandle) -> Settings {
    use tauri::Manager;
    app.state::<SettingsStore>().get()
}

/// Replace the whole settings object (the UI sends the full struct — simple and
/// race-free for a single-user local app). Live-applies side effects.
#[tauri::command]
pub fn set_settings(app: tauri::AppHandle, settings: Settings) -> Result<(), String> {
    use tauri::Manager;
    let store = app.state::<SettingsStore>();
    let previous = store.get();
    #[cfg_attr(not(target_os = "macos"), allow(unused_mut))]
    let mut next = settings.sanitized();

    // Validate alert_sound against the real system list: persisting a name no
    // sound file matches would make the ALARM ITSELF silent (a missed alert).
    #[cfg(target_os = "macos")]
    {
        let available = list_system_sounds();
        if !available.is_empty() && !available.contains(&next.alert_sound) {
            next.alert_sound = Settings::default().alert_sound;
        }
    }

    // Apply OS side effects FIRST and bail on failure, so we never persist a
    // launch-at-login value that disagrees with the actual login-item state
    // (the old order wrote disk first, then could fail — UI, disk, and OS all
    // ended up out of sync).
    // `!cfg!(debug_assertions)`: never register a dev binary as a login item (it
    // would spawn a duplicate that fights the single-instance lock). Release only.
    #[cfg(target_os = "macos")]
    if previous.launch_at_login != next.launch_at_login
        && !crate::testmode::is_test_mode()
        && !cfg!(debug_assertions)
    {
        use tauri_plugin_autostart::ManagerExt as _;
        let result = if next.launch_at_login {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        };
        if let Err(e) = result {
            return Err(format!("autostart: {e}"));
        }
    }

    // Side effects succeeded — now persist the sanitized settings.
    store.update(|s| *s = next.clone());

    // Live-apply: swap the menu-bar icon style immediately (no restart). This is
    // best-effort and can't fail loudly, so it runs after persisting.
    if previous.tray_icon != next.tray_icon {
        crate::tray::apply_tray_icon(&app, &next.tray_icon);
    }
    Ok(())
}

/// Preview a sound from the settings UI.
#[tauri::command]
pub fn preview_sound(name: String) {
    #[cfg(target_os = "macos")]
    crate::sound::play(&name);
    #[cfg(not(target_os = "macos"))]
    let _ = name;
}

/// System alert sounds available on every Mac.
#[tauri::command]
pub fn list_system_sounds() -> Vec<String> {
    #[cfg(target_os = "macos")]
    {
        let mut sounds: Vec<String> = std::fs::read_dir("/System/Library/Sounds")
            .map(|entries| {
                entries
                    .filter_map(|e| e.ok())
                    .filter_map(|e| {
                        e.path()
                            .file_stem()
                            .and_then(|s| s.to_str())
                            .map(String::from)
                    })
                    .collect()
            })
            .unwrap_or_default();
        sounds.sort();
        sounds
    }
    #[cfg(not(target_os = "macos"))]
    Vec::new()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn defaults_match_product_spec() {
        let s = Settings::default();
        assert_eq!(s.lead_minutes, 5);
        assert_eq!(s.snooze_minutes, vec![1, 5]);
        assert!(!s.auto_close_enabled, "auto-close defaults OFF");
        assert!(s.alert_tentative && s.alert_pending);
        assert!(s.enabled_calendar_ids.is_none(), "all calendars by default");
        assert_eq!(s.tray_icon, "auto", "tray icon defaults to the adaptive template");
    }

    #[test]
    fn persistence_round_trips() {
        let dir = std::env::temp_dir().join(format!("entucara-settings-{}", std::process::id()));
        let store = SettingsStore::load(dir.clone());
        store.update(|s| {
            s.lead_minutes = 2;
            s.enabled_calendar_ids = Some(vec!["cal-1".into()]);
        });
        let reloaded = SettingsStore::load(dir.clone());
        assert_eq!(reloaded.get().lead_minutes, 2);
        assert_eq!(reloaded.get().enabled_calendar_ids, Some(vec!["cal-1".into()]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn partial_and_unknown_fields_tolerated() {
        // Old file with one known field + one unknown → defaults fill the rest.
        let parsed: Settings =
            serde_json::from_str(r#"{"lead_minutes": 9, "some_future_field": true}"#).unwrap();
        assert_eq!(parsed.lead_minutes, 9);
        assert_eq!(parsed.alert_sound, "Sosumi");
    }

    #[test]
    fn sanitized_clamps_out_of_range_and_repairs_empty_snooze() {
        let bad = Settings {
            sound_repeat_secs: 0,    // would mean "spam continuously"
            lead_minutes: 9999,      // absurd early-alert horizon
            auto_close_minutes: 0,   // would auto-close instantly
            menu_bar_title_chars: 1, // truncates the title to nothing
            snooze_minutes: vec![],  // empty → no snooze buttons render
            ..Settings::default()
        };
        let s = bad.sanitized();
        assert!(s.sound_repeat_secs >= 2, "sound repeat clamped to a sane floor");
        assert!(s.lead_minutes <= 120, "lead minutes bounded");
        assert!(s.auto_close_minutes >= 1, "auto-close minutes has a floor");
        assert!(s.menu_bar_title_chars >= 4, "title chars floored so titles stay legible");
        assert_eq!(s.snooze_minutes, vec![1, 5], "empty snooze list repaired to defaults");
    }

    #[test]
    fn sanitized_drops_zero_snoozes_and_caps_count() {
        let s = Settings {
            snooze_minutes: vec![0, 1, 2, 3, 5, 10], // a 0 and too many
            ..Settings::default()
        }
        .sanitized();
        assert!(!s.snooze_minutes.contains(&0), "0-minute snooze removed");
        assert!(s.snooze_minutes.len() <= 4, "snooze list capped");
    }

    #[test]
    fn sanitized_leaves_good_settings_untouched() {
        let good = Settings::default();
        assert_eq!(good.clone().sanitized(), good, "valid defaults pass through unchanged");
    }

    #[test]
    fn device_id_is_minted_once_and_stable_across_reloads() {
        let dir = std::env::temp_dir().join(format!("entucara-devid-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let first = SettingsStore::load(dir.clone());
        let id = first.get().device_id;
        assert!(!id.is_empty(), "a device id is minted on first load");
        assert_eq!(id.len(), 36, "looks like a UUID v4 (8-4-4-4-12)");
        // Reload from disk: the same id must come back, not a fresh one.
        let second = SettingsStore::load(dir.clone());
        assert_eq!(second.get().device_id, id, "device id is persisted, not regenerated");
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn telemetry_defaults_on() {
        assert!(Settings::default().telemetry_enabled, "telemetry ships on by default");
    }

    #[test]
    fn corrupt_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("entucara-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), b"{not json").unwrap();
        let store = SettingsStore::load(dir.clone());
        // Everything falls back to defaults — except device_id, which load()
        // mints fresh when absent (an empty/corrupt file has none).
        let got = store.get();
        assert!(!got.device_id.is_empty(), "a device id is minted even from a corrupt file");
        assert_eq!(Settings { device_id: String::new(), ..got }, Settings::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
