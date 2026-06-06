# En Tu Cara

Unmissable meeting alerts for macOS. Fully local — reads your calendars via
EventKit (the accounts in System Settings → Internet Accounts), no OAuth, no
servers. Lives in the menu bar; takes over every screen at T-5 minutes and at
meeting start.

**Stack:** Tauri v2 · React + TypeScript (Vite) · Rust. macOS 14+.

| Doc | What's in it |
|---|---|
| `PLAN.md` | Phased build plan, checkpoints, risk register |
| `PROGRESS.md` | Current state, passed checkpoints, NEEDS-HUMAN queue, decision log |
| `framework-research-report.md` | Why Tauri + EventKit (decision record) |
| `reference-images/` | Tray + settings UI references |

## Prerequisites

- macOS 14+, Xcode Command Line Tools (`xcode-select --install`)
- Rust ≥1.95 (`rustup`), Node ≥24, pnpm ≥11
- One-time: `pnpm install`

## Develop

```sh
pnpm tauri dev
```

- Starts Vite (HMR — frontend edits are sub-second) + compiles and runs the Rust
  core (Rust edits trigger a cargo rebuild + app relaunch, ~5–30 s incremental).
- The app is a **menu-bar agent**: no dock icon, no window at launch. Look for
  the tray icon; left-click toggles the popover, right-click → Quit.
- First run prompts for calendar access. Dev (`tauri dev`) and packaged builds
  are different TCC code identities — you may be prompted once for each.
- Calendar permission silently fails if `NSCalendarsFullAccessUsageDescription`
  is missing — it lives in `src-tauri/Info.plist` (merged at bundle time).

### Useful env vars (spikes / debugging)

| Var | Effect |
|---|---|
| `ENTUCARA_TEST_MODE=1` | Enables test IPC (mock clock, injected events) + writes fire log to `~/Library/Application Support/dev.fforres.entucara/fire-log.jsonl` |
| `ENTUCARA_TEST_EVENTS='[{"key":"x","title":"T","start_in":15,"duration":60}]'` | Replace EventKit with synthetic events (seconds relative to launch); needs TEST_MODE |
| `ENTUCARA_SPIKE_OVERLAY=8` | Show the takeover on all displays after 8 s, self-dismiss after 12 s |
| `ENTUCARA_SPIKE_FIRE="300,latencycritical"` | Fire-latency measurement (arm: `none` \| `userinitiated` \| `latencycritical`) → `fire-spike.jsonl` |
| `ENTUCARA_SPIKE_REAL_E2E=70` | Create a REAL calendar event 70 s out, let the full pipeline fire, then auto-delete it |
| `ENTUCARA_SPIKE_DUMP=1` | Dump calendars + ±7 days of events to `spike-dump.json` and keep running |

## Test

```sh
pnpm test                    # frontend: vitest (link extraction, classification, components)
pnpm lint                    # eslint
cargo test  --manifest-path src-tauri/Cargo.toml             # Rust: alarm core, state, dedup
cargo clippy --manifest-path src-tauri/Cargo.toml -- -D warnings
```

### Checkpoint scripts (regression guards — run against a packaged build)

```sh
bash scripts/checkpoints/cp0-auto.sh --launch   # build/test/lint + bundle + launch assertions
bash scripts/checkpoints/cp1a-auto.sh           # EventKit: permission, occurrence expansion, dedup
bash scripts/checkpoints/cp1b-auto.sh           # overlay: 1 ScreenSaver-level panel per display
bash scripts/checkpoints/cp3-auto.sh            # full alarm lifecycle e2e in ~25 s (T-5 → T-0)
```

Notes:
- The scripts `pkill` the app — don't run them while you're depending on alerts.
- Overlay verification uses `scripts/bin/winlist` (CGWindowList) because
  **`screencapture` cannot see ScreenSaver-level windows** — a screenshot will
  look empty even when the takeover is on screen.
- What *cannot* be auto-verified (needs human eyes, see `scripts/checkpoints/cp1b-human.md`):
  overlay-above-another-app's-fullscreen, audible sound, popover look/feel.

### Fast end-to-end against a real calendar

```sh
pnpm tauri build   # if you haven't
DATA="$HOME/Library/Application Support/dev.fforres.entucara"; rm -f "$DATA/fire-log.jsonl" "$DATA/state.json"
ENTUCARA_TEST_MODE=1 ENTUCARA_SPIKE_REAL_E2E=70 \
  "src-tauri/target/release/bundle/macos/En Tu Cara.app/Contents/MacOS/en-tu-cara"
# ~70 s later: takeover on every display with sound. Event auto-deletes. Check:
cat "$DATA/fire-log.jsonl"
```

## Build an installable app

```sh
pnpm tauri build
```

Artifacts:
- `src-tauri/target/release/bundle/macos/En Tu Cara.app` (~10 MB)
- `src-tauri/target/release/bundle/dmg/En Tu Cara_<version>_aarch64.dmg` (~4 MB)

Currently **ad-hoc signed** (no Developer ID on this machine): Gatekeeper will
require right-click → Open on first launch, and TCC permission is keyed to the
build's code identity. With an Apple Developer cert, set `signingIdentity` under
`bundle.macOS` in `src-tauri/tauri.conf.json` and notarize for friction-free
installs. Version lives in `src-tauri/tauri.conf.json` → `version`.

Identity notes (don't change casually):
- Bundle id `dev.fforres.entucara` — calendar permission is keyed to it.
- `tauri-nspanel` is pinned to an exact git rev and `eventkit-rs` to `=0.5.6`
  in `src-tauri/Cargo.toml`; both pins are load-bearing (see PLAN risk register).
