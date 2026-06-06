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
