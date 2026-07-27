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
/// unordered map, so raw key order must NOT be trusted to line up with the
/// monitor list — and a lexical sort would put `overlay-10` before `overlay-2`,
/// silently pairing panels with the wrong display. Pure, for testing.
fn sorted_overlay_labels(labels: &[String]) -> Vec<String> {
    let mut sorted: Vec<String> = labels.to_vec();
    sorted.sort_by_key(|l| overlay_index(l).unwrap_or(usize::MAX));
    sorted
}

/// Pair each live overlay label with the index of the monitor it should occupy
/// now. Labels past the end of the monitor list (a display was DISCONNECTED
/// mid-alarm) deliberately get no target: leaving them where they are costs
/// nothing (they're on no screen, so invisible), whereas stacking them onto an
/// occupied screen would pile duplicate cards on one display. Pure, for testing.
fn reposition_targets(sorted_labels: &[String], monitor_count: usize) -> Vec<(String, usize)> {
    sorted_labels.iter().take(monitor_count).cloned().zip(0..monitor_count).collect()
}

/// A panel's frame: where it should sit and how big, in physical pixels.
type Frame = (tauri::PhysicalPosition<i32>, tauri::PhysicalSize<u32>);

/// What to do with one overlay panel on a re-assert pass.
#[derive(Debug, PartialEq, Eq)]
enum Placement {
    /// Correctly placed and unchanged — issue no setFrame at all.
    LeaveAlone,
    /// Re-place it. `announce` is true only on a real display-layout EDGE, so the
    /// log line stays once-per-change instead of once-per-tick.
    Replace { announce: bool },
}

/// Decide a single panel's placement from the target frame, what the window
/// reports now, and the frame we last applied to it.
///
/// Two independent reasons to act: the LAYOUT changed under us (target ≠ what we
/// last applied — the sleep/wake case), or the panel drifted off its target for
/// any other reason. Only the first is worth a log line: `resync_overlay_geometry`
/// runs on EVERY scheduler tick while an alarm is up, and the obs layer ships
/// INFO+ to PostHog Logs, so a per-tick line would be event spam (see
/// src-tauri/CLAUDE.md "Do NOT"). `observed: None` (the window wouldn't report its
/// frame) counts as drifted — better one redundant setFrame than a panel stranded
/// offscreen. Pure, for testing.
fn placement_for(target: Frame, observed: Option<Frame>, last_applied: Option<Frame>) -> Placement {
    let layout_changed = last_applied != Some(target);
    let drifted = observed != Some(target);
    if !layout_changed && !drifted {
        return Placement::LeaveAlone;
    }
    Placement::Replace { announce: layout_changed }
}

/// The frame we last applied to each overlay label, so `placement_for` can tell a
/// real layout change from steady state. Keyed by label; entries outlive a panel
/// harmlessly (a rebuilt panel is created at the right frame, so it matches).
static APPLIED_FRAMES: std::sync::Mutex<Option<std::collections::HashMap<String, Frame>>> =
    std::sync::Mutex::new(None);

/// Snap live overlay panels back onto the CURRENT display layout.
///
/// Panels are built one-per-NSScreen with ABSOLUTE frames at fire time. Across
/// system sleep that layout can change under them — lid closed, external display
/// unplugged, resolution switched on wake — and a panel whose frame belongs to a
/// screen that no longer exists is invisible, so `order_front_regardless` alone
/// re-fronts nothing the user can see (the force-quit report). Re-set the frame
/// first, then re-front.
fn resync_overlay_geometry(app: &AppHandle, live: &[String]) {
    let Ok(monitors) = app.available_monitors() else {
        return; // can't read the layout — leave the panels alone, still re-front.
    };
    let mut applied = APPLIED_FRAMES.lock().unwrap_or_else(|e| e.into_inner());
    let applied = applied.get_or_insert_with(std::collections::HashMap::new);

    for (label, idx) in reposition_targets(&sorted_overlay_labels(live), monitors.len()) {
        let (Some(window), Some(monitor)) = (app.get_webview_window(&label), monitors.get(idx))
        else {
            continue;
        };
        let target: Frame = (*monitor.position(), *monitor.size());
        let observed = window.outer_position().ok().zip(window.outer_size().ok());

        let Placement::Replace { announce } =
            placement_for(target, observed, applied.get(&label).copied())
        else {
            continue;
        };
        if announce {
            log::info!(
                "overlay {label}: display layout changed under it — re-placing at {},{} {}x{}",
                target.0.x,
                target.0.y,
                target.1.width,
                target.1.height
            );
        }
        let _ = window.set_position(target.0);
        let _ = window.set_size(target.1);
        applied.insert(label, target);
    }
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
        resync_overlay_geometry(app, &live);
        for label in &live {
            if let Ok(panel) = app.get_webview_panel(label) {
                panel.order_front_regardless();
            }
        }
        if let Some(main_label) = MAIN_OVERLAY.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
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
            *MAIN_OVERLAY.lock().unwrap_or_else(|e| e.into_inner()) = Some(label.clone());
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
    if let Some(main_label) = MAIN_OVERLAY.lock().unwrap_or_else(|e| e.into_inner()).as_ref() {
        if let Ok(panel) = app.get_webview_panel(main_label) {
            panel.show_and_make_key();
        }
    }

    // Recurring alert sound: loops while panels are up, stops on close (user req).
    crate::sound::start_alert_loop(app);
    write_overlay_state(&labels);
    Ok(labels)
}

