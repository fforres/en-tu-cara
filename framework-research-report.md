# Building an "In Your Face" Clone — Framework Research Report

_Research date: June 5, 2026. Produced from 28 research + adversarial-verification agents (433 tool calls) plus direct product research. Every framework's riskiest claims were independently fact-checked against current docs, GitHub issues, and — in one case — the actual shipping binary._

---

## 1. What we're cloning

**In Your Face** (inyourface.app, Blue Banana Software, $3.99/mo · $24.99/yr · $69 one-time):

- Full-screen takeover alert when a meeting starts — blocks the whole screen (optionally **all** monitors), modal, snooze 1/5 min, custom sounds, "join meeting" button (detects 30+ conferencing services)
- Calendar sources: **macOS Internet Accounts via EventKit** (iCloud/Google/Exchange, zero OAuth) plus direct Google/Microsoft OAuth for extra accounts
- Menu-bar app (no dock icon), launch at login, per-calendar/per-event filters, themes, widgets
- macOS 14+, Windows (MS Store), iOS, watchOS

### 🔴 The headline finding

A verification agent **downloaded the current shipping macOS build (v3.25.5) and inspected the binary**: In Your Face is a **native Swift/SwiftUI + AppKit app** — `otool -L` shows SwiftUI, SwiftData, AppKit, **EventKit**, FoundationModels; bundled **Sparkle** (native auto-updater) and Lottie; compiled storyboards; a WidgetKit extension; ~20 MB download. **Zero Electron artifacts.** The product you're cloning chose native Swift for exactly this feature set.

### 🔴 The hardest technical truth (applies to ALL frameworks)

"Always-on-top above another app's fullscreen Space" is **not a guaranteed macOS behavior**. Verified across Apple docs, Apple dev forums (thread 26677), iTerm2's regression (#9404, broke in macOS 11.1):

- `.fullScreenAuxiliary` officially means "can coexist with **your own** app's fullscreen window" — not "floats above any app's fullscreen Space"
- The combination that works in practice in shipping apps: **borderless `NSPanel` + `.nonactivatingPanel` + `level = .screenSaver` (or statusBar) + `[.canJoinAllSpaces, .fullScreenAuxiliary, .stationary, .ignoresCycle]` + the app running as an Accessory/agent (LSUIElement, no dock icon)**. The _activation policy_ is the real lever — Tauri issue #11488 and Electron's docs both confirm the dock-less agent configuration is what makes it work post-packaging.
- It will never draw over true _exclusive_ fullscreen (games, captured displays) on any framework. Fine for Zoom/Meet/Teams use cases.
- **This is good news for us**: our app is _inherently_ a dock-less menu-bar agent, which is precisely the configuration where this works. But it must be validated on real hardware, in a packaged/notarized build, on every macOS major release. No framework can verify it headlessly.

---

## 2. OS API map

| Capability                     | macOS                                                                                                                                            | Windows                                                                                             |
| ------------------------------ | ------------------------------------------------------------------------------------------------------------------------------------------------ | --------------------------------------------------------------------------------------------------- |
| Takeover overlay               | `NSPanel` borderless, `level=.screenSaver`, `collectionBehavior=[.canJoinAllSpaces,.fullScreenAuxiliary,.stationary]`, one window per `NSScreen` | Borderless `WS_EX_TOPMOST` window per monitor (`EnumDisplayMonitors`, `SetWindowPos(HWND_TOPMOST)`) |
| System calendars (no OAuth)    | **EventKit** `EKEventStore.requestFullAccessToEvents()` (macOS 14+), `NSCalendarsFullAccessUsageDescription`, reads all Internet Accounts        | None usable — `AppointmentStore` is dead-end; use OAuth                                             |
| Multi-account Google/Microsoft | OAuth 2.0 + PKCE, **loopback redirect** (Google still supports loopback for Desktop client type), Google Calendar v3 + Microsoft Graph           | Same — fully cross-platform                                                                         |
| Token storage                  | Keychain                                                                                                                                         | DPAPI / Credential Manager                                                                          |
| Menu bar / tray                | `NSStatusItem` / SwiftUI `MenuBarExtra`, `LSUIElement=YES`                                                                                       | `Shell_NotifyIcon`                                                                                  |
| Launch at login                | `SMAppService.mainApp.register()`                                                                                                                | Registry Run key / MSIX StartupTask                                                                 |
| Auto-update (non-store)        | Sparkle                                                                                                                                          | Squirrel / Velopack / WinSparkle                                                                    |
| Distribution                   | Developer ID + notarytool (App Store risky for this app: sandbox vs EventKit vs screen-saver-level windows)                                      | Authenticode (EV cert to avoid SmartScreen) / MS Store                                              |

