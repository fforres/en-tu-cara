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
    /// Pre-event reminders: minutes before start, in display order. 0–3 entries.
    /// Empty is allowed — the user can remove every pre-event reminder, and only
    /// the mandatory event-start alert will fire. Default is a single 5-min reminder.
    pub reminders: Vec<u32>,
    /// System sound name (from /System/Library/Sounds).
    pub alert_sound: String,
    /// Seconds between sound repeats while the overlay is up.
    pub sound_repeat_secs: u64,
    /// Default snooze duration in minutes, used by the "Remind me again" action.
    /// Independent from the reminder schedule (`reminders`).
    pub default_snooze_minutes: u32,
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
        self.auto_close_minutes = self.auto_close_minutes.clamp(1, 24 * 60);
        self.menu_bar_title_chars = self.menu_bar_title_chars.clamp(4, 60);
        // Reminders: each offset in 1..=120 min, at most 3. An EMPTY list is a
        // valid choice (no pre-event reminders — the mandatory start alarm still
        // fires), so unlike the old snooze list it is NOT repaired to a default.
        self.reminders.retain(|&m| (1..=120).contains(&m));
        self.reminders.truncate(3);
        // Default snooze is a single value; never let it drop below 1 minute
        // (a 0-minute snooze would re-fire instantly) or exceed a sane ceiling.
        self.default_snooze_minutes = self.default_snooze_minutes.clamp(1, 120);
        self
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            enabled_calendar_ids: None,
            reminders: vec![5],
            alert_sound: "Sosumi".into(),
            sound_repeat_secs: 4,
            default_snooze_minutes: 5,
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
        assert_eq!(s.reminders, vec![5], "one 5-minute pre-event reminder by default");
        assert_eq!(s.default_snooze_minutes, 5);
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
            s.reminders = vec![2, 30];
            s.default_snooze_minutes = 20;
            s.enabled_calendar_ids = Some(vec!["cal-1".into()]);
        });
        let reloaded = SettingsStore::load(dir.clone());
        assert_eq!(reloaded.get().reminders, vec![2, 30]);
        assert_eq!(reloaded.get().default_snooze_minutes, 20);
        assert_eq!(reloaded.get().enabled_calendar_ids, Some(vec!["cal-1".into()]));
        let _ = std::fs::remove_dir_all(dir);
    }

    #[test]
    fn partial_and_unknown_fields_tolerated() {
        // Old file with one known field + one unknown → defaults fill the rest.
        let parsed: Settings =
            serde_json::from_str(r#"{"alert_sound": "Basso", "some_future_field": true}"#).unwrap();
        assert_eq!(parsed.alert_sound, "Basso");
        assert_eq!(parsed.reminders, vec![5], "missing reminders → default");
        assert_eq!(parsed.default_snooze_minutes, 5, "missing snooze → default");
    }

    #[test]
    fn legacy_lead_and_snooze_fields_migrate_to_defaults() {
        // A settings.json written by an OLD version had `lead_minutes: u32` and
        // `snooze_minutes: Vec<u32>`. Those names no longer exist, so they parse as
        // unknown fields (ignored) and the new fields take their defaults — without
        // failing the whole parse and wiping every OTHER setting.
        let parsed: Settings = serde_json::from_str(
            r#"{"lead_minutes": 10, "snooze_minutes": [1, 5], "alert_sound": "Basso"}"#,
        )
        .unwrap();
        assert_eq!(parsed.reminders, vec![5], "legacy lead_minutes → default reminders");
        assert_eq!(parsed.default_snooze_minutes, 5, "legacy snooze array → default snooze");
        assert_eq!(parsed.alert_sound, "Basso", "unrelated settings survive the migration");
    }

    #[test]
    fn sanitized_clamps_out_of_range_values() {
        let bad = Settings {
            sound_repeat_secs: 0,        // would mean "spam continuously"
            reminders: vec![9999],       // absurd early-alert horizon
            default_snooze_minutes: 0,   // would re-fire instantly
            auto_close_minutes: 0,       // would auto-close instantly
            menu_bar_title_chars: 1,     // truncates the title to nothing
            ..Settings::default()
        };
        let s = bad.sanitized();
        assert!(s.sound_repeat_secs >= 2, "sound repeat clamped to a sane floor");
        assert!(s.reminders.iter().all(|&m| m <= 120), "reminder offsets bounded");
        assert!(s.default_snooze_minutes >= 1, "default snooze floored to 1 minute");
        assert!(s.auto_close_minutes >= 1, "auto-close minutes has a floor");
        assert!(s.menu_bar_title_chars >= 4, "title chars floored so titles stay legible");
    }

    #[test]
    fn sanitized_allows_empty_reminders() {
        // Removing every pre-event reminder is a valid choice (only the mandatory
        // start alert fires). Unlike the old snooze list, it is NOT repaired.
        let s = Settings { reminders: vec![], ..Settings::default() }.sanitized();
        assert!(s.reminders.is_empty(), "empty reminders is preserved, not repaired");
    }

    #[test]
    fn sanitized_drops_bad_reminders_and_caps_at_three() {
        let s = Settings {
            reminders: vec![0, 1, 5, 20, 60], // a 0 (dropped) and too many (capped to 3)
            ..Settings::default()
        }
        .sanitized();
        assert!(!s.reminders.contains(&0), "0-minute reminder removed");
        assert!(s.reminders.len() <= 3, "reminder list capped at three");
        assert_eq!(s.reminders, vec![1, 5, 20], "kept in order after dropping the 0");
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
