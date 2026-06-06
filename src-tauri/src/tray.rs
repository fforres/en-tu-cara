//! Tray icon + menu. Left- or right-click opens the standard macOS menu
//! (Open Settings · Quit). "Open Settings" opens a normal decorated window
//! (open_settings_at) — no popover, no tray-relative positioning, so it works
//! identically on any display / multi-monitor setup.

use tauri::{
    image::Image,
    menu::{Menu, MenuItem},
    tray::TrayIconBuilder,
    webview::Color,
    AppHandle, Manager, Theme,
};

const SETTINGS_LABEL: &str = "settings";
const ONBOARDING_LABEL: &str = "onboarding";

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
/// background window (post-setup, so window creation is safe — gotcha #3).
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
    if let (Some(tray), Ok(img)) = (app.tray_by_id("main"), Image::from_bytes(bytes)) {
        let _ = tray.set_icon(Some(img));
        let _ = tray.set_icon_as_template(template);
    }
}

pub fn setup(app: &AppHandle) -> tauri::Result<()> {
    let open = MenuItem::with_id(app, "open_settings", "Open Settings", true, None::<&str>)?;
    let quit = MenuItem::with_id(app, "quit", "Quit En Tu Cara", true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;

    let tray_icon =
        Image::from_bytes(include_bytes!("../icons/tray-auto.png")).expect("valid tray icon PNG");

    // Left- AND right-click both open the same menu (standard macOS menu-bar app).
    TrayIconBuilder::with_id("main")
        .icon(tray_icon)
        .icon_as_template(true) // adapts to light/dark menu bar
        .menu(&menu)
        .show_menu_on_left_click(true)
        .on_menu_event(|app, event| match event.id.as_ref() {
            "open_settings" => {
                if let Err(e) = open_settings(app.clone()) {
                    log::error!("open_settings failed: {e}");
                }
            }
            "quit" => app.exit(0),
            _ => {}
        })
        .build(app)?;

    Ok(())
}
