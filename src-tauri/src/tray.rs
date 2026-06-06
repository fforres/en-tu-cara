//! Tray icon + popover toggle (Phase 0 stub).
//!
//! Phase 1c replaces the plain-window toggle with a nonactivating nspanel positioned
//! from the tray rect (PLAN §2 CP1c). For Phase 0 the only contract is: icon visible,
//! left-click toggles the popover window, no focus stolen from the active app.

use tauri::{
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    AppHandle, Manager, Runtime,
};

pub fn setup<R: Runtime>(app: &AppHandle<R>) -> tauri::Result<()> {
    let quit = MenuItem::with_id(app, "quit", "Quit En Tu Cara", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&quit])?;

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
                ..
            } = event
            {
                toggle_popover(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}

fn toggle_popover<R: Runtime>(app: &AppHandle<R>) {
    if let Some(win) = app.get_webview_window("tray-popover") {
        let visible = win.is_visible().unwrap_or(false);
        if visible {
            let _ = win.hide();
        } else {
            let _ = win.show();
            let _ = win.set_focus();
        }
    }
}
