//! Fullscreen takeover overlay via tauri-nspanel (Phase 1b spike → production overlay).
//!
//! The recipe (research report "hardest technical truth" + nspanel fullscreen example):
//!   borderless panel · nonactivating style · ScreenSaver level (1000) ·
//!   canJoinAllSpaces + fullScreenAuxiliary + stationary · one panel per NSScreen ·
//!   app already runs as Accessory (the load-bearing config for post-packaging behavior).
//!
//! CP1b spike: launch packaged app with ENTUCARA_SPIKE_OVERLAY=<secs>; you get <secs>
//! to focus another app fullscreen; panels then cover every display for 12 s and
//! self-dismiss. HUMAN gate: did they render above the fullscreen app, no interaction?

#![cfg(target_os = "macos")]

use crate::scheduler::lock_resilient;
use objc2_app_kit::NSScreen;
// NSRect + MainThreadMarker are already in scope from the `tauri_panel!` expansion
// below; importing them again is a duplicate-definition error.
use tauri::{AppHandle, Manager, WebviewUrl, WebviewWindowBuilder};
use tauri_nspanel::{tauri_panel, CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt};

tauri_panel! {
    panel!(OverlayPanel {
        config: {
            // The alert has buttons (Join/Dismiss/Snooze) — it must accept key status…
            can_become_key_window: true,
            is_floating_panel: true
        }
    })

    panel!(DimPanel {
        config: {
            // Companion frost panels: visible, click-swallowing, but can NEVER
            // take key status — clicks must not steal focus from the main alert.
            can_become_key_window: false,
            is_floating_panel: true
        }
    })
}

/// Label of the panel carrying the alert card (primary display) — re-keyed on
/// reuse so keyboard focus always lands there.
static MAIN_OVERLAY: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

const OVERLAY_LABEL_PREFIX: &str = "overlay-";

/// Test-mode ground truth for checkpoint scripts: CGWindowList stops listing
/// transparent panel windows after their content loads (verified 2026-06-05),
/// so cp1b/cp3 assert against this file instead.
fn write_overlay_state(labels: &[String]) {
    if !crate::testmode::is_test_mode() {
        return;
    }
    if let Some(home) = std::env::var_os("HOME") {
        let dir = std::path::Path::new(&home).join("Library/Application Support/dev.fforres.entucara");
        let _ = std::fs::create_dir_all(&dir);
        let _ = std::fs::write(
            dir.join("overlay-state.json"),
            serde_json::json!({ "overlays": labels }).to_string(),
        );
        // Append-only history: checkers assert "N panels WERE shown" tolerant of
        // the human at the keyboard dismissing test overlays early (observed
        // 2026-06-05: ~3 s reaction time, perfectly reasonable, broke 4 runs).
        if let Ok(mut f) = std::fs::OpenOptions::new()
            .create(true)
            .append(true)
            .open(dir.join("overlay-log.jsonl"))
        {
            use std::io::Write;
            let _ = writeln!(
                f,
                "{}",
                serde_json::json!({ "at": chrono::Utc::now().to_rfc3339(), "overlays": labels })
            );
        }
    }
}


/// The creation index of an overlay label (`overlay-<i>`), or None if it isn't one.
fn overlay_index(label: &str) -> Option<usize> {
    label.strip_prefix(OVERLAY_LABEL_PREFIX)?.parse().ok()
}

/// Overlay labels ordered by creation index. `webview_windows()` hands back an
/// unordered map, so raw key order must NOT be trusted to line up with the screen
/// list — and a lexical sort would put `overlay-10` before `overlay-2`, silently
/// pairing panels with the wrong display. Pure, for testing.
fn sorted_overlay_labels(labels: &[String]) -> Vec<&str> {
    let mut sorted: Vec<&str> = labels.iter().map(String::as_str).collect();
    sorted.sort_by_key(|l| overlay_index(l).unwrap_or(usize::MAX));
    sorted
}

