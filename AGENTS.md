# En Tu Cara — agent guide

macOS-only menu-bar app: full-screen takeover alerts for calendar events, read
locally via EventKit (no OAuth, no servers). Tauri v2: React/TS webviews +
Rust core. The product's one unforgivable failure: a meeting started and no
alert fired.

## Commands

```sh
pnpm tauri dev          # run (HMR frontend; Rust edits = rebuild+relaunch)
pnpm tauri build        # packaged .app + .dmg (REQUIRED for checkpoint scripts)
pnpm test && pnpm lint  # vitest + eslint (lefthook also runs oxfmt/oxlint on commit)
cargo test --manifest-path src-tauri/Cargo.toml
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
bash scripts/checkpoints/cp3-auto.sh   # 25s full alarm-lifecycle e2e (see that folder)
```

## Do NOT

- Run checkpoint scripts or `pkill "En Tu Cara"` while the user relies on alerts.
- Change bundle id `dev.fforres.entucara` (calendar permission is TCC-keyed to it).
- Bump `tauri-nspanel` (git-rev pinned) or `eventkit-rs` (=exact) without reading
  the risk register in PLAN.md.
- Trust `screencapture` or CGWindowList for overlay verification — both lie
  about our windows (see src-tauri/AGENTS.md).
- Schedule slow tests: alarm e2e uses injected events seconds out
  (ENTUCARA_TEST_EVENTS), never real waiting.

## Map

- `src/` — React/TS webviews (tray popover, overlay alert, settings). Own guide.
- `src-tauri/` — Rust core (EventKit, scheduler, overlay panels, tray). Own guide.
- `scripts/checkpoints/` — regression gates, two-tier (auto + human). Own guide.
- `assets/icon-options/` — user-curated icon sets; never delete, ids are stable.
- `PLAN.md` / `PROGRESS.md` — phased plan + live state, NEEDS-HUMAN queue,
  decision log. Read PROGRESS.md before starting work; update it after.
- `reference-images/` — UI references (tray-example.png is the tray spec).

User context: Felipe wants autonomous execution and fast iteration; human gates
only for what's physically unverifiable (overlay-over-fullscreen, sound).
