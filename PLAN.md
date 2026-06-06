# En Tu Cara — Build Plan v3 (final after two adversarial reviews)

> macOS menu-bar meeting-reminder app. Tauri v2 + React/TS + Rust. Fully local:
> EventKit only (macOS Internet Accounts), no OAuth, no servers.
> Plan owner: Claude (autonomous execution) + Felipe (human gates).
> v1 → v2 → v3 on 2026-06-05 after two independent adversarial reviews (§7 changelog).
> Stack rationale: `framework-research-report.md` → Decision Record.

## 0. Product definition (MVP)

**Goal: never miss a meeting.** One unforgivable failure: a meeting started and no alert fired.
**Second-order failure that kills adoption: alert fatigue** — alerting for declined meetings, takeovers while presenting, un-dismissable pile-ups. The MVP must avoid both.

1. Read events from ALL calendars enabled in macOS (EventKit / Internet Accounts).
2. Alert **5 minutes before start** and **at start** (hard-coded leads for MVP) — except: **declined events never alert**; canceled events never alert; all-day events never alert (still listed in tray).
3. Alert = full-screen takeover overlay (above fullscreen apps, all Spaces, all displays): event info, Join (only when a video link exists; hidden otherwise), Dismiss, Snooze 1m/5m, native sound. Multiple simultaneous/overlapping events render in ONE overlay as stacked cards with per-event actions.
4. Tray icon → popover: **ongoing** ("Xm remaining" + pie) and **upcoming** (day-grouped, today/all toggle), calendar color + account, time range, recurrence icon, camera icon/link. Ref: `reference-images/tray-example.png`.
5. Post-MVP: VS Code-style settings (sidebar TOC + fuzzy search): General, Alerts, Calendars, Event Filters, Appearance, Menu Bar, Advanced. Ref: `reference-images/SETTINGS-REFERENCE.md`.

**Honest physics (documented, not bugs):** a Mac in system sleep cannot fire an alert (timers pause in sleep — Apple-documented); policy = fire immediately on wake if event still ongoing. EventKit freshness without Calendar.app running is unproven — Phase 1 probes it and a pre-committed fallback exists (§2 CP1a).

**Non-goals (MVP):** Windows, OAuth, Apple Reminders, theme marketplace, travel time, Shortcuts, auto-update, App Store, waking a sleeping Mac.

## 1. Architecture

```
en-tu-cara/
├── src/                       # React + TS (Vite, HMR)
│   ├── windows/tray/          # tray popover UI
│   ├── windows/overlay/       # fullscreen alert (multi-event stacked cards)
│   ├── windows/settings/      # Phase 7
│   ├── lib/                   # pure TS: formatting, grouping, countdown math
│   └── ipc/                   # typed invoke() wrappers
├── src-tauri/src/
│   ├── calendar.rs            # EventKit commands (eventkit-rs OR objc2-event-kit — CP1a decides)
│   ├── calendar_watch.rs      # EKEventStoreChanged observer (objc2, MAIN-THREAD) + 60s poll backstop
│   ├── scheduler.rs           # wall-clock arming, windowed latency assertion, sleep/wake hooks
│   ├── alarm_core.rs          # PURE: compute_actions(events, now, state) -> Vec<AlarmAction>
│   ├── overlay.rs             # tauri-nspanel per-screen panels + display-config-change observer
│   ├── sound.rs               # native NSSound (never webview audio)
│   ├── tray.rs                # tray icon + nspanel popover positioned from TrayIconEvent rect
│   ├── state.rs               # persisted: fired-set, snooze deadlines, pause flag (+GC)
│   └── testmode.rs            # ENTUCARA_TEST_MODE debug IPC (minimal harness built in Phase 0)
└── scripts/checkpoints/       # cpN-auto.sh (headless tier) + cpN-human.md (gate protocol + log)
```

### Load-bearing design decisions (each traces to a verified risk)

