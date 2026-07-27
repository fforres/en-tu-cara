# src-tauri/ — Rust core

Accessory (dock-less) menu-bar agent. Modules in `src/`:

- `alarm_core.rs` — PURE decision engine `compute_actions(events, now, state, cfg)`.
  All policy lives here; it must stay clock-free and side-effect-free.
- `scheduler.rs` — tick loop (≤30s, wall-clock armed), fire→present, alarm-policy
  commands (snooze/dismiss/ignore/pause), test-event injection.
- `presentation.rs` — THE owner of "what is on screen": the presented card set,
  with the panel + sound lifecycle DERIVED from it. All overlay state changes go
  through `present` / `reassert` / `finish` / `finish_all`; nothing else may open
  or close panels or touch the card set. See "Takeover presentation" below.
- `calendar.rs` — EventKit via `eventkit-rs`; multi-calendar dedup;
  `sync_event_store()` (refresh remote + reset local cache) per poll.
- `overlay.rs` / `tray.rs` — nspanel windows (takeover / popover / settings opener).
- `access.rs` — PURE calendar-access-health machine (`classify` + debounced
  `AccessTracker`) + the macOS loud surfaces. See "Calendar access health" below.
- `settings.rs` / `state.rs` — persisted user settings / fired-set+snoozes (JSON in
  `~/Library/Application Support/dev.fforres.entucara/`).
- `sound.rs` — native NSSound loop. `testmode.rs` — mock clock + fire log.
- `obs.rs` — logging backbone: ONE tracing subscriber → rolling local file
  (always on) + WARN+ PostHog events + INFO+ PostHog Logs (OTLP/JSON), shipping
  gated by the opt-out. Bridges existing `log::` calls via tracing-log.
  `telemetry.rs` — PostHog event pipeline (opt-out, drop-on-full worker).
  `identity.rs` — startup code-signing-identity log (the lost-access smoking gun).

## Interacting

```sh
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings   # warnings are errors
pnpm tauri build    # checkpoint scripts run the PACKAGED app, never `cargo run`
```

Test/debug env vars (set on the packaged binary, `…/MacOS/en-tu-cara`):
`ENTUCARA_TEST_MODE=1` (test IPC + fire-log/overlay-log jsonl on disk) ·
`ENTUCARA_TEST_EVENTS='[{"key":"k","title":"T","start_in":15,"duration":60,"my_rsvp":"accepted"}]'` ·
`ENTUCARA_SILENT=1` (no sound — use in every scripted run) ·
`ENTUCARA_SPIKE_OVERLAY=<s>` · `ENTUCARA_SPIKE_REAL_E2E=<s>` (creates+deletes a
REAL calendar event) · `ENTUCARA_OPEN_SETTINGS=<1|section>` · `ENTUCARA_SPIKE_DUMP=1` ·
`ENTUCARA_PREVIEW=popover|overlay` (open that window's UI in a NORMAL resizable
window seeded with MOCK data — `?preview=1` → src/windows/tray/preview-data.ts —
to inspect the popover's list/scrolling OR the takeover's "Calendar origins"
account list, with NO real calendar access or full-screen takeover; no TCC click
needed) ·
`ENTUCARA_TELEMETRY=on|off` (force/kill log+event shipping; default follows the
Settings toggle, off in test mode unless `on`) ·
`ENTUCARA_TEST_ACCESS='<reason>[,recover_after=<secs>]'` (test-mode only: FORCE
the calendar-access reading — `reason` ∈ `ok|fetch_failed|not_determined|denied`
— so the lost→announce→recover flow + loud surfaces are testable without real
EventKit failures; `recover_after` flips it healthy after N secs).

## Hard-won gotchas (each cost hours — do not re-learn)

1. **CGWindowList & screencapture cannot see our transparent panels** after
   content loads. Ground truth = `overlay-log.jsonl` heartbeat (test mode).
2. **NSPanel `hidesOnDeactivate` defaults YES** — an accessory app deactivates
   seconds after showing; alarm panels must set it false (popover wants true).
3. **Creating effects/transparent windows during Tauri setup aborts the process**
   (foreign ObjC exception). Scheduler has a 2s startup grace — keep it.
