# src/ — React/TS webviews

One Vite bundle serves every window. The Rust side opens
`index.html?window=<kind>&…` and `App.tsx` routes on that param:
no param → tray popover · `overlay` (+`role=main|dim`) → alert · `settings`.

## Layout

- `windows/tray/` — popover UI (spec: reference-images/tray-example.png).
- `windows/overlay/` — takeover alert + `themes.ts`. Has its own AGENTS.md — read
  it before touching colors.
- `windows/settings/` — registry-driven settings UI. Own AGENTS.md.
- `lib/` — pure domain logic (link extraction, classification). Own AGENTS.md.

## Interacting

```sh
pnpm test            # vitest, jsdom; per-test cleanup lives in src/test-setup.ts
pnpm exec oxlint --fix src   # the commit hook enforces oxlint (incl. curly braces)
pnpm build           # tsgo --noEmit && vite build — TS errors fail SILENTLY in
                     # `pnpm tauri build` summaries; run this directly to see them
```

- Tauri commands are snake_case in Rust, camelCase from JS:
  `invoke("fetch_events", { daysBack: 1, daysForward: 7 })`.
- Component tests mock IPC with `vi.hoisted` + `vi.mock("@tauri-apps/api/core")` —
  copy the pattern in `windows/settings/SettingsWindow.test.tsx`.
- The `Settings` interface in `windows/settings/registry.ts` MUST mirror the Rust
  `Settings` struct (src-tauri/src/settings.rs) field-for-field; update both and
  the DEFAULTS object in SettingsWindow.test.tsx together.

## Do NOT

- Call `Date.now()`/`new Date()` inside `lib/` — `now` is always injected
  (mock-clock discipline).
- Use CSS system colors (Canvas, Highlight…) in OVERLAY windows — see
  windows/overlay/AGENTS.md. They're fine in the popover/settings (opaque
  normal windows).
- Add a context-menu or click-to-dismiss surface to overlay windows: dismissal
  is buttons + Esc only (explicit user requirement).
