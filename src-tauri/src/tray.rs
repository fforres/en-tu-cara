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

/// Open an event in macOS Calendar.app.
///
/// macOS supports a deep-link scheme `ical://ekevent/<eventIdentifier>?method=
/// show&options=more` that selects the specific event in Calendar.app. Our
/// `EventDto.id` IS that `eventIdentifier` (calendar.rs maps it from
/// `event.eventIdentifier()`), so a PRECISE deep-link is feasible — no fallback
/// to merely opening Calendar.app is needed. We open it through the opener
/// plugin (same sink as `open_url`); the OS routes the `ical://` scheme to
/// Calendar.app.
#[tauri::command]
pub fn open_in_calendar(app: AppHandle, event_id: String) -> Result<(), String> {
    use tauri_plugin_opener::OpenerExt;
    let url = format!("ical://ekevent/{event_id}?method=show&options=more");
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

/// Menu-bar title beside the icon ("🔥 ENGINE… · 23m"). `None` clears it.
///
/// We clear by setting an EMPTY STRING, not `None`: passing `None` was observed
/// to leave the previous title in place on macOS, which froze a finished event
/// in the menu bar for hours (it never updated once `next_event_title` started
/// returning `None`). An empty title reliably blanks the text next to the icon.
pub fn set_tray_title(app: &AppHandle, title: Option<String>) {
    if let Some(tray) = app.tray_by_id("main") {
        let _ = tray.set_title(Some(title.clone().unwrap_or_default()));
    }
    // Edge-triggered (logs only on an actual change, like the event-presence log)
    // so the log shows a clean timeline of the next-event countdown ticking down
    // and clearing — the ground truth for verifying the title isn't frozen
    // (the menu-bar title can't be read back via screencapture/CGWindowList).
    if let Ok(mut last) = LAST_TRAY_TITLE.lock() {
        if *last != title {
            log::debug!("menu-bar title → {}", title.as_deref().unwrap_or("(cleared)"));
            *last = title;
        }
    }
}

/// Last applied menu-bar title, for the edge-triggered change log in `set_tray_title`.
static LAST_TRAY_TITLE: std::sync::Mutex<Option<String>> = std::sync::Mutex::new(None);

/// Owned snapshot of the menu-bar candidates from the latest calendar poll. The
/// title is re-derived from this against the LIVE clock by a short-interval loop
/// (see `scheduler::spawn_menu_bar_loop`), so the countdown stays current and a
/// just-finished event drops off within ~10s WITHOUT re-hitting EventKit — only
/// `now` changes between calendar polls, not the event set.
#[derive(Clone)]
pub struct OwnedCandidate {
    pub title: String,
    pub start: chrono::DateTime<chrono::Utc>,
    pub all_day: bool,
    pub status: String,
}

static MENU_BAR_SNAPSHOT: std::sync::Mutex<Vec<OwnedCandidate>> = std::sync::Mutex::new(Vec::new());

/// Replace the menu-bar candidate snapshot (called after each calendar read — the
/// scheduler poll and the popover refresh).
pub fn set_menu_bar_snapshot(snapshot: Vec<OwnedCandidate>) {
    if let Ok(mut g) = MENU_BAR_SNAPSHOT.lock() {
        *g = snapshot;
    }
}

/// Re-derive and apply the menu-bar "next event" title from the current snapshot
/// against the live clock. THE one place the title is applied — the scheduler
/// poll, the popover refresh, and the short-interval loop all funnel through here
/// so they can never compute it differently. Must run on the main thread.
pub fn refresh_menu_bar_title(app: &AppHandle) {
    let settings = app.state::<crate::settings::SettingsStore>().get();
    if !settings.show_next_event_in_menu_bar {
        set_tray_title(app, None);
        return;
    }
    let now = crate::testmode::clock::now();
    let snapshot = MENU_BAR_SNAPSHOT
        .lock()
        .map(|g| g.clone())
        .unwrap_or_default();
    let candidates: Vec<MenuBarCandidate> = snapshot
        .iter()
        .map(|c| MenuBarCandidate {
            title: &c.title,
            start: c.start,
            all_day: c.all_day,
            status: &c.status,
        })
        .collect();
    let title = next_event_title(&candidates, now, settings.menu_bar_title_chars as usize);
    set_tray_title(app, title);
}

/// Minimal projection of an event for the menu-bar title. Both `AlarmEvent`
/// (the scheduler heartbeat) and `EventDto` (the popover refresh) map to this,
/// so the title is ONE derivation over whichever path refreshed last — it can
/// never drift from the list the popover shows.
pub struct MenuBarCandidate<'a> {
    pub title: &'a str,
    pub start: chrono::DateTime<chrono::Utc>,
    pub all_day: bool,
    pub status: &'a str,
}