4. **Tauri re-asserts builder `visible:false` after webview load** — pair
   `window.show()` with `panel.order_front_regardless()`.
5. **TCC keys calendar permission to the bundle's code IDENTITY (signature), not
   the bundle id.** CI releases are Developer-ID signed + notarized (STABLE
   identity → the grant survives auto-updates). LOCAL builds are ad-hoc
   (`signingIdentity:"-"`) and `tauri dev` debug binaries are ad-hoc/unsigned —
   a DIFFERENT identity under the SAME bundle id `dev.fforres.entucara`. Running
   those alongside the Developer-ID app makes macOS RESET the grant to
   NotDetermined, and the in-app prompt can get wedged (fix: `tccutil reset
   Calendar dev.fforres.entucara`). So: test against the packaged app; expect
   dev runs to churn the prod grant; the startup identity log (identity.rs)
   makes an identity change visible. PREVENTION: `pnpm tauri:dev` builds under a
   SEPARATE bundle id (`dev.fforres.entucara.dev`, via tauri.dev.conf.json) so
   local runs get their own TCC grant and never reset the release's; the
   startup identity guard warns if an ad-hoc build runs under the prod id. The
   app must SELF-HEAL regardless — see "Calendar access health".
6. **Accessory apps won't order-in normal windows** — settings window switches
   activation policy Regular↔Accessory (open_settings / on_window_event).
7. Timer precision needs `NSActivityLatencyCritical` (windowed ≤120s before
   fire); `.userInitiated` alone does NOT defeat App Nap. Measured: 0–1ms on AC.
8. Occurrence identity = `(event_id @ occurrence_start)` (fired-set/snooze/ignore
   key). But `dedup_events` collapses on CONTENT — `(normalized title, start,
   end)` — to catch BOTH the same event seen via many subscribed calendars (same
   id) AND the same meeting living as SEPARATE events in two accounts (different
   ids; e.g. a business calendar shared into a personal Gmail). It keeps the
   user's own copy (my_rsvp > organizer > first) and accumulates every
   contributing calendar onto the survivor's `EventDto.calendars` so the popover
   can list each account. Untitled holds fall back to the occurrence key so blanks
   don't merge. One meeting therefore alerts ONCE even across accounts.
9. EKEventStore is `!Send` — `sync_event_store` uses a thread_local store, so it
   is safe from BOTH the scheduler tick and the main-thread `fetch_events`
   command (each thread gets its own instance; the store is never shared). It
   does `refreshSourcesIfNecessary` + `reset` so a read SYNCS (picks up external
   deletes/edits) instead of serving this process's first-access cache. The store
   is also tagged with a GENERATION: `invalidate_event_store()` bumps it so each
   thread rebuilds its store on its next read — because a store whose connection
   to the calendar daemon dies (after sleep, or a TCC reset) keeps returning
   stale "no data" forever otherwise. Bumped on wake (scheduler drift detection)
   and on every Lost read (self-heal).
10. **`window.set_position` / `set_size` cannot move a window between displays of
    different scale factors.** tao converts with the SOURCE window's scale
    (`position.to_logical(self.scale_factor())`), so a target computed from
    another display lands at half/double coordinates — and since the window then
    never reaches the target, any "is it placed correctly?" loop re-issues the
    move forever. Both windows that move across screens drive the native
    `NSWindow` frame instead, in AppKit points, one coordinate space, no
    physical/logical mixing: `tray::position_under_tray` (popover) and
    `overlay::resync_overlay_geometry` (takeover panels after a display change).
    Extract a shared helper if a third caller appears. Related: tauri#5229,
    tauri#7139. Bonus reason to avoid the tauri path here — `available_monitors()`
    costs 0.2–1 ms of main-thread AppKit work (it makes O(displays²)
    `CGDisplayCreateUUIDFromDisplayID` round-trips); `NSScreen::screens` is ~1000×
    cheaper and matters on a per-tick path.

## Calendar access health (never fail silently)