---

## 3. Framework verdicts

### 3.1 Native Swift + SwiftUI/AppKit — ⭐ best macOS fit, no Windows story

**Language:** Swift 6 only. **What In Your Face itself uses.**

**Benefits**

- The overlay maps **1:1 to public AppKit APIs** — the hardest feature is the _easiest_ here. NSPanel recipe above, hosting SwiftUI content via `NSHostingView`. No plugins, no FFI, no "works in dev, breaks packaged" class of bugs.
- **EventKit is a first-party one-liner** — the "skip OAuth on Mac" superpower no cross-platform stack gets for free.
- Best footprint of all options: ~30–80 MB idle RAM, single-digit-MB bundle, instant launch. Ideal for a 24/7 resident agent.
- `MenuBarExtra` / `NSStatusItem`, `LSUIElement`, `SMAppService` — all first-party one-liners.
- Governance is the safest of any option: Apple-backed, formal steering groups, effectively zero abandonment risk for the macOS path.
- AI-agent story improved dramatically in 2025–26: `xcodebuild`/`swift build` are fully headless; **XcodeBuildMCP** (~59 MCP tools) and Apple's `xcrun mcpbridge` let Claude Code build, test, run, and screenshot without Xcode open. Massive Swift/AppKit training corpus.
- Signing/notarization (`notarytool`) and Sparkle are mature and CLI-scriptable.

**Drawbacks**

