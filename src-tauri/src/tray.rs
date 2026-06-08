//! Tray icon + popover. Left-click the tray icon → a nonactivating NSPanel
//! popover (the upcoming-events list) positioned under the icon; right-click →
//! the native menu (Open Settings · Quit). A cog inside the popover also opens
//! the menu actions.
//!
//! Visibility recipe (mirrors the working overlay, overlay.rs): nonactivating
//! style + **hides_on_deactivate(false)** (an Accessory app deactivates
//! instantly, so the default YES made the popover vanish the moment it showed) +
//! order_front_regardless + show_and_make_key (key status so resign-key dismisses
//! it on outside click). Positioned with set_position AFTER show (a no-op while
//! hidden).

// panel_event!'s grammar requires `-> ()` on handler signatures; clippy flags it
// as unused_unit and attributes on macro invocations are ignored → file-level.
#![allow(clippy::unused_unit)]

use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent},
    webview::Color,
    AppHandle, Manager, Theme,
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

const SETTINGS_LABEL: &str = "settings";
const ONBOARDING_LABEL: &str = "onboarding";
const POPOVER_LABEL: &str = "popover";

// When the popover is open and you click the tray to close it, the click first
// makes the panel resign key (→ hide). We record that moment so the click's
// toggle doesn't immediately re-open it (toggle-vs-resign race).
#[cfg(target_os = "macos")]
static LAST_POPOVER_HIDE: std::sync::Mutex<Option<std::time::Instant>> =
    std::sync::Mutex::new(None);

/// Paint a freshly-built (hidden) window's background to match the system
/// light/dark mode — so it doesn't flash white before the webview paints — then
/// show + focus it and bring the Accessory app forward (it won't activate on
/// window creation on its own; observed: onscreen=false without this).
fn present_window(window: &tauri::WebviewWindow) {
    let bg = match window.theme() {
        Ok(Theme::Dark) => Color(30, 30, 30, 255),
        _ => Color(246, 246, 246, 255),
    };
    let _ = window.set_background_color(Some(bg));
    let _ = window.show();
    let _ = window.set_focus();
    #[cfg(target_os = "macos")]
    {
        use objc2_app_kit::NSApplication;
        use objc2_foundation::MainThreadMarker;
        if let Some(mtm) = MainThreadMarker::new() {
            #[allow(deprecated)]
            NSApplication::sharedApplication(mtm).activateIgnoringOtherApps(true);
        }
    }
}

/// Open (or focus) the settings window — a NORMAL decorated window. The app is
/// an Accessory (no Dock icon), so it doesn't activate on its own; we activate +
/// focus so the window comes forward.
#[tauri::command]
pub fn open_settings(app: AppHandle) -> Result<(), String> {
    open_settings_at(app, None)
}

pub fn open_settings_at(app: AppHandle, section: Option<&str>) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(SETTINGS_LABEL) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }
    let url = match section {
        Some(s) => format!("index.html?window=settings&section={s}"),
        None => "index.html?window=settings".to_string(),
    };
    let window =
        tauri::WebviewWindowBuilder::new(&app, SETTINGS_LABEL, tauri::WebviewUrl::App(url.into()))
            .title("En Tu Cara Settings")
            .inner_size(900.0, 640.0)
            .min_inner_size(720.0, 480.0)
            // Build hidden so we can paint the right background BEFORE first show.
            .visible(false)
            .build()
            .map_err(|e| e.to_string())?;
    present_window(&window);
    Ok(())
}

/// Open (or focus) the first-run onboarding window — a small, fixed-size themed
/// window. Idempotent.
fn open_onboarding(app: &AppHandle) -> Result<(), String> {
    if let Some(existing) = app.get_webview_window(ONBOARDING_LABEL) {
        let _ = existing.show();
        let _ = existing.set_focus();
        return Ok(());
    }
    let window = tauri::WebviewWindowBuilder::new(
        app,
        ONBOARDING_LABEL,
        tauri::WebviewUrl::App("index.html?window=onboarding".into()),
    )
    .title("Welcome to En Tu Cara")
    .inner_size(520.0, 580.0)
    .resizable(false)
    .visible(false)
    .build()
    .map_err(|e| e.to_string())?;
    present_window(&window);
    Ok(())
}

/// Show onboarding on launch if the user hasn't completed it. Invoked from the
/// popover window (post-setup, so window creation is safe — gotcha #3).
#[tauri::command]
pub fn maybe_show_onboarding(app: AppHandle) -> Result<(), String> {
    if app.state::<crate::settings::SettingsStore>().get().onboarded {
        return Ok(());
    }
    open_onboarding(&app)
}

