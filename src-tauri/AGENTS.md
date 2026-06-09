# src-tauri/ — Rust core

Accessory (dock-less) menu-bar agent. Modules in `src/`:

- `alarm_core.rs` — PURE decision engine `compute_actions(events, now, state, cfg)`.
  All policy lives here; it must stay clock-free and side-effect-free.
- `scheduler.rs` — tick loop (≤30s, wall-clock armed), fire→overlay+sound,
  snooze/dismiss/pause commands, test-event injection.
- `calendar.rs` — EventKit via `eventkit-rs`; multi-calendar dedup;
  `refresh_sources()` per poll.
- `overlay.rs` / `tray.rs` — nspanel windows (takeover / popover / settings opener).
- `settings.rs` / `state.rs` — persisted user settings / fired-set+snoozes (JSON in
  `~/Library/Application Support/dev.fforres.entucara/`).
- `sound.rs` — native NSSound loop. `testmode.rs` — mock clock + fire log.

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
REAL calendar event) · `ENTUCARA_OPEN_SETTINGS=<1|section>` · `ENTUCARA_SPIKE_DUMP=1`.

## Hard-won gotchas (each cost hours — do not re-learn)

1. **CGWindowList & screencapture cannot see our transparent panels** after
   content loads. Ground truth = `overlay-log.jsonl` heartbeat (test mode).
2. **NSPanel `hidesOnDeactivate` defaults YES** — an accessory app deactivates
   seconds after showing; alarm panels must set it false (popover wants true).
3. **Creating effects/transparent windows during Tauri setup aborts the process**
   (foreign ObjC exception). Scheduler has a 2s startup grace — keep it.
4. **Tauri re-asserts builder `visible:false` after webview load** — pair
   `window.show()` with `panel.order_front_regardless()`.
5. **TCC keys calendar permission to the bundle's code identity** — test against
   the packaged app, never bare binaries; ad-hoc signing has held so far.
6. **Accessory apps won't order-in normal windows** — settings window switches
   activation policy Regular↔Accessory (open_settings / on_window_event).
7. Timer precision needs `NSActivityLatencyCritical` (windowed ≤120s before
   fire); `.userInitiated` alone does NOT defeat App Nap. Measured: 0–1ms on AC.
8. Occurrence identity = `(event_id @ occurrence_start)`; the same meeting
   appears once per subscribed calendar — `dedup_events` collapses by that key.
9. EKEventStore is `!Send` — `refresh_sources` uses a thread_local store, so it
   is safe from BOTH the scheduler tick and the main-thread `fetch_events`
   command (each thread gets its own instance; the store is never shared). It
   does `refreshSourcesIfNecessary` + `reset` so a read SYNCS (picks up external
   deletes/edits) instead of serving this process's first-access cache.

## Do NOT

- Touch alarm policy outside `alarm_core.rs`, or give it access to a clock.
- Initialize plugins/windows before the existing setup order in `lib.rs`.
- Remove the dual write in `testmode::log_fire` / overlay heartbeat — every
  checkpoint script depends on those files.