**Data**
- **EventKit is the database.** Change detection: `EKEventStoreChanged` observer + 60s poll backstop + refetch-on-wake. The observer is NOT in eventkit-rs (verified) — we add an objc2 `NSNotificationCenter` observer **registered on the main thread** (live run loop required; NSNotificationCenter delivers on the posting thread), with all reactions marshaled to main via `run_on_main_thread` / objc2 `MainThreadMarker`. CP3 proves the observer in isolation (poll disabled) — a dead observer must FAIL a check, not hide behind the poll.
- **EventDto** includes `attendee_status` (accepted/tentative/declined/needs-action), `is_organizer`, `event_status` (confirmed/canceled), `availability`, `event_timezone` — alarm policy needs them (declined→skip; canceled→skip; tentative→alert in MVP, configurable later).
- **Occurrence identity is THE data-model risk**: key = `(event_id, occurrence_start)`. `eventIdentifier` is unstable across series edits; eventkit-rs does not document occurrence expansion. CP1a proves it or we switch to objc2-event-kit (`eventsMatchingPredicate` natively expands). NOTE: the Decision Record's "~200–250 lines of Rust" estimate is conditional on eventkit-rs passing — the objc2 path is meaningfully more code.

**Timing (the "never miss" engine)**
- **Wall-clock arming, not interval polling**, in Rust (webview timers throttle).
- **App Nap**: timer precision requires `NSActivityLatencyCritical` — `.userInitiated` alone does NOT promise timer fidelity (Apple Energy Efficiency Guide; review-2 correction). We hold `beginActivity(.userInitiated | .latencyCritical)` **only in a window**: acquire when soonest alarm ≤ 2 min out, release after fire. Holding it 24/7 would tank battery and contradict the footprint rationale.
- **Sleep ≠ App Nap**: mach timers pause during system sleep; `beginActivity` does not help. `NSWorkspace.willSleep/didWake` hooks: on wake → immediate refetch + `compute_actions` pass. Missed-while-asleep policy: **fire-on-wake if event still ongoing; skip if already ended.**
- Precision target (honest): fire within **±5 s while awake**; on wake otherwise.