/// Mark onboarding complete and close the window.
#[tauri::command]
pub fn finish_onboarding(app: AppHandle) -> Result<(), String> {
    app.state::<crate::settings::SettingsStore>()
        .update(|s| s.onboarded = true);
    if let Some(win) = app.get_webview_window(ONBOARDING_LABEL) {
        let _ = win.close();
    }
    Ok(())
}

/// Open an external URL in the default browser (About / Help & Support tab).
#[tauri::command]
pub fn open_url(app: AppHandle, url: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    app.opener()
        .open_url(url, None::<&str>)
        .map_err(|e| e.to_string())
}

/// Hide the popover (called from the cog menu before opening settings, so the
/// popover doesn't linger behind the settings window).
#[tauri::command]
pub fn hide_popover(app: AppHandle) {
    #[cfg(target_os = "macos")]
    if let Ok(panel) = app.get_webview_panel(POPOVER_LABEL) {
        panel.hide();
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Menu-bar title beside the icon ("🔥 ENGINE… · 23m"). None clears it.
pub fn set_tray_title(app: &AppHandle, title: Option<String>) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_title(title);
    }
}

/// Swap the menu-bar icon style live (Settings → Menu Bar → Tray icon).
/// "light"/"dark" force a fixed glyph; anything else ("auto") uses the template
/// that adapts to the light/dark menu bar. Glyphs are embedded at compile time.
pub fn apply_tray_icon(app: &AppHandle, style: &str) {
    let (bytes, template): (&[u8], bool) = match style {
        "light" => (include_bytes!("../icons/tray-light.png"), false),
        "dark" => (include_bytes!("../icons/tray-dark.png"), false),
        _ => (include_bytes!("../icons/tray-auto.png"), true),
    };
    match (app.tray_by_id("main"), Image::from_bytes(bytes)) {
        (Some(tray), Ok(img)) => {
            let _ = tray.set_icon(Some(img));
            let _ = tray.set_icon_as_template(template);
        }
        // include_bytes! is compile-time, so a decode failure is a build bug, and
        // a missing tray means we were called before setup — surface both rather
        // than silently keeping the wrong (or no) glyph.
        (tray, img) => log::warn!(
            "apply_tray_icon('{style}') skipped: tray present={}, image ok={}",
            tray.is_some(),
            img.is_ok()
        ),
    }
}

/// Convert the config-declared popover window into a nonactivating panel once.
/// See the module-level visibility recipe.
#[cfg(target_os = "macos")]
fn setup_popover_panel(app: &AppHandle) -> tauri::Result<()> {
    let window = app
        .get_webview_window(POPOVER_LABEL)
        .expect("popover window declared in tauri.conf.json");
    let panel = window.to_panel::<PopoverPanel>()?;

    // PopUpMenu (101): above normal windows, below the alert overlay (1000).
    panel.set_level(PanelLevel::PopUpMenu.value());
    panel.set_style_mask(StyleMask::empty().nonactivating_panel().into());
    // THE fix (overlay.rs gotcha): default hidesOnDeactivate=YES + an Accessory
    // app that deactivates instantly = the popover vanished the moment it showed.
    panel.set_hides_on_deactivate(false);
    panel.set_collection_behavior(
        CollectionBehavior::new()
            .can_join_all_spaces()
            .stationary()
            .ignores_cycle()
            .into(),
    );

    // Dismiss on outside click: the panel resigns key → hide it.
    let handler = PopoverEventHandler::new();
    let handle = app.clone();
    handler.window_did_resign_key(move |_notification| {
        if let Ok(panel) = handle.get_webview_panel(POPOVER_LABEL) {
            *LAST_POPOVER_HIDE.lock().unwrap() = Some(std::time::Instant::now());
            panel.hide();
        }
    });
    panel.set_event_handler(Some(handler.as_ref()));
    Ok(())
}

