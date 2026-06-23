//! En Tu Cara — unmissable meeting alerts for macOS. Fully local (EventKit only).
//! Architecture: see PLAN.md.

mod access;
mod alarm_core;
mod identity;
mod obs;
#[cfg(target_os = "macos")]
mod calendar;
#[cfg(target_os = "macos")]
mod fire_spike;
#[cfg(target_os = "macos")]
mod grant_repair;
#[cfg(target_os = "macos")]
mod overlay;
#[cfg(target_os = "macos")]
mod scheduler;
#[cfg(target_os = "macos")]
mod snapshot;
mod paths;
mod settings;
#[cfg(target_os = "macos")]
mod sound;
mod state;
mod telemetry;
mod testmode;
mod tray;

use tauri::Manager;

/// Default log verbosity, overridable at launch with `ENTUCARA_LOG=<level>`
/// (off|error|warn|info|debug|trace). Default is Info: quiet enough for a
/// release log, but a user hitting "no events show" can relaunch with
/// `ENTUCARA_LOG=debug` to capture the per-poll `fetch_events`/`list_calendars`
/// timing + count lines (demoted to debug to stop per-30s-tick spam) that were
/// load-bearing in diagnosing the calendar-access saga — no rebuild needed.
fn log_level_from_env() -> log::LevelFilter {
    parse_log_level(std::env::var("ENTUCARA_LOG").ok().as_deref())
}

/// True only for the benign "EventKit returned NULL because the process has no
/// calendar access" read panic — the one we contain in `calendar::guard_eventkit`
/// and must NOT let spam the log on every poll. Deliberately narrow: keyed on
/// objc2's exact message, NOT the source file (which also covers real write-path
/// bugs). A `None` payload (non-string panic) is never swallowed. Pure for tests.
fn is_swallowable_eventkit_null(payload: Option<&str>) -> bool {
    payload.is_some_and(|p| p.contains("unexpected NULL returned"))
}

/// Pure parse so it's unit-testable. Unknown/empty/missing → Info.
fn parse_log_level(value: Option<&str>) -> log::LevelFilter {
    match value.map(str::trim).map(str::to_ascii_lowercase).as_deref() {
        Some("off") => log::LevelFilter::Off,
        Some("error") => log::LevelFilter::Error,
        Some("warn") => log::LevelFilter::Warn,
        Some("debug") => log::LevelFilter::Debug,
        Some("trace") => log::LevelFilter::Trace,
        _ => log::LevelFilter::Info,
    }
}