/// THE single source of the menu-bar "next event" title: pick the soonest
/// upcoming, non-all-day, non-canceled event and format it as "Title… · 12m"
/// (or "1h05m"). `None` when nothing qualifies. Pure + testable; called from the
/// scheduler tick AND `refresh_popover` so the two can't compute it differently.
pub fn next_event_title(
    candidates: &[MenuBarCandidate],
    now: chrono::DateTime<chrono::Utc>,
    max_chars: usize,
) -> Option<String> {
    candidates
        .iter()
        .filter(|c| !c.all_day && c.status != "canceled" && c.start > now)
        .min_by_key(|c| c.start)
        .map(|c| {
            let mins = (c.start - now).num_minutes();
            let when = if mins >= 60 {
                format!("{}h{:02}m", mins / 60, mins % 60)
            } else {
                format!("{}m", mins.max(1))
            };
            let max = max_chars.max(4);
            let label = if c.title.chars().count() > max {
                let mut s: String = c.title.chars().take(max - 1).collect();
                s.push('…');
                s
            } else {
                c.title.to_string()
            };
            format!("{label} · {when}")
        })
}

/// The popover's single on-open/refresh read: fetch upcoming events AND refresh
/// the menu-bar title from the SAME list, in one round-trip. This is what kills
/// the title-lags-behind-the-list drift — the title now lands at the same instant
/// as the list, derived by the same `next_event_title` the background heartbeat
/// uses. The scheduler heartbeat still updates the title while the popover is
/// closed; both are the same computation, just triggered at different times.
#[tauri::command]
pub fn refresh_popover(
    app: AppHandle,
    days_back: i64,
    days_forward: i64,
) -> Result<Vec<crate::calendar::EventDto>, String> {
    let events = crate::calendar::fetch_events(days_back, days_forward)?;
    // Refresh the menu-bar snapshot from the SAME events the popover shows, then
    // re-derive the title — one derivation, so the title can't drift from the
    // list, and the short-interval loop keeps it fresh from this same snapshot.
    let snapshot: Vec<OwnedCandidate> = events
        .iter()
        .filter_map(|e| {
            Some(OwnedCandidate {
                title: e.title.clone(),
                start: chrono::DateTime::parse_from_rfc3339(&e.start)
                    .ok()?
                    .with_timezone(&chrono::Utc),
                all_day: e.all_day,
                status: e.status.clone(),
            })
        })
        .collect();
    set_menu_bar_snapshot(snapshot);
    // Runs on the main thread (Tauri IPC) — apply directly.
    refresh_menu_bar_title(&app);
    Ok(events)
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
        log::info!("popover: resign-key → hide");
        if let Ok(panel) = handle.get_webview_panel(POPOVER_LABEL) {
            // Poison-tolerant: this runs on the AppKit MAIN thread inside an ObjC
            // callback — a panic here would unwind across the FFI boundary (UB /
            // abort), so never `.unwrap()` a possibly-poisoned lock here.
            *LAST_POPOVER_HIDE.lock().unwrap_or_else(|e| e.into_inner()) =
                Some(std::time::Instant::now());
            panel.hide();
        }
        log::info!("popover: resign-key handler done");
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
        log::info!("toggle_popover: enter visible={visible}");
        if visible {
            panel.hide();
            log::info!("toggle_popover: hidden");
            return;
        }
        // Clicking the tray while open made it resign key (→ hide) a moment ago;
        // don't re-open on the same click.
        if let Some(t) = *LAST_POPOVER_HIDE.lock().unwrap_or_else(|e| e.into_inner()) {
            if t.elapsed() < std::time::Duration::from_millis(250) {
                log::info!("toggle_popover: suppressed re-open (hide {}ms ago)", t.elapsed().as_millis());
                return;
            }
        }
        if let Some(window) = app.get_webview_window(POPOVER_LABEL) {
            // Position BEFORE showing: set the native frame while hidden so the
            // panel never paints at a stale location (no first-show flicker/jump).
            log::info!("toggle_popover: positioning");
            position_under_tray(&window);
            log::info!("toggle_popover: window.show()");
            let _ = window.show();
            log::info!("toggle_popover: show_and_make_key()");
            panel.show_and_make_key();
            panel.order_front_regardless();
            log::info!("toggle_popover: shown");
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

#[cfg(test)]
mod tests {
    use super::{next_event_title, MenuBarCandidate};
    use chrono::{DateTime, Utc};

    fn at(rfc: &str) -> DateTime<Utc> {
        DateTime::parse_from_rfc3339(rfc).unwrap().with_timezone(&Utc)
    }

    #[test]
    fn picks_soonest_future_skipping_all_day_canceled_and_past() {
        let now = at("2026-06-08T09:00:00Z");
        let cands = vec![
            MenuBarCandidate { title: "Past", start: at("2026-06-08T08:30:00Z"), all_day: false, status: "confirmed" },
            MenuBarCandidate { title: "AllDay", start: at("2026-06-08T09:05:00Z"), all_day: true, status: "confirmed" },
            MenuBarCandidate { title: "Canceled", start: at("2026-06-08T09:03:00Z"), all_day: false, status: "canceled" },
            MenuBarCandidate { title: "Standup", start: at("2026-06-08T09:05:00Z"), all_day: false, status: "confirmed" },
            MenuBarCandidate { title: "Later", start: at("2026-06-08T10:00:00Z"), all_day: false, status: "confirmed" },
        ];
        assert_eq!(next_event_title(&cands, now, 20).as_deref(), Some("Standup · 5m"));
    }

    #[test]
    fn countdown_decrements_as_now_advances_then_clears_once_started() {
        // The regression fence for the "frozen dead event in the menu bar" bug:
        // against a FIXED event start, as `now` advances the minutes count DOWN,
        // and the instant the event has started it drops out (None → the title
        // clears). The live 10s loop calls this with the real clock, so the
        // menu-bar countdown moves and a finished event can't linger.
        let c = vec![MenuBarCandidate {
            title: "Standup",
            start: at("2026-06-08T09:10:00Z"),
            all_day: false,
            status: "confirmed",
        }];
        assert_eq!(next_event_title(&c, at("2026-06-08T09:00:00Z"), 20).as_deref(), Some("Standup · 10m"));
        assert_eq!(next_event_title(&c, at("2026-06-08T09:01:00Z"), 20).as_deref(), Some("Standup · 9m"));
        assert_eq!(next_event_title(&c, at("2026-06-08T09:05:00Z"), 20).as_deref(), Some("Standup · 5m"));
        // 30s out floors to a visible "1m", never "0m".
        assert_eq!(next_event_title(&c, at("2026-06-08T09:09:30Z"), 20).as_deref(), Some("Standup · 1m"));
        // At start and well past it → no longer upcoming → cleared (the bug).
        assert!(next_event_title(&c, at("2026-06-08T09:10:00Z"), 20).is_none());
        assert!(
            next_event_title(&c, at("2026-06-08T11:10:00Z"), 20).is_none(),
            "an event 2h dead must not linger in the menu bar"
        );
    }

    #[test]
    fn formats_hours_and_truncates_long_titles() {
        let now = at("2026-06-08T09:00:00Z");
        let cands = vec![MenuBarCandidate {
            title: "Quarterly planning sync",
            start: at("2026-06-08T10:05:00Z"), // 65 min out
            all_day: false,
            status: "confirmed",
        }];
        // 65 min → "1h05m"; 10-char cap → 9 chars + ellipsis.
        assert_eq!(next_event_title(&cands, now, 10).unwrap(), "Quarterly… · 1h05m");
    }

    #[test]
    fn none_when_nothing_upcoming() {
        let now = at("2026-06-08T09:00:00Z");
        let past = vec![MenuBarCandidate {
            title: "Past", start: at("2026-06-08T08:00:00Z"), all_day: false, status: "confirmed",
        }];
        assert!(next_event_title(&past, now, 20).is_none());
        assert!(next_event_title(&[], now, 20).is_none());
    }
}