fn toggle_popover(app: &AppHandle) {
    #[cfg(target_os = "macos")]
    {
        let Ok(panel) = app.get_webview_panel(POPOVER_LABEL) else {
            log::warn!("toggle_popover: no panel");
            return;
        };
        let visible = panel.is_visible();
        if visible {
            panel.hide();
            return;
        }
        // Clicking the tray while open made it resign key (→ hide) a moment ago;
        // don't re-open on the same click.
        if let Some(t) = *LAST_POPOVER_HIDE.lock().unwrap() {
            if t.elapsed() < std::time::Duration::from_millis(250) {
                return;
            }
        }
        if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
            // Position BEFORE showing: set the native frame while hidden so the
            // panel never paints at a stale location (no first-show flicker/jump).
            position_under_tray(&window);
            let _ = window.show();
            panel.show_and_make_key();
            panel.order_front_regardless();
        }
    }
    #[cfg(not(target_os = "macos"))]
    let _ = app;
}

/// Position the popover under the menu-bar icon, centered on the cursor, top edge
/// flush below the menu bar.
///
/// This bypasses Tauri's `window.set_position` entirely. That path goes through
/// tao, which on macOS mis-handles cross-monitor moves between displays of
/// different scale factors: the window's physical size is reported stale on the
/// first show on a new display and `set_position` can even reset the size
/// (tauri#5229, tauri#7139). That was the "first click on a display lands
/// separated, second click snaps into place" bug.
///
/// Instead we drive the native `NSWindow` frame directly, working entirely in
/// AppKit's logical-point coordinate space (one consistent space, no
/// physical/logical or scale-factor mixing):
///   - `NSEvent::mouseLocation()` — the cursor sits on the icon at click time, so
///     it gives the icon's horizontal center without touching the tray rect.
///   - the `NSScreen` under the cursor — its `visibleFrame` already excludes the
///     menu bar, so the window's top edge lands just below it.
///   - the window's *actual* current `frame()` — real size, never stale.
///
/// `setFrame:display:` then places it in one shot. This is the approach ahkohd's
/// canonical tauri menubar example uses.
#[cfg(target_os = "macos")]
fn position_under_tray(window: &tauri::WebviewWindow) {
    use objc2_app_kit::{NSEvent, NSScreen, NSWindow};
    use objc2_foundation::MainThreadMarker;

    let Some(mtm) = MainThreadMarker::new() else {
        return;
    };
    let Ok(ptr) = window.ns_window() else {
        return;
    };
    if ptr.is_null() {
        return;
    }
    // SAFETY: ns_window() returns this window's live NSWindow; we are on the main
    // thread (MainThreadMarker obtained above) and only read/set its frame.
    let ns_window: &NSWindow = unsafe { &*(ptr as *const NSWindow) };

    let mouse = NSEvent::mouseLocation();

    // The visible frame (menu bar excluded) of the screen the cursor is on, i.e.
    // the screen whose menu bar hosts the clicked icon. Fall back to main screen.
    let screens = NSScreen::screens(mtm);
    let visible = (0..screens.count())
        .map(|i| screens.objectAtIndex(i))
        .find(|s| {
            let f = s.frame();
            mouse.x >= f.origin.x
                && mouse.x <= f.origin.x + f.size.width
                && mouse.y >= f.origin.y
                && mouse.y <= f.origin.y + f.size.height
        })
        .or_else(|| NSScreen::mainScreen(mtm))
        .map(|s| s.visibleFrame());
    let Some(visible) = visible else {
        return;
    };

    let mut frame = ns_window.frame();
    // Top edge flush with the top of the visible area (just under the menu bar).
    frame.origin.y = visible.origin.y + visible.size.height - frame.size.height;
    // Center horizontally on the cursor, clamped to stay on this screen.
    let max_x = visible.origin.x + visible.size.width - frame.size.width;
    frame.origin.x = (mouse.x - frame.size.width / 2.0)
        .clamp(visible.origin.x, max_x.max(visible.origin.x));

    ns_window.setFrame_display(frame, false);
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open_settings", "Open Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit En Tu Cara", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    #[cfg(target_os = "macos")]
    setup_popover_panel(app)?;

    let tray_icon =
        Image::from_bytes(include_bytes!("../icons/tray-auto.png")).expect("valid tray icon PNG");

    // Left-click → popover; right-click → menu.
    TrayIconBuilder::with_id("main")
        .icon(tray_icon)
        .icon_as_template(true) // adapts to light/dark menu bar
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open_settings" => {
                if let Err(e) = open_settings(app.clone()) {
                    log::error!("open_settings failed: {e}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            // Toggle on Down (snappy + the Up event isn't reliably delivered for a
            // status item). Right-click → the native menu (show_menu_on_left_click
            // is false, so the menu only appears on right-click).
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Down,
                ..
            } = event
            {
                toggle_popover(tray.app_handle());
            }
        })
        .build(app)?;

    Ok(())
}
