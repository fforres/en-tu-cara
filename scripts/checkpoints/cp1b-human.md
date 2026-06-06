# CP1b — Human gate protocol: overlay above fullscreen

**Why a human:** macOS excludes layer-1000 windows from screencapture, and no API
proves "rendered above ANOTHER app's fullscreen Space" — only eyes do (PLAN §3).

## Steps (~3 minutes)

1. Build current: `pnpm tauri build`
2. Make **Zoom** (or Chrome) fullscreen (green button) on your MAIN display.
3. In a terminal:
   ```sh
   ENTUCARA_SPIKE_OVERLAY=8 "src-tauri/target/release/bundle/macos/En Tu Cara.app/Contents/MacOS/en-tu-cara"
   ```
4. Within 8 s, click into the fullscreen app so it is FOCUSED. Don't touch anything.
5. **PASS criteria — all must hold:**
   - [ ] Takeover appears ABOVE the fullscreen app (not on another desktop/Space)
   - [ ] It appears WITHOUT you clicking/focusing anything (timer-triggered)
   - [ ] It covers ALL displays (check every monitor)
   - [ ] Focus did not leave the fullscreen app (type — keystrokes still go there
         until you click the overlay)
   - [ ] Dismiss button works; after 12 s any remaining panels self-close
6. Repeat once with the fullscreen app on a SECONDARY display.

## Record the result

Append to PROGRESS.md → human_gates: `CP1b | <date> | <sw_vers productVersion+buildVersion> | pass/fail + notes`

**If it fails** (overlay under fullscreen / wrong Space): this is the documented
tauri #5566-class behavior. Mitigations to try in order: (1) confirm app is running
as Accessory (it is, by default), (2) re-apply level+behavior after `order_front_regardless`,
(3) escalate per PLAN §2 GO/NO-GO (stack fallback).

## Re-run cadence

After every macOS update (`sw_vers` drift auto-flags this gate), and at every phase gate.
