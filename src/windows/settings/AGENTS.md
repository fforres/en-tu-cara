# settings/ — internal decisions

Registry-driven: every setting is a `SettingDef` in `registry.ts`; the sidebar
TOC, fuzzy search, and section views all generate from it. To add a setting:
registry entry + field in BOTH `Settings` types (here and
src-tauri/src/settings.rs, serde-defaulted) + a control case if new kind +
DEFAULTS in SettingsWindow.test.tsx. Nothing else.

- `set_settings` sends the FULL settings object (single-user app; no patching).
- Live-apply is the contract — no "save" button, no restart-required settings.
- Fuzzy weighting label(3x) > keywords(2x) > description(1x); highlights apply
  to labels only.