- **Windows = a second codebase in a second language** (realistically C#/.NET + WinUI 3). Verified: Swift-on-Windows is _not_ an escape hatch — the swift-winui binding was **archived Oct 2025**, its industrial backer (Browser Company → Atlassian) pivoted away, and the Swift Windows Workgroup (formed Jan 2026) is infrastructure-only. Also verified: WinUI 3 has its own open topmost bug (#9990). Roughly doubles effort for platform #2.
- Hot reload is second-class: SwiftUI Previews need the Xcode GUI and can't render real window-level behavior; **Inject/InjectionNext** (third-party, actively maintained — Inject 1.6.0 Apr 2026) hot-swaps view bodies into the running app but is flaky, requires disabling library validation in dev builds, and only handles function-body changes. Realistic loop: incremental rebuild + relaunch in seconds-to-tens-of-seconds.
- `.xcodeproj` is agent-hostile — **use SwiftPM or XcodeGen/Tuist** so the agent edits plain text.
- Overlay behavior can't be verified headlessly (true everywhere).

**Verdict:** Strongest technical fit by a wide margin for a macOS-first product — the proof is that the original app ships this way. The cost is Windows.

---

### 3.2 Electron — ⭐ best AI dev loop, best cross-platform; heaviest

**Language:** TypeScript everywhere; Obj-C++ N-API addon _only_ for EventKit.

**Benefits**

- Only cross-platform framework with **first-party, documented APIs for the overlay**: `setAlwaysOnTop(true, 'screen-saver')` (full NSWindow level enum) + `setVisibleOnAllWorkspaces(true, {visibleOnFullScreen: true})`; per-display windows via the `screen` module. Windows topmost included. Zero native code for the overlay.
- **Best autonomous-AI loop of any option, by far**: largest training corpus; Vite HMR sub-second for all UI; main-process restart ~1–2 s (no compile step at all); **official Playwright-for-Electron** — an agent can launch the real app, inject a fake calendar event, assert `isAlwaysOnTop()`, click snooze, and screenshot, fully headless.
- Tray, `app.dock.hide()`/LSUIElement, `setLoginItemSettings()`, `safeStorage` (Keychain/DPAPI) — entire feature list except EventKit in pure JS. OAuth via `googleapis` + `@azure/msal-node` is trivial and battle-tested.
- Safest governance of any option: OpenJS Foundation, ~8-week Chromium-tracking cadence, VS Code/Slack/Figma dependency = zero abandonment risk.
- Mature signing/notarization/auto-update via Forge/Builder + electron-updater.

**Drawbacks**

- **Footprint**: verified realistic idle RAM ~130–180 MB (can exceed with per-monitor overlay windows — each is a renderer process); ~150–250 MB on disk. The single biggest strike for an always-resident utility. Mitigations: lazy-create/destroy overlay windows, keep one hidden renderer max.
- **Overlay reliability nuance (verified)**: the dock-transform flicker is _documented default behavior_ — you must set `skipTransformProcessType` and be an accessory app from launch (we are). Issue #36364 ("doesn't appear over fullscreen until manually focused") is closed-wontfix — mitigations exist (set flags after `ready-to-show`, accessory policy) but the no-user-interaction wake-over-fullscreen case **must be tested on real hardware early**.
- **EventKit needs a native addon**: the only community module (`eventkit-node`) is a 9-star hobby project missing recurrence expansion. Realistic plan: have the AI write a small Obj-C++ N-API addon, or ship a tiny Swift CLI helper that dumps EventKit JSON (a proven pattern) — or just lean on OAuth and add EventKit later.
- Web-rendered feel; fine for a fullscreen alert + small settings panel.

**Verdict:** Lowest-risk path to _shipping on both OSes fast with AI agents doing most of the work_. You pay in RAM.

---

### 3.3 Tauri v2 — ⭐ best footprint; the hard 20% is unsafe Rust

**Language:** Web frontend (any stack) + Rust core; `objc2-app-kit` / `objc2-event-kit` / `windows` crates for native glue.

**Benefits**

- Smallest cross-platform footprint: few-MB installer; Rust core idles at ~5 MB — though **verified correction**: total idle is webview-dominated (~30–120 MB; per-monitor overlay webviews push toward low hundreds of MB on macOS WKWebView). Still lighter than Electron, but not the "tens of MB" marketing number.
- Official plugin suite covers the agent shape: tray, `ActivationPolicy::Accessory`, autostart, single-instance, updater, stronghold/keyring, **tauri-plugin-oauth** (loopback PKCE).
- **Verified production path for the overlay exists**: the **`tauri-nspanel` plugin** (ahkohd) converts the window to a native NSPanel and floats above fullscreen reliably in packaged builds — shipping in Cap, Screenpipe, EcoPaste; a June 2026 case study confirms it across Sonoma/Sequoia/Tahoe. Combined with Accessory activation policy this works.
- Foundation-governed (Commons Conservancy, elected board) with full-time maintainers funded via CrabNebula; brisk release cadence.
- Frontend half gets full Vite HMR + huge web training corpus.

**Drawbacks**

- Tauri **core** cannot do the overlay: `always_on_top` is a bare boolean, no window-level enum, no `visibleOnFullScreen`; issues #5566 (open, "works in dev, breaks in release") and #11488 (closed not-planned). You depend on a third-party native plugin or ~100–150 lines of your own `unsafe` objc2 code.
- EventKit = hand-written Rust (see Decision Record below — `eventkit-rs` reduces this substantially).
- **No Rust hot reload** — every native-side change is a 5–60 s cargo rebuild (watcher over-trigger/lock-contention documented, #11732). The slow loop sits exactly on the hardest, least-AI-automatable code: sparse training data, failures are silent-at-runtime rather than compile errors.
- Two webview engines (WKWebView vs WebView2) = cross-platform visual QA.

**Verdict:** The right choice if always-resident footprint is your top priority and you (or budgeted human time) can own a few hundred lines of native Rust. Otherwise the native core will bottleneck the AI workflow.

---

### 3.4 React Native (react-native-macos + react-native-windows) — ❌ not recommended

**Benefits:** React/TS reuse (+ future iOS companion), Metro Fast Refresh on the UI layer, Microsoft-backed with Office/Teams shipping on RN-Windows.

**Drawbacks (decisive)**

- Every load-bearing feature — overlay window, tray, EventKit, Keychain, OAuth redirect, login item, auto-update — is a **hand-written native module** in Obj-C/Swift _and_ C++/WinRT. The JS-reuse promise barely touches what makes this app hard.
- **Verified: react-native-macos is the least-maintained piece** — stuck on the 0.81 line (0.81.7, Apr 2026) while RN core is at 0.85 and went Fabric-only at 0.82; RN-Windows already shipped Fabric-only. Your _primary_ platform rides Microsoft's least-invested fork, ~4 minors behind.
- Thin/stale desktop docs and little training data for New-Architecture native modules — agents hallucinate exactly where the app is hardest.
- Heaviest cross-platform idle footprint after Electron (Hermes + RN runtime + per-window React trees).

**Verdict:** Defensible only if an iOS/Android companion sharing code is a near-term certainty. Otherwise you write the same native code as Swift, plus a bridge, on a lagging fork.

---

### 3.5 NeutralinoJS — ❌ eliminated

**Benefits:** Tiny (<5 MB, low RAM), fast web loop, basic tray, healthy ~2-month release cadence, GSoC 2026 org.

**Drawbacks (fatal, verified against full API surface + changelog through v6.8.0)**

- `setAlwaysOnTop(boolean)` is all you get — **no window levels, no collectionBehavior, no native handle access**. Verified: the official extension mechanism is WebSocket-isolated separate processes that _cannot_ touch the window handle (maintainer-confirmed), and the "custom native API" path is literally _fork the C++ core and recompile_. The defining feature is unreachable.
- No EventKit path (verified: no community extension exists at any maturity), no secure storage (plaintext JSON), no display enumeration for multi-monitor, no signing/notarization/update pipeline.
- Bus-factor ~1 governance (solo-maintainer-led non-profit).

**Verdict:** Poor fit. Great footprint wasted because the product's core feature requires forking the framework.

---

### 3.6 Flutter Desktop — viable middle path, missed in your list

**Benefits**

- **Best-in-class hot reload that includes desktop**: stateful sub-second reload for ~85% of the code (Dart UI + logic). Strong typing + `flutter analyze` + headless `flutter test` give agents fast, reliable feedback.
- The exact app shape is covered by the cohesive **leanflutter suite**: `window_manager` (incl. `setVisibleOnAllWorkspaces(visibleOnFullScreen: true)` — verified it sets canJoinAllSpaces + fullScreenAuxiliary), `tray_manager`, `launch_at_startup`, `screen_retriever`, `auto_updater` (Sparkle/WinSparkle).
- `macos_window_utils` exposes `setLevel(screenSaver)`; **verified**: it composes with window_manager without conflict (orthogonal NSWindow properties).
- `googleapis` gives a typed Calendar v3 client; multi-account is just multiple token sets; `flutter_secure_storage` wraps Keychain/DPAPI.
- Desktop investment de-risked by **Canonical** funding multi-window work.

**Drawbacks**

- Multi-monitor takeover is the weak spot: official multi-window APIs still experimental (3.35–3.41); `desktop_multi_window` spawns a **separate Flutter engine per window** with per-engine plugin re-registration and possibly ~100 MB/engine worst case (verified: per-engine ImageCache alone defaults to 100 MB).
- EventKit = your own ~60–100-line Swift platform channel (no mature package). Native changes break hot reload.
- Dart + the niche desktop packages are underrepresented in training data — agents need pub.dev docs fed to them.
- Governance: Google single-vendor + 2024 layoffs + Flock fork overhang (upstream still shipping fine); the leanflutter/macosui packages are tiny single-maintainer projects — the acuter abandonment risk.
- Idle RAM ~40–150 MB; between Tauri and Electron.

**Verdict:** Credible if you like Dart and want one codebase with genuine hot reload. The multi-monitor + EventKit corners are yours to own.

---

### 3.7 Others sweep — all eliminated (with reasons)

| Framework                        | Why eliminated                                                                                                               |
| -------------------------------- | ---------------------------------------------------------------------------------------------------------------------------- |
| **.NET MAUI**                    | **Impossible**: macOS runs via Mac Catalyst (UIKit); window levels/always-on-top officially unsupported (dotnet/maui #11778) |
| **Wails v3 (Go)**                | Still stabilizing; open AlwaysOnTop macOS regressions (#3834); same raw-NSWindow burden as Tauri with a smaller ecosystem    |
| **Kotlin Compose Multiplatform** | Undecorated windows refuse true fullscreen on macOS (JetBrains #4573); desktop is JetBrains' least-invested target           |
| **Avalonia**                     | `Topmost` only; no window-level/canJoinAllSpaces story without hand-rolled NSWindow interop; single-vendor VC-backed         |
| **Qt/QML**                       | Achievable only via native NSWindow hacks; frameless quirks; commercial licensing friction for closed-source                 |
| **Sciter**                       | Tiny community, sparse training data, weak overlay/EventKit path                                                             |

---

## 4. Head-to-head matrix

|                           | Swift native                   | Electron                                | Tauri v2                    | Flutter                     | RN desktop                | Neutralino       |
| ------------------------- | ------------------------------ | --------------------------------------- | --------------------------- | --------------------------- | ------------------------- | ---------------- |
| Overlay (macOS, packaged) | ✅✅ first-party               | ✅ first-party JS API (test wake-case!) | ⚠️ via tauri-nspanel plugin | ⚠️ 2 packages + maybe Swift | ⚠️ DIY native module      | ❌ fork the core |
| Overlay (Windows)         | ➖ separate app                | ✅                                      | ✅                          | ✅                          | ⚠️ DIY                    | ⚠️ partial       |
| EventKit (no-OAuth Mac)   | ✅✅ one-liner                 | ⚠️ small ObjC++ addon                   | ⚠️ Rust via eventkit-rs     | ⚠️ small Swift channel      | ⚠️ DIY bridge             | ❌               |
| OAuth multi-account       | ✅ AppAuth                     | ✅✅ googleapis/msal                    | ✅ official plugin          | ✅ community pkgs           | ⚠️ DIY redirect           | ⚠️ DIY           |
| Tray/agent/login-item     | ✅✅                           | ✅✅                                    | ✅✅                        | ✅✅                        | ⚠️                        | ⚠️               |
| Idle RAM                  | **~30–80 MB**                  | ~130–180+ MB                            | ~30–120 MB                  | ~40–150 MB                  | high                      | **low**          |
| Bundle                    | **~10 MB**                     | 150–250 MB                              | **~5–15 MB**                | 40–100 MB                   | large                     | **<5 MB**        |
| Hot reload                | ⚠️ Inject (flaky)              | ✅✅ HMR + 2s main reload               | ✅ UI / ❌ Rust             | ✅✅ Dart (not native)      | ✅ JS / ❌ native         | ✅ web only      |
| Headless AI verification  | ✅ XcodeBuildMCP + screenshots | ✅✅ Playwright-for-Electron            | ⚠️ build/test only          | ✅ flutter test             | ⚠️                        | ⚠️               |
| Training-data depth       | ✅✅                           | ✅✅✅                                  | ✅ web / ⚠️ objc2           | ⚠️ Dart desktop             | ⚠️ desktop forks          | ❌               |
| Governance                | ✅✅ Apple                     | ✅✅ OpenJS                             | ✅ Commons Conservancy      | ⚠️ Google + layoffs         | ⚠️ MS least-invested fork | ❌ bus-factor 1  |
| Windows from same code    | ❌                             | ✅✅                                    | ✅                          | ✅                          | ✅ (lagging)              | ✅               |

---

## 5. Recommendation (original — superseded by Decision Record below)

**The decision reduces to one question: how much do you care about Windows, and when?**

### Option A — macOS-first, native Swift (what the original did)

Best product on the platform that matters: first-party overlay APIs, EventKit one-liner, 30–80 MB resident, native feel. AI-agent workflow is _good_ (XcodeBuildMCP, SwiftPM/Tuist for agent-editable projects, headless build/test/screenshot) but iteration is rebuild-based, not HMR. Windows later = separate C#/WinUI app (or an Electron/Tauri Windows-only build — nothing stops a hybrid).

### Option B — Cross-platform now, Electron (the AI-velocity pick)

One TS codebase, both OSes, the overlay via documented first-party APIs, and the **best autonomous loop in the industry** — your agent can write a Playwright test that launches the app, fires a fake meeting, and screenshots the takeover. Costs: ~150 MB RAM resident, and EventKit needs one small ObjC++ addon (or skip it — OAuth covers everything, exactly like In Your Face's own Google/Microsoft direct connections).

### Option C — Cross-platform, footprint-obsessed: Tauri

Take it if the always-resident RAM story matters more than dev velocity. Use `tauri-nspanel` (production-proven) + `ActivationPolicy::Accessory` for the overlay; budget human attention for the unsafe-Rust corners the AI will fumble.

---

## DECISION RECORD (June 5, 2026) — Tauri v2, local-only

After review, the chosen stack is **Tauri v2** with a **fully local, EventKit-only** architecture. This supersedes the Electron MVP recommendation above. Rationale:

1. **Binary size is a hard requirement** — Electron's floor is ~80–100 MB installer (bundled Chromium); Tauri ships ~5–15 MB using the OS WKWebView. Electron cannot be made small; this is architectural.
2. **No OAuth, by design** — the app reads calendars exclusively via **EventKit** (macOS Internet Accounts: iCloud/Google/Exchange). The OS owns sync, auth, and offline caching. This eliminates: Google Cloud project + consent-screen verification (Calendar is a "sensitive scope"), the 7-day refresh-token expiry for unverified apps, token storage, and all server-side concerns. It is also exactly how In Your Face v1 worked.
3. **The local-only decision erases Electron's main advantage** (pure-JS OAuth). For EventKit, Electron needs a native ObjC++ addon anyway — so the matchup tilts to Tauri.
4. **The Rust burden dropped**: [`eventkit-rs`](https://github.com/weekendsuperhero-io/eventkit-rs) (v0.5.6, May 29 2026, Apache 2.0) is a _safe_ wrapper over objc2-event-kit that handles `requestFullAccessToEvents` (macOS 14+) and date-range queries across all calendars. Mitigation for its 5-star/single-maintainer status: vendor or pin+fork. Estimated hand-written Rust for the MVP: **~200–250 lines**, of which only the `tauri-nspanel` overlay glue is high-risk (week-1 spike).
5. Windows support is deferred indefinitely (EventKit is macOS-only; the Windows story would reintroduce OAuth).

Architecture: React/TS frontend (tray popover, overlay alert, settings) + Rust core (eventkit-rs commands, alarm scheduler with sleep/wake re-arm, tauri-nspanel overlay, tray) + `tauri-plugin-store` for settings. EventKit is the database; no SQLite needed for MVP.

See `PLAN.md` for the phased build plan with verification checkpoints.

---

## Appendix: corrections that came out of adversarial verification

1. **In Your Face is native Swift, not Electron** — confirmed by binary inspection of v3.25.5 (a research agent had claimed the opposite; the verifier refuted it with `otool`).
2. **No framework gets "reliably above another app's fullscreen Space" for free** — `fullScreenAuxiliary` semantics are same-app; the accessory-app activation policy is the real enabler; iTerm2 lost this to a macOS 11.1 regression; Electron #36364/#10078 are wontfix; plan real-device QA per macOS release.
3. **Tauri core can't do the overlay, but `tauri-nspanel` ships it in production** (Cap, Screenpipe) — the gap is integration burden, not capability.
4. **Electron RAM claims verified conservative**: expect 130–180+ MB, more per overlay window.
5. **Tauri "tens of MB" idle is marketing**: webview-dominated, realistically 30–120 MB, low hundreds with per-monitor overlays.
6. **`eventkit-node` is not a dependable shortcut** (9 stars, no recurrence expansion, compile-from-source, Swift toolchain required).
7. **react-native-macos is stuck on the 0.81 line** while core is at 0.85 (Fabric-only since 0.82) — the primary-platform fork lags worst.
8. **Swift-on-Windows is dead as an escape hatch**: swift-winui archived Oct 2025; Windows Workgroup (Jan 2026) is infra-only.
9. **Neutralino verified unreachable**: full 30-method window API audit + changelog through v6.8.0 — no levels, no handles, extensions provably can't touch the window.
10. **Flutter's `window_manager` + `macos_window_utils` don't conflict** (orthogonal NSWindow properties — source-inspected), but no working combined over-fullscreen sample exists publicly.