/// Close every overlay panel.
#[tauri::command]
pub fn close_overlays(app: AppHandle) {
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
                close_overlays(h.clone());
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
        let sorted = sorted_overlay_labels(&labels(&[
            "overlay-10",
            "overlay-2",
            "overlay-0",
            "overlay-1",
        ]));
        assert_eq!(sorted, labels(&["overlay-0", "overlay-1", "overlay-2", "overlay-10"]));
    }

    #[test]
    fn unparseable_labels_sort_last_and_never_displace_a_real_panel() {
        // Defensive: an overlay-prefixed label without a numeric suffix must not
        // land at index 0 and steal the primary display's placement.
        let sorted = sorted_overlay_labels(&labels(&["overlay-weird", "overlay-1", "overlay-0"]));
        assert_eq!(sorted, labels(&["overlay-0", "overlay-1", "overlay-weird"]));
    }

    #[test]
    fn steady_state_pairs_each_panel_with_its_own_display() {
        // The overwhelmingly common re-assert: nothing changed since fire time.
        let targets = reposition_targets(&labels(&["overlay-0", "overlay-1"]), 2);
        assert_eq!(targets, vec![("overlay-0".to_string(), 0), ("overlay-1".to_string(), 1)]);
    }

    #[test]
    fn a_display_disconnected_mid_alarm_leaves_the_orphan_panel_alone() {
        // Two panels, one screen left (lid closed / dock unplugged across sleep).
        // overlay-0 is re-placed onto the surviving screen; overlay-1 gets NO
        // target on purpose — it's on no screen, so it's invisible and harmless,
        // whereas re-placing it too would stack a duplicate card on the one
        // display the user can actually see.
        let targets = reposition_targets(&labels(&["overlay-0", "overlay-1"]), 1);
        assert_eq!(targets, vec![("overlay-0".to_string(), 0)]);
    }

    #[test]
    fn a_display_attached_mid_alarm_does_not_invent_a_panel_for_it() {
        // One panel, two screens now. We re-place the panel we have and stop:
        // building a window in the REUSE path is the documented ObjC-abort
        // hazard (close() is async, labels stay taken), so the new display simply
        // goes uncovered until the next fire rebuilds the set.
        let targets = reposition_targets(&labels(&["overlay-0"]), 2);
        assert_eq!(targets, vec![("overlay-0".to_string(), 0)]);
    }

    #[test]
    fn no_displays_readable_means_no_repositioning_at_all() {
        // available_monitors() coming back empty must never be read as "move every
        // panel to index 0" — the re-assert still re-fronts, it just doesn't move
        // anything.
        assert!(reposition_targets(&labels(&["overlay-0", "overlay-1"]), 0).is_empty());
    }

    fn frame(x: i32, y: i32, w: u32, h: u32) -> Frame {
        (tauri::PhysicalPosition::new(x, y), tauri::PhysicalSize::new(w, h))
    }

    #[test]
    fn a_correctly_placed_panel_is_left_completely_alone() {
        // The overwhelmingly common case: a re-assert every tick while an alarm is
        // up, nothing changed. No setFrame (no flicker) and — critically — NO log
        // line, because the obs layer ships INFO+ to PostHog Logs and this path
        // runs on every tick.
        let f = frame(0, 0, 2560, 1440);
        assert_eq!(placement_for(f, Some(f), Some(f)), Placement::LeaveAlone);
    }

    #[test]
    fn a_resolution_change_across_sleep_re_places_and_announces_once() {
        // Woke up on a different mode: the panel's old frame no longer covers the
        // screen. Re-place AND log — this is a real edge worth having in the file
        // when triaging "the alarm was audible but invisible".
        let (was, now) = (frame(0, 0, 3456, 2234), frame(0, 0, 1920, 1080));
        assert_eq!(
            placement_for(now, Some(was), Some(was)),
            Placement::Replace { announce: true }
        );
        // …and the tick right after, having recorded it, goes quiet again.
        assert_eq!(placement_for(now, Some(now), Some(now)), Placement::LeaveAlone);
    }

    #[test]
    fn a_panel_that_drifted_off_target_is_fixed_without_a_log_line() {
        // Same layout as we last applied, but the window is no longer there. Still
        // self-heal it — silently, since this can recur per-tick and must not
        // become PostHog event spam.
        let target = frame(0, 0, 2560, 1440);
        assert_eq!(
            placement_for(target, Some(frame(-2560, 0, 2560, 1440)), Some(target)),
            Placement::Replace { announce: false }
        );
    }

    #[test]
    fn a_window_that_wont_report_its_frame_is_re_placed_defensively() {
        // outer_position/outer_size failing must never be read as "it's fine" — a
        // redundant setFrame is far cheaper than an alarm stranded offscreen.
        let target = frame(0, 0, 2560, 1440);
        assert_eq!(
            placement_for(target, None, Some(target)),
            Placement::Replace { announce: false }
        );
    }

    #[test]
    fn a_panel_we_have_never_placed_announces_its_first_placement() {
        // No remembered frame (fresh process, or a panel rebuilt after being lost).
        let target = frame(0, 0, 2560, 1440);
        assert_eq!(
            placement_for(target, Some(target), None),
            Placement::Replace { announce: true }
        );
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
