# checkpoints/ — internal decisions

Two tiers: `cpN-auto.sh` (headless, re-runnable — the regression guard) and
`cpN-human.md` (eyes-only gates: overlay-over-fullscreen, audible sound).
Run autos against a FRESH `pnpm tauri build`; they pkill the app.

- Overlay ground truth = `overlay-log.jsonl` (append-only heartbeat). Assert
  "N panels WERE shown", never "still shown at t=X" — the human at the keyboard
  dismisses test overlays in ~3s and that must not fail a build.
- `scripts/bin/winlist` (CGWindowList) is INFORMATIONAL ONLY for overlays
  (transparent panels vanish from the window list while still rendering).
- Always set ENTUCARA_SILENT=1; always rm the state/log files a script asserts
  on before launching; always restore settings.json you replace (trap EXIT).
- Timing: app has a 2s scheduler startup grace — first fire lands ~2.5s after
  launch, panels ~2s later. Budget sleeps accordingly.
- Calendar-access-loss e2e: don't `tccutil reset` a RUNNING app (EventKit caches
  auth in-process; it won't observe it). Drive it deterministically with
  `ENTUCARA_TEST_ACCESS='fetch_failed,recover_after=<s>'` (test mode) and assert
  the log shows `calendar access lost` then `calendar access restored` EXACTLY
  ONCE each (the debounce must not flap). Access lines are at INFO/WARN; the file
  is daily-rolled `en-tu-cara.log.<date>`, appended across runs — scope your grep
  to the current run (after the last `started —` line), not the whole file.