pub fn run() {
    // Skyward data dir (~/.config/skyward/en-tu-cara) for logs + exports.
    paths::ensure();

    // Observability backbone FIRST, so even early startup logs are captured. One
    // tracing subscriber → rolling local file (always) + WARN+ PostHog events
    // (gated by the opt-out). Bridges existing `log::` calls (see obs.rs).
    obs::init(log_level_from_env());

    // eventkit-rs / objc2-event-kit PANIC (rather than error) when an EventKit
    // call returns NULL — which happens whenever the process has no calendar
    // access (notably a bare `tauri dev` binary, whose TCC grant is keyed to the
    // terminal, not our bundle id; see gotcha #5). We already contain that unwind
    // in calendar::guard_eventkit and degrade to "no events", but catch_unwind
    // does NOT stop the default hook from printing the backtrace first — so it
    // spams the log on every poll.
    //
    // Narrow the swallow to EXACTLY that benign NULL-on-no-access read panic,
    // identified by its payload message ("unexpected NULL returned" — objc2's
    // signature when a non-null return is None). Keying on the source FILE
    // (`objc2-event-kit`) was too broad: it also hid genuine bugs in that crate,
    // e.g. a panic on the real-e2e event-CREATION write path. Every other panic
    // — including other objc2-event-kit panics — goes through the default hook.
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info
            .payload()
            .downcast_ref::<&str>()
            .copied()
            .or_else(|| info.payload().downcast_ref::<String>().map(String::as_str));
        if !is_swallowable_eventkit_null(payload) {
            // Surface genuine panics in telemetry (no-op until init / if disabled).
            // try_send is non-blocking, so this is safe even from a panicking thread.
            crate::telemetry::record(
                "rust_panic",
                serde_json::json!({
                    "message": payload.unwrap_or("non-string panic payload"),
                    "location": info.location().map(|l| format!("{}:{}", l.file(), l.line())),
                }),
            );
            default_hook(info);
        }
    }));

    tauri::Builder::default()
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
            grant_repair::repair_calendar_access,
            calendar::list_calendars,
            calendar::fetch_events,
            overlay::spike_show_overlays,
            overlay::close_overlays,
            scheduler::inject_events,
            scheduler::get_active_alarms,
            scheduler::get_access_state,
            scheduler::demo_alert,
            scheduler::snooze_alarm,
            scheduler::dismiss_alarms,
            scheduler::set_paused,
            scheduler::get_paused,
            scheduler::ignore_occurrence,
            scheduler::unignore_occurrence,
            scheduler::get_ignored,
            tray::open_settings,
            tray::open_feedback,
            tray::maybe_show_onboarding,
            tray::finish_onboarding,
            tray::open_url,
            tray::open_in_calendar,
            tray::hide_popover,
            tray::refresh_popover,
            settings::get_settings,
            settings::set_settings,
            settings::preview_sound,
            settings::list_system_sounds,
            telemetry::telemetry_config,
            telemetry::submit_feedback,
            paths::export_logs,
        ])
        .setup(|app| {
            log::info!(
                "En Tu Cara v{} started — data dir {}",
                app.package_info().version,
                paths::data_dir().display()
            );

            // Front-load the single most common root cause of "no events / no
            // alerts": calendar authorization. Every log file now opens stating
            // FullAccess / Denied / NotDetermined so triage starts here, not
            // three reproductions later.
            #[cfg(target_os = "macos")]
            log::info!(
                "calendar authorization: {}",
                calendar::calendar_authorization_status()
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
            fire_spike::maybe_run_fire_spike(app.handle());

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

            // DEV preview: ENTUCARA_PREVIEW=popover|overlay → open that window's UI
            // in a normal resizable window seeded with mock data (cross-account
            // dedup + the takeover's "Calendar origins", no real calendar access or
            // full-screen takeover needed). Same post-setup-grace dispatch as
            // ENTUCARA_OPEN_SETTINGS (window creation during setup is the known
            // abort trap, gotcha #3).
            if let Ok(kind) = std::env::var("ENTUCARA_PREVIEW") {
                if kind == "popover" || kind == "overlay" {
                    let handle = app.handle().clone();
                    std::thread::spawn(move || {
                        std::thread::sleep(std::time::Duration::from_secs(3));
                        let h = handle.clone();
                        let dispatched = handle.run_on_main_thread(move || {
                            match tray::open_preview(h, &kind) {
                                Ok(()) => eprintln!("PREVIEW: {kind} window opened"),
                                Err(e) => eprintln!("PREVIEW: ERR {e}"),
                            }
                        });
                        if dispatched.is_err() {
                            eprintln!("PREVIEW: main-thread dispatch failed");
                        }
                    });
                }
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
            let onboarded = settings_store.get().onboarded;

            // Anonymized telemetry (PostHog) — opt-out, off in test mode unless
            // ENTUCARA_TELEMETRY=on. Started BEFORE the scheduler so the first
            // ticks' events are captured. The worker runs on its own thread behind
            // a drop-on-full queue and can never stall an alarm (see telemetry.rs).
            telemetry::start(app.handle(), &settings_store.get());
            // Gate log shipping (PostHog events + OTLP logs) on the same opt-out,
            // and stamp the OTLP resource. Local file logging stays on regardless.
            {
                let s = settings_store.get();
                obs::configure_shipping(
                    telemetry::is_enabled(s.telemetry_enabled),
                    s.device_id.clone(),
                    app.package_info().version.to_string(),
                );
            }
            // Log the running code-signing identity — an identity change vs the
            // TCC-granted one is THE root cause of the silent lost-access bug, so
            // make it visible in every log. Off-thread (shells codesign).
            #[cfg(target_os = "macos")]
            identity::log_signing_identity(
                app.package_info().version.to_string(),
                app.config().identifier.clone(),
            );

            app.manage(settings_store);
            #[cfg(target_os = "macos")]
            scheduler::spawn_loop(app.handle());
            // Independent 10s loop that keeps the menu-bar "next event" title and
            // countdown current between calendar polls (and clears a finished one).
            #[cfg(target_os = "macos")]
            scheduler::spawn_menu_bar_loop(app.handle());

            // Startup pre-flight: auto-prompt for calendar access if a returning
            // (already-onboarded) user's grant was lost — e.g. a rebuild re-signs
            // the app with a new ad-hoc identity and TCC resets it to
            // NotDetermined — so they never have to find the "Grant calendar
            // access" button. New users are driven by onboarding instead. Skipped
            // in test mode (the test harness manages its own TCC fixture).
            #[cfg(target_os = "macos")]
            if onboarded && !testmode::is_test_mode() {
                calendar::preflight_calendar_access(app.handle().clone());
            }
            #[cfg(not(target_os = "macos"))]
            let _ = onboarded;

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

#[cfg(test)]
mod tests {
    use super::{is_swallowable_eventkit_null, parse_log_level};
    use log::LevelFilter;

    #[test]
    fn only_the_benign_eventkit_null_panic_is_swallowed() {
        // The exact benign read panic → swallowed (no log spam on every poll).
        assert!(is_swallowable_eventkit_null(Some(
            "Retval should not be null: unexpected NULL returned from -[EKEventStore ...]"
        )));
        // Everything else goes through the default hook: a non-string payload,
        // an unrelated panic, and crucially OTHER objc2-event-kit panics (e.g.
        // the real-e2e write path) so genuine bugs are never hidden.
        assert!(!is_swallowable_eventkit_null(None));
        assert!(!is_swallowable_eventkit_null(Some("index out of bounds")));
        assert!(!is_swallowable_eventkit_null(Some(
            "called `Option::unwrap()` on a `None` value"
        )));
    }

    #[test]
    fn log_level_defaults_to_info_when_absent_or_unknown() {
        assert_eq!(parse_log_level(None), LevelFilter::Info);
        assert_eq!(parse_log_level(Some("")), LevelFilter::Info);
        assert_eq!(parse_log_level(Some("verbose")), LevelFilter::Info);
    }

    #[test]
    fn log_level_parses_known_levels_case_and_space_insensitively() {
        assert_eq!(parse_log_level(Some("debug")), LevelFilter::Debug);
        assert_eq!(parse_log_level(Some(" DEBUG ")), LevelFilter::Debug);
        assert_eq!(parse_log_level(Some("Trace")), LevelFilter::Trace);
        assert_eq!(parse_log_level(Some("warn")), LevelFilter::Warn);
        assert_eq!(parse_log_level(Some("error")), LevelFilter::Error);
        assert_eq!(parse_log_level(Some("off")), LevelFilter::Off);
    }
}
