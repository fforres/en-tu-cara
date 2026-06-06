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
}

const OVERLAY_LABEL_PREFIX: &str = "overlay-";

/// Spawn one takeover panel per connected display. Returns the labels created.
///
/// If overlays are ALREADY live (T-0 firing while the T-5 alert is still up —
/// the normal back-to-back case), reuse them: re-front and return. Recreating
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
        for label in &live {
            if let Ok(panel) = app.get_webview_panel(label) {
                panel.order_front_regardless();
            }
        }
        return Ok(live);
    }

    let monitors = app.available_monitors()?;
    let mut labels = Vec::new();

    for (i, monitor) in monitors.iter().enumerate() {
        let label = format!("{OVERLAY_LABEL_PREFIX}{i}");

        let scale = monitor.scale_factor();
        let pos = monitor.position().to_logical::<f64>(scale);
        let size = monitor.size().to_logical::<f64>(scale);

        let window = WebviewWindowBuilder::new(
            app,
            &label,
            WebviewUrl::App("index.html?window=overlay".into()),
        )
        .title("En Tu Cara Alert")
        .position(pos.x, pos.y)
        .inner_size(size.width, size.height)
        .decorations(false)
        .resizable(false)
        .visible(false) // shown via panel.order_front_regardless below
        .build()?;

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

    Ok(labels)
}

/// Close every overlay panel.
#[tauri::command]
pub fn close_overlays(app: AppHandle) {
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