The one unforgivable failure is a meeting that fires no alert. A subtle way it
happened in prod: the running process LOST calendar access mid-session — either
`authorization_status` flipped to NotDetermined (gotcha #5), or the EKEventStore
went stale after sleep (`authorization_status` still says FullAccess but reads
return nothing). The old code silently yielded 0 events and fired nothing.

Design (all reads go through ONE extraction — `calendar::active_events` — which
returns `Err` when access is unavailable):

- **Detect:** `scheduler::tick` feeds `(auth, fetch_outcome)` to the pure
  `access::classify` each real read. `FullAccess + Failed` → Lost (the stale-store
  case); NotDetermined/Denied → Lost.
- **Debounce (access.rs `AccessTracker`):** announce a transition only after
  `CONFIRM_TICKS` (2) consecutive opposite readings. This is load-bearing: the
  self-heal rebuilds the store on a Lost read, so a transient stale store
  recovers on the NEXT tick and the debounce never announces (silent self-heal);
  a flapping grant can't spam lost/restored notifications. Persistent loss
  announces once.
- **Shout (edge-triggered, non-blocking):** macOS notification + menu-bar ⚠️
  badge (`tray::set_access_badge`, overrides the next-event title) + a
  `access-state-changed` event → Settings banner AND the tray-popover banner
  ("events may be outdated"; the popover keeps showing last-known events via
  preserve-on-failure). `get_access_state` lets a window pull on mount.
- **Self-heal:** `invalidate_event_store()` on every Lost read (rebuild the
  store), re-prompt if NotDetermined (cooldown'd; reuses `prompt_access_off_main`
  + relaunch-on-grant), and a wake-from-sleep refresh (wall-clock drift, NOT a
  fragile NSWorkspace observer).
- **Test it:** `ENTUCARA_TEST_ACCESS` forces the reading deterministically (real
  EventKit loss is hard to simulate — `tccutil reset` on a RUNNING process isn't
  observed because EventKit caches auth in-process). The pure machine is
  exhaustively unit-tested (classify, debounce, flap, silent-self-heal, recovery).

## Takeover presentation (panels are a function of the card set)

`presentation.rs` owns "what is on screen". ONE rule: the panels and the alert
sound are DERIVED from the presented card set, and only that module may change
either side. Every takeover bug in this project's history was a state where the
two disagreed:

- **cards present, panels gone** → an audible alarm with nothing to dismiss; the
  user's only escape was force-quitting (reported). Cured by `reassert`, run from
  every scheduler tick: it re-frames/re-fronts surviving panels and rebuilds lost
  ones. A rebuilt panel repopulates by pulling `get_active_alarms` on mount, which
  is why the cards must outlive the panels.
- **panels present, cards gone** → a blank takeover with the sound looping, which
  `reassert` then faithfully resurrects every tick. This is why `reassert` re-reads
  the card set on the MAIN THREAD (the scheduler's own check is just an
  optimization to skip the hop), and why `close_overlays` is no longer an IPC
  command — being callable from the webview put that state one `invoke` away.

The serialization that makes this safe: every mutation runs on the main thread —
the fire path dispatches via `run_on_main_thread`, and the commands that reach
here are all SYNC `#[tauri::command]`s, which Tauri dispatches inline on the main
thread. Marking one of them `async` moves it to the async runtime and reopens the
resurrection race; if you ever need that, the panel lifecycle needs a real lock,
not a re-read.

Panel geometry is driven through the native `NSWindow` frame from `NSScreen`
frames, NOT `window.set_position`/`set_size` — see gotcha #10.

## Do NOT

- Touch alarm policy outside `alarm_core.rs`, or give it access to a clock.
- Open/close overlay panels, start/stop the alert sound, or mutate the presented
  card set anywhere but `presentation.rs`. That module's whole purpose is that
  "cards on screen" and "panels on screen" can never disagree.
- Initialize plugins/windows before the existing setup order in `lib.rs`.
- Remove the dual write in `testmode::log_fire` / overlay heartbeat — every
  checkpoint script depends on those files.
- Add per-tick `log::warn!`s on a recurring failure path (e.g. "not authorized"):
  the obs layer ships WARN+ to PostHog, so per-tick warns become event spam. The
  access machine owns the loud signal (once per edge); keep recurring per-poll
  failures at `debug`.
- Re-add a second event-extraction path: everything reads `calendar::active_events`
  so the tray list, menu-bar title, alarm scheduler, and access machine can't
  disagree about what exists.
