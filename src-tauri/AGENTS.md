# src-tauri/ — Rust core

Accessory (dock-less) menu-bar agent. Modules in `src/`:

- `alarm_core.rs` — PURE decision engine `compute_actions(events, now, state, cfg)`.
  All policy lives here; it must stay clock-free and side-effect-free.
- `scheduler.rs` — tick loop (≤30s, wall-clock armed), fire→overlay+sound,
  snooze/dismiss/pause commands, test-event injection.
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
   makes an identity change visible. The app must SELF-HEAL regardless — see
   "Calendar access health".
6. **Accessory apps won't order-in normal windows** — settings window switches
   activation policy Regular↔Accessory (open_settings / on_window_event).
7. Timer precision needs `NSActivityLatencyCritical` (windowed ≤120s before
   fire); `.userInitiated` alone does NOT defeat App Nap. Measured: 0–1ms on AC.
8. Occurrence identity = `(event_id @ occurrence_start)`; the same meeting
   appears once per subscribed calendar — `dedup_events` collapses by that key.
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

## Do NOT

- Touch alarm policy outside `alarm_core.rs`, or give it access to a clock.
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