/// Does a panel sitting at `current` need moving to cover `target`?
///
/// Both are `NSWindow`/`NSScreen` frames — AppKit points, bottom-left origin, ONE
/// coordinate space (see `resync_overlay_geometry` on why we never touch physical
/// pixels here). A sub-point tolerance is what makes this converge: after a
/// successful `setFrame:` the two agree, so a correctly-placed panel issues no
/// further frame changes and — load-bearing — no further log lines, on a path that
/// runs every scheduler tick while an alarm is up. Pure, for testing.
fn needs_replacing(current: NSRect, target: NSRect) -> bool {
    const TOLERANCE: f64 = 0.5; // sub-point: invisible, but absorbs f64 noise.
    let off = |a: f64, b: f64| (a - b).abs() > TOLERANCE;
    off(current.origin.x, target.origin.x)
        || off(current.origin.y, target.origin.y)
        || off(current.size.width, target.size.width)
        || off(current.size.height, target.size.height)
}

/// Snap live overlay panels back onto the CURRENT display layout, and return the
/// label now covering the primary screen (the one that should hold key focus).
///
/// Panels are built one-per-`NSScreen` with ABSOLUTE frames at fire time. Across
/// system sleep that layout can change under them — lid closed, external display
/// unplugged, resolution switched on wake — and a panel whose frame belongs to a
/// screen that no longer exists is invisible, so `order_front_regardless` alone
/// re-fronts nothing the user can see (the force-quit report). Re-frame first,
/// then re-front.
///
/// Driven through the native `NSWindow` frame rather than `window.set_position` /
/// `set_size`, for the reason `tray::position_under_tray` already documents: those
/// go through tao, which converts with the SOURCE window's scale factor
/// (`to_logical(self.scale_factor())`), so moving a panel between displays of
/// DIFFERENT scale — a retina built-in and a 1x external, i.e. the exact case this
/// function exists for — lands it at half/double coordinates and never converges,
/// re-issuing a futile frame change every tick. Working entirely in AppKit points
/// keeps one consistent space with no scale-factor mixing. (`tray.rs` has the same
/// `ns_window()` → `setFrame:display:` shape; worth extracting to a shared helper
/// if a third caller appears.)
///
/// Screens past the end of the panel list get no panel, and panels past the end of
/// the screen list (a display was DISCONNECTED mid-alarm) are deliberately left
/// where they are: they're on no screen, so invisible and harmless, whereas
/// stacking them onto a surviving screen would pile duplicate cards on the one
/// display the user can actually see.
fn resync_overlay_geometry(app: &AppHandle, live: &[String]) -> Option<String> {
    use objc2_app_kit::NSWindow;

    let mtm = MainThreadMarker::new()?;
    let screens = NSScreen::screens(mtm);
    let ordered = sorted_overlay_labels(live);
    // AppKit orders `screens` with the primary (menu-bar) display first, so the
    // panel paired with index 0 is the one that should take key status.
    let main_label = ordered.first().map(|l| (*l).to_string());

    for (idx, label) in ordered.iter().enumerate().take(screens.count()) {
        let target = screens.objectAtIndex(idx).frame();
        let Some(window) = app.get_webview_window(label) else {
            continue;
        };
        let Ok(ptr) = window.ns_window() else {
            continue;
        };
        if ptr.is_null() {
            continue;
        }
        // SAFETY: ns_window() hands back this window's live NSWindow; we hold a
        // MainThreadMarker (obtained above) and only read/set its frame.
        let ns_window: &NSWindow = unsafe { &*(ptr as *const NSWindow) };
        let current = ns_window.frame();
        if !needs_replacing(current, target) {
            continue;
        }
        log::info!(
            "overlay {label}: display layout changed under it — re-framing to \
             {},{} {}x{}",
            target.origin.x,
            target.origin.y,
            target.size.width,
            target.size.height
        );
        ns_window.setFrame_display(target, false);
    }
    main_label
}

