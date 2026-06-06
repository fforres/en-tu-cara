# PROGRESS — En Tu Cara

> Autonomous-execution state file (PLAN §3). Updated every session.
> Schema: phase / last_passed_auto / human_gates / needs_human / decisions.

## phase
Phase 0 — Scaffold, identity & harness (in progress)

## last_passed_auto
- (none yet — cp0 pending first run)

## human_gates
| gate | last passed | macOS build | notes |
|---|---|---|---|
| CP1a (permission + iPhone event) | — | — | |
| CP1b (overlay over fullscreen, 2 displays) | — | — | |
| CP1c (popover positioning, 2 display configs) | — | — | |
| CP3 (real event T-5/T-0) | — | — | |
| CP4 (tray visual parity) | — | — | |
| CP5 (Join real browser / sound audible / redaction) | — | — | |
| CP6 (3-day dogfood) | — | — | |

## needs_human
- (queue is empty)

## decisions
- 2026-06-05 — Stack: Tauri v2, local-only EventKit (see report Decision Record).
- 2026-06-05 — No Developer ID cert on this machine (`security find-identity` → 0). Using
  stable ad-hoc signing. RISK: TCC permission may not persist across REBUILDS with ad-hoc
  identity (it keys on code identity). CP1a must explicitly test permission-across-rebuild;
  durable fix = Apple Developer Program cert. ESCALATE to Felipe if CP1a shows re-prompting.
- 2026-06-05 — tauri-nspanel pinned to rev a3122e89 (v2.1 branch, = v2.1.0). eventkit-rs
  pinned =0.5.6. objc2/objc2-app-kit/objc2-foundation added for observer + fallback work.
- 2026-06-05 — Test-mode slice (mock clock + fire log) built in Phase 0 per PLAN v3
  dependency fix (CP1d needs it).

## known limitations / parked
- The 7 settings reference PNGs were lost in a scaffolding accident (2026-06-05);
  `reference-images/SETTINGS-REFERENCE.md` is the substitute. Felipe to re-screenshot
  In Your Face's settings when convenient.
