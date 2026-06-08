# Human check: does the takeover render ABOVE another app's fullscreen?

This is the one thing automation can't prove. `screencapture` and CGWindowList
both report our transparent panels unreliably — a screenshot can look empty while
the overlay is plainly on screen — and **neither tells you z-order**. "On top of
another app's fullscreen Space" is a claim only human eyes can confirm. So when
the overlay code, the panel level, or the collection behavior changes (and after
any macOS update), a person has to look.

## Setup

1. Build the real thing — the panel level/Spaces behavior only holds in a
   packaged build, not `cargo run`:
   ```sh
   pnpm tauri build
   ```
2. Put another app into **true fullscreen** (green button / ⌃⌘F), not just a
   maximized window. Good choices: Zoom, a Chrome tab, Keynote, QuickTime.
3. Do this once on the **built-in display** and once on an **external display**.

## Trigger the overlay

Fire a takeover without waiting for a real meeting — `ENTUCARA_SPIKE_OVERLAY=<s>`
shows it after `<s>` seconds, then self-dismisses (env vars: `src-tauri/CLAUDE.md`):

```sh
ENTUCARA_SILENT=1 ENTUCARA_SPIKE_OVERLAY=8 \
  "src-tauri/target/release/bundle/macos/En Tu Cara.app/Contents/MacOS/en-tu-cara"
```

After launching, click into the fullscreen app so it is **focused**, then don't
touch anything — the point is to prove the overlay appears over a focused
fullscreen app on its own, with no interaction. (Drop `ENTUCARA_SILENT=1` if you
also want to confirm the sound is audible.)

## Pass criteria — all must hold, on each display

- [ ] The takeover appears **above** the fullscreen app — same screen, not a flash
      on a different Space/desktop.
- [ ] It appears **without you clicking or focusing anything** (timer-triggered).
- [ ] It covers **every** display.
- [ ] Keyboard focus stays with the fullscreen app until you click the overlay
      (type — keystrokes still go to the app underneath).
- [ ] Dismiss (button / Esc) closes it cleanly; any leftover panels self-close
      when the spike window elapses.

## If it fails (overlay appears _under_ the fullscreen app, or on the wrong Space)

Check, in order:

1. The app is running as an **Accessory** (no Dock icon) — it is, by default.
2. The panel's level + collection behavior are re-applied **after**
   `order_front_regardless` (see `src-tauri/src/overlay.rs`).
3. Confirm the panel level is ScreenSaver-tier and the collection behavior includes
   can-join-all-spaces + fullscreen-auxiliary + stationary.

## Re-run cadence

Note the macOS build you tested on (`sw_vers -productVersion`,
`sw_vers -buildVersion`). Re-run this check after every macOS update — overlay
z-order is exactly the kind of behavior Apple can regress between releases.
