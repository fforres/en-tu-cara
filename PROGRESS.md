# PROGRESS — En Tu Cara

> Autonomous-execution state file (PLAN §3). Updated every session.
> Schema: phase / last_passed_auto / human_gates / needs_human / decisions.

## phase

MVP CORE COMPLETE (Phases 0-5 auto-tiers all passed 2026-06-05). Remaining: human
gates (1b fullscreen, popover visual), 1a-freshness probe, packaging polish (Phase 6),
settings (Phase 7), hardening (Phase 8).

## last_passed_auto

- CP0 (2026-06-05) — full ladder incl. bundle + launch assertions
- CP1a (2026-06-05) — EventKit in-bundle: permission persisted across relaunch AND
  rebuild (ad-hoc signing OK so far); occurrence expansion PROVEN (17 series);
  RSVP 126/147; dedup 209→147
- CP1b-auto (2026-06-05) — 3/3 ScreenSaver-level panels (one per display), clean dismiss
- CP1d (2026-06-05, TAGGED) — fire latency on AC power: none/35min-backgrounded = 0 ms
  (the original slow arm completed before the fast rerun), none/5min = 1 ms,
  latencycritical/60s = 1 ms, real-pipeline T-0 = 0.57 s. All ≤5 s target. App Nap
  showed NO measurable throttling on AC w/ 3 displays; battery-condition re-test
  deferred to dogfood; windowed latencyCritical assertion kept regardless (cheap insurance)
- CP2 (2026-06-05) — 54 TS tests: 35 link fixtures (8 providers + generic), 18 classify edges
- CP3-auto (2026-06-05) — injected-event lifecycle in 25 s: T-5 overlay 3/3 displays,
  [t_minus_5, t_zero] sequence, 0.0 s latency, declined never fires. CAUGHT overlay
  recreate crash (ObjC exception → SIGABRT) — fixed via reuse-live-panels
- CP7 (2026-06-06) — settings integration: lead_minutes=1 → early alert scheduled
  exactly 60s before start (fire-log proven); alert_tentative=false suppresses w/
  accepted control firing; future-versioned settings file tolerated. 79 TS tests
  (15 registry/fuzzy + 10 component) + 24 Rust tests. Visual evidence docs/evidence/cp7/
- REAL-PIPELINE E2E (2026-06-05) — real EventKit event created/discovered/fired:
  T-0 latency 0.57 s, Join link extracted from notes, auto-cleanup verified.
  RAM: 76 MB idle / 88 MB with 3 overlay panels (main process RSS)

## human_gates

| gate                                                | last passed | macOS build | notes |
| --------------------------------------------------- | ----------- | ----------- | ----- |
| CP1a (permission + iPhone event)                    | —           | —           |       |
| CP1b (overlay over fullscreen, 2 displays)          | —           | —           |       |
| CP1c (popover positioning, 2 display configs)       | —           | —           |       |
| CP3 (real event T-5/T-0)                            | —           | —           |       |
| CP4 (tray visual parity)                            | —           | —           |       |
| CP5 (Join real browser / sound audible / redaction) | —           | —           |       |
| CP6 (3-day dogfood)                                 | —           | —           |       |

## needs_human

1. **CP1b-human** (~3 min): above-fullscreen overlay test — protocol in
   `scripts/checkpoints/cp1b-human.md`. (Felipe already eyeballed the desktop case
   live on 2026-06-05 — "this shows the overlay" — fullscreen case remains.)
2. **CP1a-freshness** (~2 min now, check back over 24-48 h): create a test event from
   your iPhone while Calendar.app is CLOSED on this Mac; we measure propagation.
3. Decision: daily-smoke scheduling mechanism — local launchd/cron vs Claude /schedule
   remote routine (remote can't touch this Mac's TCC/displays; lean local or
   interactive-session checks).

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
- 2026-06-05 — eventkit-rs CLEARS the go/no-go: occurrence expansion + RSVP + status
  all proven on real data. No objc2-event-kit rewrite.
- 2026-06-05 — Multi-calendar duplication discovered (same meeting once per subscribed
  calendar; 45/209 dup keys): dedup by occurrence_key, prefer my_rsvp > organizer >
  first. occurrence_key collisions across calendars are a FEATURE for alarm dedup.
- 2026-06-05 — OVERLAY BUG CAUGHT BY SPIKE: NSPanel hidesOnDeactivate defaults YES →
  panels vanished ~2 s after show in an Accessory app. Fixed: set_hides_on_deactivate(false)
  - window.show() to sync Tauri's visible-state (else re-asserted on webview load).
- 2026-06-05 — screencapture CANNOT see layer-1000 windows. Overlay verification =
  CGWindowList assertions (scripts/bin/winlist, swiftc one-shot). PLAN's
  "screencapture non-blank" idea replaced; CP5 visual checks should use
  CGWindowListCreateImage per window id instead.

## known limitations / parked

- The 7 settings reference PNGs were lost in a scaffolding accident (2026-06-05);
  `reference-images/SETTINGS-REFERENCE.md` is the substitute. Felipe to re-screenshot
  In Your Face's settings when convenient.
