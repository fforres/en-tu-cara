# En Tu Cara — Plan

> macOS menu-bar meeting-alert app. Tauri v2 + React/TS webviews + Rust core.
> Fully local: reads calendars via EventKit (the accounts in macOS System
> Settings → Internet Accounts) — no OAuth, no servers.
>
> **The one unforgivable failure: a meeting started and no alert fired.** The
> second failure that kills adoption is alert fatigue (alerting for declined
> meetings, takeovers while presenting, un-dismissable pile-ups). Everything
> below serves those two constraints.

## What ships today

The core is built and guarded by `scripts/checkpoints/*-auto.sh`:

- **Calendar read** — EventKit via `eventkit-rs`; multi-calendar dedup;
  occurrence expansion; RSVP/status fields; `EKEventStoreChanged` observer +
  ≤60 s poll backstop + refetch-on-wake.
- **Alarm engine** — pure `compute_actions(events, now, state)` in
  `alarm_core.rs`: 0–3 configurable pre-event reminders + a MANDATORY T-0 start
  alarm (cannot be disabled); declined/canceled never alert; tentative alerts;
  snooze; pause; fire-on-wake-if-still-ongoing; dedup. Wall-clock arming with a
  windowed `latencyCritical` assertion; sleep/wake hooks.
- **Takeover overlay** — one `tauri-nspanel` panel per display, above fullscreen
  apps and on all Spaces; event info, Join (only when a link exists), Dismiss,
  "Remind me again in N min" (configurable default snooze), native NSSound;
  multiple events stack as cards.
- **Tray popover** — ongoing ("Xm remaining" + pie) and upcoming (day-grouped,
  today/all), calendar color/account, meeting links. Right-click → native menu.
- **Settings** — registry-driven (sidebar + fuzzy search), live-applied/persisted.
- **Packaging & ops** — `.app` + `.dmg`, self-update, logging to
  `~/.config/skyward/en-tu-cara`, first-run onboarding, GitHub Actions CI.
  Release flow: `docs/RELEASING.md`.

## Architecture

```
src/                       React + TS (Vite, HMR); one bundle, routed by ?window=
  windows/tray/            popover UI
  windows/overlay/         fullscreen alert + themes (own guide)
  windows/settings/        registry-driven settings (own guide)
  windows/onboarding/      first-run permissions
  lib/                     pure TS: link extraction, classification (own guide)
src-tauri/src/             Rust core (own guide)
  calendar.rs              EventKit commands + dedup
  scheduler.rs             arming, latency assertion, sleep/wake, test injection
  alarm_core.rs            PURE decision engine — clock-free, side-effect-free
  overlay.rs / tray.rs     nspanel windows (takeover / popover / settings opener)
  settings.rs / state.rs   persisted settings / fired-set + snoozes
  sound.rs / testmode.rs   native sound / mock clock + fire log
scripts/checkpoints/       regression gates: *-auto.sh + human runbooks
```

## Remaining work

### Tray popover & menu

- **Cog menu**: a cog in the popover opens a native macOS menu —
  Settings · About · Feedback · Quit.
- **Popover positioning**: native `NSWindow` `setFrame:` (bypasses Tauri's
  `set_position`, which mis-handles cross-monitor/scale-factor moves). Commit and
  verify the first click lands centered under the icon on every display.

### Appearance

- Full dock-icon picker and full tray-glyph picker. Curated sets live in
  `assets/icon-options/` — never delete them; ids are stable.

### Distribution hardening

- Apple Developer ID cert + notarization. Today the build is ad-hoc signed:
  Gatekeeper needs right-click→Open on first launch, and the TCC calendar grant
  is keyed to the ad-hoc code identity (can re-prompt across rebuilds).

### Reliability hardening

- Permission revoked mid-run → tray banner + System Settings deep-link.
- Chaos: 0 calendars; ~300 events; week-long sleep → no alert storm on wake;
  monitor hot-plug between arming and fire re-targets overlay panels.
- Resource re-check: idle < 120 MB; overlays on 2 displays < 250 MB.

### EventKit freshness

- Confirm propagation when Calendar.app is closed (event made on iPhone/web).
  Fallback ladder if stale: trigger EventKit refresh on poll/wake → prompt to
  open Calendar.app → escalate (the local-only premise needs a rethink).

## Human-only verification (cannot be automated)

`screencapture`/CGWindowList cannot prove z-order above another app's fullscreen,
and nothing can prove audible sound. These need eyes/ears — protocol in
`scripts/checkpoints/cp1b-human.md`:

- Overlay renders ABOVE another app's fullscreen, on built-in + external displays.
- Popover lands under the tray icon on the first click, on each display.
- Alert sound is audible; Join opens the real browser exactly once.
- 3-day dogfood: every accepted/tentative non-all-day meeting fired T-5 and T-0;
  nothing fired for declined/canceled; tray data correct within ~75 s of edits;
  survived sleep/wake/restart with no replay or storm.

## Load-bearing constraints (don't break)

- Bundle id `dev.fforres.entucara` — the calendar TCC grant is keyed to it.
- `tauri-nspanel` pinned to an exact git rev (its v2.1 branch); `eventkit-rs`
  pinned `=0.5.6`. Both pins are load-bearing.
- NSPanel `hidesOnDeactivate` defaults YES; an Accessory app deactivates
  instantly, so overlay panels must set it false (the popover dismiss-on-blur
  path wants the opposite — see `tray.rs`).
- Alarm policy lives ONLY in `alarm_core.rs` and stays clock-free / side-effect-free.
- Alerts use native NSSound — never webview audio (WKWebView autoplay gating).
- Occurrence identity = `(event_id, occurrence_start)`; the same meeting appears
  once per subscribed calendar and is collapsed by that key.
- Timer precision needs `latencyCritical` windowed ≤120 s before fire;
  `.userInitiated` alone does not defeat App Nap.
