//! User settings: typed, persisted, live-applied (PLAN Phase 7).
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
    /// Default OFF: hiding an unactioned alarm contradicts "never miss" (PLAN §1).
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
        let current = std::fs::read(&path)
            .ok()
            .and_then(|bytes| serde_json::from_slice(&bytes).ok())
            .unwrap_or_default();
        Self { current: Mutex::new(current), path }
    }

    pub fn get(&self) -> Settings {
        self.current.lock().unwrap().clone()
    }

    pub fn update<R>(&self, f: impl FnOnce(&mut Settings) -> R) -> R {
        let mut guard = self.current.lock().unwrap();
        let result = f(&mut guard);
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        if let Ok(json) = serde_json::to_vec_pretty(&*guard) {
            let _ = std::fs::write(&self.path, json);
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
    store.update(|s| *s = settings.clone());

    // Live-apply: launch at login.
    #[cfg(target_os = "macos")]
    if previous.launch_at_login != settings.launch_at_login && !crate::testmode::is_test_mode() {
        use tauri_plugin_autostart::ManagerExt as _;
        let result = if settings.launch_at_login {
            app.autolaunch().enable()
        } else {
            app.autolaunch().disable()
        };
        if let Err(e) = result {
            return Err(format!("autostart: {e}"));
        }
    }

    // Live-apply: swap the menu-bar icon style immediately (no restart).
    if previous.tray_icon != settings.tray_icon {
        crate::tray::apply_tray_icon(&app, &settings.tray_icon);
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
        assert!(!s.auto_close_enabled, "auto-close defaults OFF (PLAN §1)");
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
    fn corrupt_file_falls_back_to_defaults() {
        let dir = std::env::temp_dir().join(format!("entucara-corrupt-{}", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        std::fs::write(dir.join("settings.json"), b"{not json").unwrap();
        let store = SettingsStore::load(dir.clone());
        assert_eq!(store.get(), Settings::default());
        let _ = std::fs::remove_dir_all(dir);
    }
}
