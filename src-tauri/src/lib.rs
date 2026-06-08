//! En Tu Cara — unmissable meeting alerts for macOS. Fully local (EventKit only).
//! Architecture: see PLAN.md.

mod alarm_core;
#[cfg(target_os = "macos")]
mod calendar;
#[cfg(target_os = "macos")]
mod overlay;
#[cfg(target_os = "macos")]
mod scheduler;
mod paths;
mod settings;
#[cfg(target_os = "macos")]
mod sound;
mod state;
mod testmode;
mod tray;

use tauri::Manager;

pub fn run() {
    // Skyward data dir (~/.config/skyward/en-tu-cara) for logs + exports.
    paths::ensure();

    tauri::Builder::default()
        .plugin(
            tauri_plugin_log::Builder::new()
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Folder {
                        path: paths::logs_dir(),
                        file_name: Some("en-tu-cara".to_string()),
                    },
                ))
                .target(tauri_plugin_log::Target::new(
                    tauri_plugin_log::TargetKind::Stdout,
                ))
                .level(log::LevelFilter::Info)
                .build(),
        )
        .plugin(tauri_plugin_single_instance::init(|app, _argv, _cwd| {
            // Second launch → open settings on the already-running instance.
            let _ = tray::open_settings(app.clone());
        }))
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_nspanel::init())
        .plugin(tauri_plugin_store::Builder::new().build())
        .plugin(tauri_plugin_updater::Builder::new().build())
        .plugin(tauri_plugin_process::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .invoke_handler(tauri::generate_handler![
            testmode::set_mock_now,
            testmode::advance_clock,
            testmode::get_fired_log,
            calendar::calendar_authorization_status,
            calendar::request_calendar_access,
            calendar::list_calendars,
            calendar::fetch_events,
            overlay::spike_show_overlays,
            overlay::close_overlays,
            scheduler::inject_events,
            scheduler::get_active_alarms,
            scheduler::demo_alert,
            scheduler::snooze_alarm,
            scheduler::dismiss_alarms,
            scheduler::set_paused,
            scheduler::get_paused,
            tray::open_settings,
            tray::maybe_show_onboarding,
            tray::finish_onboarding,
            tray::open_url,
            tray::hide_popover,
            settings::get_settings,
            settings::set_settings,
            settings::preview_sound,
            settings::list_system_sounds,
        ])
        .setup(|app| {
            log::info!(
                "En Tu Cara v{} started — data dir {}",
                app.package_info().version,
                paths::data_dir().display()
            );

            // Menu-bar agent: no Dock icon, no Cmd-Tab entry, never steals focus on
            // launch. Info.plist LSUIElement covers packaged builds; this covers dev.
            #[cfg(target_os = "macos")]
            app.set_activation_policy(tauri::ActivationPolicy::Accessory);

            tray::setup(app.handle())?;

            // CP1a spike automation: dump calendars + events as JSON and keep running.
            // cp1a-auto.sh launches the packaged app with this env var, reads the file,
            // validates the schema, and asserts permission persistence across relaunch.
            #[cfg(target_os = "macos")]
            if std::env::var("ENTUCARA_SPIKE_DUMP").is_ok_and(|v| v == "1") {
                let dir = app.path().app_data_dir()?;
                std::fs::create_dir_all(&dir)?;
                let dump = calendar::spike_dump();
                let path = dir.join("spike-dump.json");
                std::fs::write(&path, serde_json::to_vec_pretty(&dump)?)?;
                println!("ENTUCARA_SPIKE_DUMP written: {}", path.display());
            }

            // CP1b spike: ENTUCARA_SPIKE_OVERLAY=<secs> → timed takeover test.
            #[cfg(target_os = "macos")]
            overlay::maybe_run_spike(app.handle());

            // CP1d spike: ENTUCARA_SPIKE_FIRE="<secs>,<arm>" → fire-latency test.
            #[cfg(target_os = "macos")]
            scheduler::maybe_run_fire_spike(app.handle());

            // Visual checks / dev convenience: open the settings window on launch
            // (after the 2s setup-grace — window creation during setup is the
            // known abort trap).
            if let Ok(section_env) = std::env::var("ENTUCARA_OPEN_SETTINGS") {
                let handle = app.handle().clone();
                std::thread::spawn(move || {
                    std::thread::sleep(std::time::Duration::from_secs(3));
                    let h = handle.clone();
                    let section = (section_env != "1").then_some(section_env);
                    let dispatched = handle.run_on_main_thread(move || {
                        eprintln!("OPEN_SETTINGS: invoking");
                        match tray::open_settings_at(h, section.as_deref()) {
                            Ok(()) => eprintln!("OPEN_SETTINGS: ok"),
                            Err(e) => eprintln!("OPEN_SETTINGS: ERR {e}"),
                        }
                    });
                    if dispatched.is_err() {
                        eprintln!("OPEN_SETTINGS: main-thread dispatch failed");
                    }
                });
            }

            // Real-calendar e2e: ENTUCARA_SPIKE_REAL_E2E="<start_in_secs>".
            #[cfg(target_os = "macos")]
            calendar::maybe_run_real_e2e();

            // Persisted alarm state + settings + the production scheduler loop.
            // Persist state + settings in the Skyward data dir (~/.config/skyward/
            // en-tu-cara), not ~/Library — no existing users to migrate.
            let data_dir = paths::data_dir();
            app.manage(state::SharedState::load(data_dir.clone()));
            let settings_store = settings::SettingsStore::load(data_dir);
            // Apply the saved tray-icon style now that settings are loaded (the
            // tray was built with the template default in tray::setup above).
            tray::apply_tray_icon(app.handle(), &settings_store.get().tray_icon);
            app.manage(settings_store);
            #[cfg(target_os = "macos")]
            scheduler::spawn_loop(app.handle());

            // Launch at login per settings (default on). Skipped in test mode AND
            // in ALL debug builds: a dev binary must never register a LaunchAgent
            // pointing at target/debug/en-tu-cara. Enabling autostart loads that
            // agent (RunAtLoad=true), which immediately spawns a SECOND copy of the
            // dev binary; it loses the single-instance race and exits, so the tray
            // flickers up and the app "closes on its own". Only the packaged release
            // build manages autostart.
            #[cfg(not(debug_assertions))]
            if !testmode::is_test_mode() {
                use tauri_plugin_autostart::ManagerExt as _;
                let wanted = app.state::<settings::SettingsStore>().get().launch_at_login;
                let _ = if wanted {
                    app.autolaunch().enable()
                } else {
                    app.autolaunch().disable()
                };
            }

            Ok(())
        })
        .on_window_event(|window, event| {
            // Settings closed → back to dock-less Accessory (see open_settings).
            #[cfg(target_os = "macos")]
            if window.label() == "settings" {
                if let tauri::WindowEvent::CloseRequested { .. } = event {
                    let _ = window
                        .app_handle()
                        .set_activation_policy(tauri::ActivationPolicy::Accessory);
                }
            }
            #[cfg(not(target_os = "macos"))]
            let _ = (window, event);
        })
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