/// Spawn one takeover panel per connected display. Returns the labels created.
///
/// If overlays are ALREADY live (T-0 firing while the T-5 alert is still up —
/// the normal back-to-back case, and every scheduler self-heal re-assert),
/// reuse them: re-place onto the current displays, re-front, return. Recreating
/// is both wasteful and a crash (close() is async, the label is still taken,
/// and the builder's failure path raised an ObjC exception that aborted Rust —
/// caught by CP3 e2e 2026-06-05).
pub fn show_overlays(app: &AppHandle) -> tauri::Result<Vec<String>> {
    let live: Vec<String> = app
        .webview_windows()
        .keys()
        .filter(|l| l.starts_with(OVERLAY_LABEL_PREFIX))
        .cloned()
        .collect();
    if !live.is_empty() {
        // Re-elect the key panel to whichever one now covers the primary screen.
        // MAIN_OVERLAY was previously written ONLY at creation, so after a display
        // change it could still name a panel left stranded on a vanished screen —
        // and `show_and_make_key` below would hand Esc/Enter to an invisible
        // window, killing keyboard dismissal while the sound kept looping.
        if let Some(main) = resync_overlay_geometry(app, &live) {
            *lock_resilient(&MAIN_OVERLAY) = Some(main);
        }
        for label in &live {
            if let Ok(panel) = app.get_webview_panel(label) {
                panel.order_front_regardless();
            }
        }
        if let Some(main_label) = lock_resilient(&MAIN_OVERLAY).as_ref() {
            if let Ok(panel) = app.get_webview_panel(main_label) {
                panel.show_and_make_key();
            }
        }
        crate::sound::start_alert_loop(app);
        write_overlay_state(&live);
        return Ok(live);
    }

    let monitors = app.available_monitors()?;
    let primary = app.primary_monitor()?.map(|m| m.position().to_owned());
    let mut labels = Vec::new();

    for (i, monitor) in monitors.iter().enumerate() {
        let label = format!("{OVERLAY_LABEL_PREFIX}{i}");

        let scale = monitor.scale_factor();
        let pos = monitor.position().to_logical::<f64>(scale);
        let size = monitor.size().to_logical::<f64>(scale);

        // CARD ON EVERY DISPLAY (user request): the alert renders AND is
        // actionable on all screens, not just the primary. Every panel is an
        // OverlayPanel (can take key) so Join/Dismiss/Snooze work wherever the
        // user is looking; the primary still takes INITIAL key status (below) so
        // Esc/Enter work immediately without a click.
        let is_primary = primary.as_ref().is_none_or(|p| p == monitor.position());

        let window = WebviewWindowBuilder::new(
            app,
            &label,
            WebviewUrl::App("index.html?window=overlay&role=main".into()),
        )
        .title("En Tu Cara Alert")
        .position(pos.x, pos.y)
        .inner_size(size.width, size.height)
        .decorations(false)
        .resizable(false)
        // Frosted glass: transparent webview over a native NSVisualEffectView.
        // (The earlier "effects destroys the panel" diagnosis was FALSE — it
        // trusted CGWindowList, which silently stops listing transparent windows
        // once content loads. Ground truth is overlay-state.json + human eyes.)
        // state: Active forces the SAME material on every display regardless of
        // which panel is key — fixes the active/inactive darkness mismatch.
        .transparent(true)
        .effects(tauri::utils::config::WindowEffectsConfig {
            effects: vec![tauri::utils::WindowEffect::HudWindow],
            state: Some(tauri::utils::WindowEffectState::Active),
            radius: None,
            color: None,
        })
        .visible(false) // shown via panel.order_front_regardless below
        .build()?;

        if is_primary {
            *lock_resilient(&MAIN_OVERLAY) = Some(label.clone());
        }
        let panel = window.to_panel::<OverlayPanel>()?;
        panel.set_level(PanelLevel::ScreenSaver.value());
        // Nonactivating: takeover draws above everything but never activates the app —
        // focus stays where the user was (they may be mid-keystroke in the meeting).
        panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
        // CP1b finding (2026-06-05): NSPanel defaults hidesOnDeactivate=YES, and an
        // Accessory app deactivates seconds after showing — panels silently vanished
        // ~1-2 s in. An alarm must outlive activation state; disable it.
        panel.set_hides_on_deactivate(false);
        panel.set_collection_behavior(
            CollectionBehavior::new()
                .can_join_all_spaces()
                .full_screen_auxiliary()
                .stationary()
                .ignores_cycle()
                .into(),
        );
        // window.show() records visible=true on the Tauri side (else Tauri re-asserts
        // the builder's visible:false after webview load); order_front_regardless
        // guarantees AppKit ordering without requiring app activation.
        window.show()?;
        panel.order_front_regardless();

        labels.push(label);
    }

    // Autofocus the principal display: the main panel takes key status so Esc /
    // Enter work immediately. Nonactivating style means the APP still doesn't
    // activate — focus-of-record stays with whatever the user was using.
    if let Some(main_label) = lock_resilient(&MAIN_OVERLAY).as_ref() {
        if let Ok(panel) = app.get_webview_panel(main_label) {
            panel.show_and_make_key();
        }
    }

    // Recurring alert sound: loops while panels are up, stops on close (user req).
    crate::sound::start_alert_loop(app);
    write_overlay_state(&labels);
    Ok(labels)
}

