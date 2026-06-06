//! Tray icon + popover (Phase 1c: positioned nonactivating nspanel).
//!
//! Tauri does NOT auto-position windows under the tray (PLAN §1) — we compute the
//! popover origin from the TrayIconEvent rect (physical px → logical) on every
//! click, which also handles multi-display + menu-bar-on-secondary setups.
//!
//! The popover is a nonactivating NSPanel so opening it never steals focus from
//! the app the user is working in; it hides when it resigns key (outside click).

// panel_event!'s grammar REQUIRES `-> ()` on handler signatures; clippy flags it
// as unused_unit and attributes on macro invocations are ignored, so file-level.
#![allow(clippy::unused_unit)]

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, PhysicalPosition, PhysicalSize,
};

#[cfg(target_os = "macos")]
use tauri_nspanel::{tauri_panel, CollectionBehavior, ManagerExt, PanelLevel, StyleMask, WebviewWindowExt};

#[cfg(target_os = "macos")]
tauri_panel! {
    panel!(PopoverPanel {
        config: {
            can_become_key_window: true,
            is_floating_panel: true
        }
    })

    panel_event!(PopoverEventHandler {
        window_did_resign_key(notification: &NSNotification) -> ()
    })
}

const POPOVER_LABEL: &str = "tray-popover";

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let quit = MenuItem::with_id(app, "quit", "Quit En Tu Cara", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&quit])?;

    #[cfg(target_os = "macos")]
    setup_popover_panel(app)?;

    TrayIconBuilder::with_id("main")
        .icon(app.default_window_icon().expect("bundled icon").clone())
        .icon_as_template(true) // adapts to light/dark menu bar
        .menu(&menu)
        .show_menu_on_left_click(false) // left = popover, right = menu
        .on_menu_event(|app, event| {
            if event.id.as_ref() == "quit" {
                app.exit(0);
            }
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                rect,
                ..
            } = event
            {
                toggle_popover(tray.app_handle(), rect);
            }
        })
        .build(app)?;

    Ok(())
}

/// Convert the config-declared popover window into a nonactivating panel once.
#[cfg(target_os = "macos")]
fn setup_popover_panel(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window(POPOVER_LABEL)
        .expect("tray-popover window declared in tauri.conf.json");
    let panel = window.to_panel::<PopoverPanel>()?;

    // PopUpMenu (101): above normal windows, below the alert overlay (1000).
    panel.set_level(PanelLevel::PopUpMenu.value());
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
    // Popovers SHOULD hide when the app deactivates — unlike the alarm overlay
    // (CP1b finding) this is the desired outside-click/⌘-tab dismissal behavior...
    panel.set_hides_on_deactivate(true);
    // ...but resign-key is the primary dismissal: click anywhere outside.
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .transient()
            .ignores_cycle()
            .into(),
    );

    let handler = PopoverEventHandler::new();
    let handle = app.clone();
    handler.window_did_resign_key(move |_notification| {
        if let Ok(panel) = handle.get_webview_panel(POPOVER_LABEL) {
            panel.hide();
        }
    });
    panel.set_event_handler(Some(handler.as_ref()));

    Ok(())
}

fn toggle_popover(app: &AppHandle, tray_rect: tauri::Rect) {
    #[cfg(target_os = "macos")]
    {
        let Ok(panel) = app.get_webview_panel(POPOVER_LABEL) else {
            return;
        };
        if panel.is_visible() {
            panel.hide();
            return;
        }
        if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
            position_under_tray(&window, &tray_rect);
            let _ = window.show(); // sync Tauri visible-state (CP1b lesson)
            panel.show_and_make_key(); // key so resign-key dismissal works
        }
    }
    #[cfg(not(target_os = "macos"))]
    {
        let _ = (app, tray_rect);
    }
}

/// Center the popover horizontally under the tray icon, top edge below the menu
/// bar. Tray rect arrives in PHYSICAL pixels of whichever display hosts the icon.
fn position_under_tray(window: &tauri::WebviewWindow, rect: &tauri::Rect) {
    let (icon_pos, icon_size) = match (rect.position, rect.size) {
        (tauri::Position::Physical(p), tauri::Size::Physical(s)) => (p, s),
        (tauri::Position::Logical(p), tauri::Size::Logical(s)) => {
            // Normalize logical → physical via the monitor under the icon.
            let scale = window
                .current_monitor()
                .ok()
                .flatten()
                .map(|m| m.scale_factor())
                .unwrap_or(2.0);
            (
                PhysicalPosition::new((p.x * scale) as i32, (p.y * scale) as i32),
                PhysicalSize::new((s.width * scale) as u32, (s.height * scale) as u32),
            )
        }
        _ => return,
    };

    let win_size = window.outer_size().unwrap_or(PhysicalSize::new(840, 1120));
    let icon_center_x = icon_pos.x + icon_size.width as i32 / 2;
    let x = icon_center_x - win_size.width as i32 / 2;
    let y = icon_pos.y + icon_size.height as i32; // just below the menu bar
    let _ = window.set_position(PhysicalPosition::new(x, y));
}