**Presentation**
- **Sound native (NSSound)** — WKWebView autoplay gating makes webview audio unfit for alarms.
- **Tray popover is a nonactivating nspanel** positioned from `TrayIconEvent` rect (Tauri does not auto-position; multi-display + menu-bar-on-secondary handled). Hide on focus-lost; never steal focus.
- **Overlay panels per NSScreen** + display-config-change observer (monitor hot-plug between arm and fire re-targets panels).
- **Screen-sharing awareness (privacy)**: full takeover while presenting broadcasts your next meeting's title. MVP decision: **detect active capture (`CGDisplayStream`/`SCShareableContent` check) and if sharing, show the overlay with title REDACTED ("Upcoming meeting") + sound**. Full redaction options post-MVP.
- **Focus/DND**: MVP **overrides** Focus modes deliberately (the product's contract is "never miss"); revisit as a setting (Open Question #4).
- **Auto-close failsafe: default OFF for MVP** (review-2: auto-closing an unactioned alert contradicts the prime directive). Setting later.

**State & semantics**
- Persistence (`tauri-plugin-store`, `state.json`): `fired: {key: firedAtIso}`, `snoozes: {key: fireAtIso}`, `paused: bool`. GC on write: drop entries with occurrence end < now − 48 h.
- **Pause**: scheduler keeps computing and advances fired-state; suppresses presentation only. Un-pause does NOT replay. Snooze deadlines survive restart.
- **Back-to-back pile-up** is a first-class policy case: T-0(A) coinciding with T-5(B) → one overlay, both cards; snooze acts per-card.

**Verification**
- **Test mode** (`ENTUCARA_TEST_MODE=1`) debug IPC: `inject_events`, `set_mock_now`, `advance_clock`, `get_window_registry`, `get_fired_log`. Substitutes for tauri-driver (no macOS support). The minimal slice needed by CP1d (mock clock + fire log) is built in **Phase 0**, not Phase 3.
- **Checkpoint scripts are two-tier**: `cpN-auto.sh` (headless, re-runnable) + `cpN-human.md` (gate protocol; records last-passed date + macOS build). The regression guard re-runs ONLY auto tiers and **flags any human gate whose macOS version has drifted since last pass.**

## 2. Phases & checkpoints

❗HUMAN = Felipe required. Phase-1 spikes are go/no-go; Phase 2+ does not start until 1a/1b/1d pass (1a-freshness runs in parallel over days).

### Phase 0 — Scaffold, identity & harness (scaffold DONE)
- [x] Tauri v2 + React-TS scaffold, git, deps, first compile
- [ ] **Stable bundle identity NOW** (review-2): fixed bundle id `dev.fforres.entucara`, Developer ID signing if available else stable ad-hoc, `NSCalendarsFullAccessUsageDescription`, working `tauri build` — TCC keys grants to code identity; all Phase-1 spikes run against THIS identity
- [ ] Deps pinned: store/opener/autostart/single-instance plugins; **tauri-nspanel pinned to exact git rev on its v2 branch**; eventkit-rs pinned
- [ ] Accessory activation policy + tray stub
- [ ] Minimal test-mode slice: mock clock + fire log IPC (needed by CP1d)
- [ ] vitest + cargo test + clippy + eslint; `PROGRESS.md` created (schema: current phase / last-passed auto checkpoints / human-gate log w/ dates + macOS build / NEEDS-HUMAN queue / decisions log)
- **CP0-auto:** build ✓ check ✓ tests ✓ lint ✓; launch: tray present, no dock icon, **no focus steal** (`lsappinfo` frontmost unchanged).

### Phase 1 — De-risk gates

**1a. EventKit-in-bundle spike** (in the Phase-0 signed bundle)
- Request full access; list calendars (name/color/account); dump ±7 days JSON
- **Occurrence-identity proof**: known recurring series → N distinct rows, distinct `occurrence_start`s, stable key. Fail → switch to objc2-event-kit (decision logged).
- **RSVP proof**: dump includes attendee_status/organizer/canceled fields (whichever lib).
- **Freshness probe (runs over 24–48 h, parallel to other phases):** events created on iPhone/web with Calendar.app closed on Mac — measure propagation. **Pre-committed fallback ladder** if stale: (i) trigger EventKit refresh on poll/wake and re-measure → (ii) detect-and-prompt "open Calendar.app in background" notice → (iii) if even that fails, escalate to Felipe: local-only premise needs a rethink (documented NO-GO path, not a shrug).
- **CP1a-auto:** JSON schema valid; recurrence expands; permission persists across relaunch (same identity). **CP1a-human:** grant prompt; iPhone test event.

**1b. Overlay spike** (packaged, signed build)
- nspanel: borderless, screenSaver level, canJoinAllSpaces+fullScreenAuxiliary+stationary, per-screen, **timer-triggered while another app is fullscreen & focused** (the no-interaction case)
- **CP1b-human:** overlays fullscreen Zoom + Chrome on built-in AND external display, no interaction. Evidence → `docs/evidence/cp1b/`. Log macOS build.

**1c. Tray-popover spike** (same human session as 1b)
- nspanel popover from tray rect; hide on focus-lost; no focus steal; correct with menu-bar-on-secondary-display
- **CP1c-human:** both display configs pass.

**1d. Fire-reliability spike**
- Arm alarm 35 min out; app backgrounded (battery if possible); **three arms: no assertion / `.userInitiated` only / `.userInitiated|.latencyCritical`** — record latency for each (data-driven, not assumed)
- **CP1d-auto:** parse fire log: latency ≤5 s in the latencyCritical arm. Uses Phase-0 test-mode slice.
- **GO/NO-GO:** 1a-identity fail → objc2 rewrite (stay). 1b fail → stack fallback per report. 1a-freshness fail → fallback ladder. 1c/1d fail → design change (stay).

### Phase 2 — Calendar domain core
- EventDto incl. attendee_status, is_organizer, event_status, availability, event TZ
- Link extraction (TS): Zoom, Meet, Teams, Webex, Jitsi, Whereby, Around, Discord + generic; fixture matrix = **each provider × {location, notes, url-field}** + adversarial (multiple links → priority; tracking-wrapped links; links in HTML notes) — ≥30 cases, not 30 Zoom links
- Classification: ongoing/upcoming/past, remaining-time, day grouping, today/all
- Edge tests: starts-exactly-now; spans-midnight; all-day; zero-duration; **event TZ ≠ local**; emoji titles; canceled; declined
- **CP2-auto:** all green.

### Phase 3 — Scheduler & alarm engine
- `compute_actions` pure core: T-5/T-0; declined/canceled→skip; tentative→alert (MVP); snooze; pause; missed-while-asleep fire-if-ongoing; dedup
- Wall-clock arming + windowed latencyCritical assertion (from 1d data); willSleep/didWake hooks; EKEventStoreChanged observer (main-thread registration per §1) + 60s poll + wake refetch
- Persistence + GC; snooze survives restart
- Edge tests: created-90s-before-start (only T-0); moved/cancelled after arming; DST spring-forward across T-5 window; **back-to-back pile-up (T-0(A)+T-5(B) → one overlay, two cards, per-card snooze)**; restart mid-snooze; declined event never fires
- **CP3-auto:** simulated-timeline e2e (≥8 mock events covering edges) → exact action log; **observer-isolation test: poll disabled, real store mutation → observer fires** (dead observer = red). **CP3-human (5 min):** real event 6 min out → both alerts.

### Phase 4 — Tray popover UI
- Build on 1c. Sections per tray-example.png; pause toggle; gear/eye stubs
- **CP4-auto:** component tests; data parity vs CP1a dump; calendar-edit reflected ≤ **75 s** (60s poll + render margin — SLA matches mechanism). **CP4-human:** structure parity vs reference via screencapture.

### Phase 5 — Overlay alert UX
- Per-display panels + hot-plug observer; ticking countdown; color/source; **Join hidden when no link**; Dismiss; Snooze 1m/5m; native sound; multi-event stacked cards; auto-close OFF; **screen-share redaction** (per §1)
- **CP5-auto:** test-mode e2e: fire → registry shows panel/display + non-blank screencaptures; snooze re-fires; Join via opener-mock; no-link event renders without Join; redaction state togglable in test mode. **CP5-human:** packaged Join opens real browser once; **"I heard the sound"**; redaction sanity while Zoom-sharing.

### Phase 6 — Packaging & daily-driver beta
- Icon, DMG, autostart default-on, single-instance
- Cold-boot: login-item launch pre-network → first fetch empty → poll recovers (automated)
- Resources: idle RSS < 120 MB; with overlays on 2 displays < 250 MB (10-min sampler)
- **CP6-human:** 3-day dogfood; zero missed meetings; zero wrong-alerts (declined/canceled); issues triaged.

### Phase 7 — Settings (VS Code style)
- Registry-driven ({id, section, label, description, keywords, control, default}) → TOC + fuzzy search (filter+highlight)
- Priority: Calendars → Alerts (leads, sound, snooze durations, auto-close toggle, tentative policy) → Event Filters → General → Menu Bar → Appearance → Advanced (per SETTINGS-REFERENCE.md)
- Live-apply; persist
- **CP7-auto:** fuzzy e2e ("snooze" → highlight in Alerts); calendar toggle → tray ≤75 s; survives restart.

### Phase 8 — Hardening
- Permission revoked mid-run → tray banner + System Settings deep-link; 0 calendars; 300 events; week-long sleep → no storm; monitor hot-plug at fire time
- **CP8-auto:** chaos checklist; resource re-check. Tag v0.1.0.

## 3. Autonomous execution protocol

- **Session** = one Claude working block (interactive or scheduled). Start: `git status` clean → run ALL passed `cp*-auto.sh` (regression guard; human tiers are NOT re-run — instead flag any human gate whose recorded macOS build ≠ current) → read `PROGRESS.md`. End: commit; update `PROGRESS.md` (incl. NEEDS-HUMAN queue).
- **PROGRESS.md schema (created in Phase 0):** `phase`, `last_passed_auto` (list), `human_gates` (gate, date, macOS build), `needs_human` (queue with what/why/est-minutes), `decisions` (dated log — e.g., 1a library choice).
- **Scheduled checks (concrete; set up via /schedule at build kickoff, owner: Claude):**
  - **Per session** (every working block): full test+lint suite
  - **Daily 09:00 during active dev:** smoke — build, test-mode launch, inject events, assert action log, screencapture tray+overlay, RAM sample; report deltas to PROGRESS.md
  - **Per phase gate:** full auto-ladder cp0..cpN
  - **After any macOS update detected (`sw_vers` change):** flag ALL human overlay gates stale → top of NEEDS-HUMAN
- **Cannot be automated (honest list):** overlay-above-fullscreen truth (screencapture ≠ z-order proof — only recurring human gates guard it), audible sound, TCC prompts, screen-share redaction realism. Everything else self-serves.
- **Never:** force-flags on scaffolding tools, destructive ops without `git status` first, push without ask.

## 4. Risk register

| Risk | Mitigation | Phase |
|---|---|---|
| TCC grant keyed to wrong identity | Stable signed bundle id from Phase 0; spikes use it | 0–1a |
| eventkit-rs lacks occurrence expansion/RSVP | Proven or replaced at gate; Rust estimate flagged conditional | 1a |
| EventKit stale without Calendar.app | Freshness probe + pre-committed fallback ladder (refresh→notice→escalate) | 1a |
| App Nap throttling | ✅ MEASURED (cp-1d): 0–1 ms all arms incl. 35-min no-assertion on AC; windowed latencyCritical kept as cheap insurance; battery re-test in dogfood | 1d |
| System sleep ≠ App Nap | willSleep/didWake hooks; fire-on-wake-if-ongoing; honest target | 3 |
| Dead EKEventStoreChanged observer masked by poll | Main-thread registration; observer-isolation test (poll disabled) | 3 |
| tauri-nspanel breaks/abandoned | Pinned rev; isolated overlay.rs; objc2 fallback recipe in report | 1b |
| WKWebView audio unreliable | Native NSSound | 5 |
| Overlay regression invisible to automation | Recurring human gates; macOS-version drift flagging | all |
| Alert storm after long sleep | Persisted fired-set + skip-if-ended + GC | 3 |
| Privacy leak while screen-sharing | Capture detection + redacted overlay | 5 |
| Alert fatigue (declined/canceled) | RSVP/status in EventDto; skip policies in core | 2–3 |
| Monitor hot-plug at fire time | Display-config observer; CP8 chaos test | 5,8 |

## 5. Open questions (non-blocking)
1. T-5 alert: full takeover vs banner? (MVP: full; revisit after dogfood)
2. Multi-event card layout design (CP5 task)
3. All-day events in tray placement (MVP: under day header, never alert)
4. Focus/DND: MVP overrides; should a setting respect it later?
5. Redaction depth while screen-sharing (MVP: title only)

## 6. Definition of done (MVP = end of Phase 6)
Packaged, signed app that for 3 consecutive dogfood days: fired T-5 and T-0 for every accepted/tentative non-all-day meeting (±5 s awake, on-wake if asleep), never alerted for declined/canceled events, showed correct tray data within 75 s of edits, survived sleep/wake/restart without replay or storm, idle < 120 MB.

## 7. Changelog
- **v2 (review #1):** EventKit spike → app bundle (TCC); occurrence-identity + freshness proofs in CP1a; App Nap spike; tray-popover spike; EKEventStoreChanged observer added; native sound; pause/snooze/persistence semantics + GC; honest automation limits; multi-event overlay; cold-boot; TZ tests; nspanel pinned.
- **v3 (review #2):** `.latencyCritical` correction (was `.userInitiated` — insufficient for timer precision) + windowed assertion (not 24/7); sleep-vs-App-Nap split + fire-on-wake-if-ongoing pinned; observer main-thread/run-loop design + observer-isolation test; freshness fallback ladder pre-committed; Phase-0 stable signing identity + test-mode slice (dependency fixes); PROGRESS.md schema + two-tier checkpoints + concrete cron cadence + macOS-drift flagging; product gaps: RSVP/canceled skip policies, no-link Join, back-to-back pile-up, screen-share redaction, Focus/DND decision; auto-close default OFF; CP4 SLA 75 s; fixture coverage matrix; Rust estimate flagged conditional; display hot-plug.