/// Silence the alert and close every overlay panel.
///
/// NOT an IPC command, and not to be called directly: `presentation` owns when the
/// takeover comes down, because panels have to stay a function of the card set.
/// Closing panels while cards remain is a SELF-RESURRECTING state — the scheduler's
/// next tick would rebuild them and restart the sound. This used to be exposed to
/// the webview, which made that state one `invoke` away.
pub(crate) fn close_overlays(app: &AppHandle) {
    crate::sound::stop_alert_loop();
    write_overlay_state(&[]);
    for (label, _) in app.webview_windows() {
        if label.starts_with(OVERLAY_LABEL_PREFIX) {
            if let Ok(panel) = app.get_webview_panel(&label) {
                if let Some(window) = panel.to_window() {
                    let _ = window.close();
                    continue;
                }
            }
            if let Some(window) = app.get_webview_window(&label) {
                let _ = window.close();
            }
        }
    }
}

/// Test/spike command — fire the overlay on demand.
#[tauri::command]
pub fn spike_show_overlays(app: AppHandle) -> Result<Vec<String>, String> {
    show_overlays(&app).map_err(|e| e.to_string())
}

/// CP1b automation: ENTUCARA_SPIKE_OVERLAY=<delay_secs> → wait (user makes another
/// app fullscreen), cover all displays, self-dismiss after 12 s.
pub fn maybe_run_spike(app: &AppHandle) {
    let Ok(delay) = std::env::var("ENTUCARA_SPIKE_OVERLAY") else {
        return;
    };
    let delay: u64 = delay.parse().unwrap_or(10);
    let handle = app.clone();
    std::thread::spawn(move || {
        std::thread::sleep(std::time::Duration::from_secs(delay));
        let shown = handle.run_on_main_thread({
            let h = handle.clone();
            move || {
                match show_overlays(&h) {
                    Ok(labels) => println!("SPIKE_OVERLAY shown: {labels:?}"),
                    Err(e) => eprintln!("SPIKE_OVERLAY failed: {e}"),
                }
            }
        });
        if shown.is_err() {
            eprintln!("SPIKE_OVERLAY: main-thread dispatch failed");
            return;
        }
        std::thread::sleep(std::time::Duration::from_secs(12));
        let _ = handle.run_on_main_thread({
            let h = handle.clone();
            move || {
                close_overlays(&h);
                println!("SPIKE_OVERLAY dismissed");
            }
        });
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    fn labels(of: &[&str]) -> Vec<String> {
        of.iter().map(|s| (*s).to_string()).collect()
    }

    #[test]
    fn overlay_labels_sort_by_creation_index_not_lexically() {
        // A 10+ display rig is exotic, but a lexical sort silently pairs
        // `overlay-10` with the FIRST monitor and re-places every panel onto the
        // wrong screen — a corruption that only shows up on the machines hardest
        // to test on. Pin the numeric order.
        let live = labels(&["overlay-10", "overlay-2", "overlay-0", "overlay-1"]);
        assert_eq!(
            sorted_overlay_labels(&live),
            ["overlay-0", "overlay-1", "overlay-2", "overlay-10"]
        );
    }

    #[test]
    fn unparseable_labels_sort_last_and_never_displace_a_real_panel() {
        // Defensive: an overlay-prefixed label without a numeric suffix must not
        // land at index 0 and steal the primary display's placement (which is also
        // the label elected to hold key focus).
        let live = labels(&["overlay-weird", "overlay-1", "overlay-0"]);
        assert_eq!(sorted_overlay_labels(&live), ["overlay-0", "overlay-1", "overlay-weird"]);
    }

    fn frame(x: f64, y: f64, w: f64, h: f64) -> NSRect {
        NSRect::new(
            objc2_foundation::NSPoint::new(x, y),
            objc2_foundation::NSSize::new(w, h),
        )
    }

    #[test]
    fn a_correctly_placed_panel_is_left_completely_alone() {
        // The overwhelmingly common case: a re-assert every tick while an alarm is
        // up, nothing changed. No setFrame (no flicker) and — critically — NO log
        // line, because the obs layer ships INFO+ to PostHog Logs and this path
        // runs on every tick. This is why the comparison has to CONVERGE.
        let f = frame(0.0, 0.0, 2560.0, 1440.0);
        assert!(!needs_replacing(f, f));
    }

    #[test]
    fn a_resolution_change_across_sleep_is_detected() {
        // Woke up on a different display mode: the panel's frame no longer covers
        // the screen, so the card is clipped or offscreen while the sound loops.
        let was = frame(0.0, 0.0, 3456.0, 2234.0);
        let now = frame(0.0, 0.0, 1920.0, 1080.0);
        assert!(needs_replacing(was, now));
    }

    #[test]
    fn a_panel_stranded_on_a_vanished_screen_is_detected() {
        // External display unplugged across sleep: the panel's origin still points
        // at coordinates no screen covers, so re-fronting it shows the user nothing.
        let stranded = frame(-2560.0, 0.0, 2560.0, 1440.0);
        let surviving = frame(0.0, 0.0, 1512.0, 982.0);
        assert!(needs_replacing(stranded, surviving));
    }

    #[test]
    fn sub_point_noise_is_not_a_layout_change() {
        // f64 frame arithmetic must not read as "the layout changed" — that would
        // re-issue a frame change AND an INFO line on every single tick, which is
        // the per-tick log spam src-tauri/CLAUDE.md forbids.
        let target = frame(0.0, 0.0, 2560.0, 1440.0);
        assert!(!needs_replacing(frame(0.2, -0.1, 2560.3, 1439.8), target));
        // …but a visible fraction of a point still counts.
        assert!(needs_replacing(frame(0.0, 0.0, 2559.0, 1440.0), target));
    }

    #[test]
    fn overlay_index_only_matches_real_overlay_labels() {
        assert_eq!(overlay_index("overlay-3"), Some(3));
        assert_eq!(overlay_index("overlay-"), None);
        // The popover/settings windows share the process; they must never be
        // mistaken for takeover panels and re-framed onto a monitor.
        assert_eq!(overlay_index("main"), None);
        assert_eq!(overlay_index("settings"), None);
    }
}
