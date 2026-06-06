# overlay/ — internal decisions

- **Fixed rgba/hex colors ONLY** (enforced by themes.test.ts). CSS system colors
  resolve per-window active/inactive appearance → each display rendered a
  different shade. This shipped broken once; the test is the fence.
- One panel per display: `role=main` (primary, the card + key focus) vs
  `role=dim` (frost tint only, `pointer-events: none`, window class can't take
  key). Card content must never render twice.
- The fire emit happens BEFORE this webview boots — always pull
  `get_active_alarms` on mount AND listen for `alarm-fired`.
- Dismissal: explicit buttons + Esc only. The Dismiss button must exist even
  with zero alarm cards (spike path regression).
- Frost blur is native (Tauri window effects, state=Active). Don't add
  `backdrop-filter` — it cannot blur behind the window.
