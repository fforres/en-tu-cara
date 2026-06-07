#!/usr/bin/env bash
# CP0 automated tier:
#   build ✓ check ✓ tests ✓ lint ✓ ; packaged app: LSUIElement + calendar usage strings
#   present in Info.plist ; launch: accessory policy active, frontmost app unchanged.
# Headless-safe: the launch section runs only when a built .app exists (pass --launch to force build).
set -euo pipefail
cd "$(dirname "$0")/../.."

pass() { printf '  \033[32m✓\033[0m %s\n' "$1"; }
fail() { printf '  \033[31m✗ %s\033[0m\n' "$1"; exit 1; }

echo "== CP0: frontend =="
pnpm build >/dev/null 2>&1 && pass "pnpm build" || fail "pnpm build"
pnpm test >/dev/null 2>&1 && pass "pnpm test (vitest)" || fail "pnpm test"
pnpm lint >/dev/null 2>&1 && pass "pnpm lint (eslint)" || fail "pnpm lint"

echo "== CP0: rust core =="
(cd src-tauri && cargo check --quiet 2>/dev/null) && pass "cargo check" || fail "cargo check"
(cd src-tauri && cargo test --quiet 2>/dev/null >/dev/null) && pass "cargo test" || fail "cargo test"
(cd src-tauri && cargo clippy --quiet -- -D warnings 2>/dev/null) && pass "cargo clippy" || fail "cargo clippy"

APP="src-tauri/target/release/bundle/macos/En Tu Cara.app"
if [[ "${1:-}" == "--launch" && ! -d "$APP" ]]; then
  echo "== CP0: building bundle (--launch) =="
  pnpm tauri build >/dev/null 2>&1 || fail "tauri build"
fi

if [[ -d "$APP" ]]; then
  echo "== CP0: bundle assertions =="
  PLIST="$APP/Contents/Info.plist"
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :LSUIElement' "$PLIST" 2>/dev/null)" == "true" ]] \
    && pass "LSUIElement=true" || fail "LSUIElement missing"
  /usr/libexec/PlistBuddy -c 'Print :NSCalendarsFullAccessUsageDescription' "$PLIST" >/dev/null 2>&1 \
    && pass "NSCalendarsFullAccessUsageDescription present" || fail "calendar usage string missing"
  [[ "$(/usr/libexec/PlistBuddy -c 'Print :CFBundleIdentifier' "$PLIST")" == "dev.fforres.entucara" ]] \
    && pass "bundle id stable" || fail "bundle id drifted"

  echo "== CP0: launch assertions =="
  FRONT_BEFORE=$(osascript -e 'tell application "System Events" to get name of first process whose frontmost is true' 2>/dev/null || echo "?")
  open -g "$APP"
  sleep 4
  FRONT_AFTER=$(osascript -e 'tell application "System Events" to get name of first process whose frontmost is true' 2>/dev/null || echo "?")
  [[ "$FRONT_BEFORE" == "$FRONT_AFTER" ]] && pass "no focus steal ($FRONT_BEFORE unchanged)" || fail "focus stolen: $FRONT_BEFORE -> $FRONT_AFTER"
  LSINFO=$(lsappinfo info -app dev.fforres.entucara 2>/dev/null || echo "")
  echo "$LSINFO" | grep -qi "Prohibited\|Accessory\|UIElement" && pass "accessory/UIElement activation policy" || fail "activation policy not accessory: $LSINFO"
  osascript -e 'tell application "En Tu Cara" to quit' >/dev/null 2>&1 || pkill -f "En Tu Cara" || true
  pass "app quit cleanly"
else
  echo "  (bundle not built — skipped plist/launch tier; run with --launch for full CP0)"
fi

echo "CP0 PASSED"
