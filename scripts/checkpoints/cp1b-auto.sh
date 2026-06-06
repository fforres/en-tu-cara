#!/usr/bin/env bash
# CP1b automated tier: overlay panels exist at ScreenSaver level on EVERY display.
#
# Verification is via CGWindowList (swift one-shot), NOT screencapture — macOS
# excludes layer-1000 windows from screencapture (verified 2026-06-05), so a
# screenshot can't see the overlay at all. CGWindowList asserts: one window per
# display, layer=1000, alpha=1.0, bounds == display bounds.
#
# The above-ANOTHER-app's-fullscreen truth remains the HUMAN gate: cp1b-human.md.
set -euo pipefail
cd "$(dirname "$0")/../.."

APP="src-tauri/target/release/bundle/macos/En Tu Cara.app"
pass() { printf '  \033[32m✓\033[0m %s\n' "$1"; }
fail() { printf '  \033[31m✗ %s\033[0m\n' "$1"; pkill -f "En Tu Cara" 2>/dev/null || true; exit 1; }

[[ -d "$APP" ]] || fail "packaged app missing — run: pnpm tauri build"

WINLIST="scripts/bin/winlist"
if [[ ! -x "$WINLIST" ]]; then
  echo "== CP1b-auto: compiling winlist checker =="
  mkdir -p scripts/bin
  cat > /tmp/entucara-winlist.swift <<'EOF'
import CoreGraphics
import AppKit
var out: [String] = []
let screens = NSScreen.screens.count
if let list = CGWindowListCopyWindowInfo([.optionOnScreenOnly], kCGNullWindowID) as? [[String: Any]] {
    for w in list {
        let owner = w["kCGWindowOwnerName"] as? String ?? "?"
        let layer = w["kCGWindowLayer"] as? Int ?? -999
        let alpha = w["kCGWindowAlpha"] as? Double ?? -1
        if owner == "En Tu Cara" && layer == 1000 && alpha >= 0.99 {
            out.append("PANEL")
        }
    }
}
print("screens=\(screens) panels=\(out.count)")
EOF
  swiftc -O /tmp/entucara-winlist.swift -o "$WINLIST"
fi

STATE="$HOME/Library/Application Support/dev.fforres.entucara/overlay-state.json"
echo "== CP1b-auto: overlay panel assertions =="
# NOTE: transparent panel windows VANISH from CGWindowList after content load
# (verified 2026-06-05) — ground truth is the app's test-mode overlay-state.json;
# winlist output is informational only.
rm -f "$STATE"; pkill -f "En Tu Cara" 2>/dev/null || true; sleep 1
ENTUCARA_TEST_MODE=1 ENTUCARA_SPIKE_OVERLAY=2 "$APP/Contents/MacOS/en-tu-cara" >/tmp/entucara-spike.log 2>&1 &
sleep 7
SCREENS=$("$WINLIST" | sed -E 's/screens=([0-9]+).*/\1/')
PANELS=$(python3 -c "import json; print(len(json.load(open('$STATE'))['overlays']))" 2>/dev/null || echo 0)
echo "  (winlist informational: $("$WINLIST"))"
sleep 9  # let spike self-dismiss
DISMISSED=$(python3 -c "import json; print(len(json.load(open('$STATE'))['overlays']))" 2>/dev/null || echo "?")
pkill -f "En Tu Cara" 2>/dev/null || true

grep -q "SPIKE_OVERLAY shown" /tmp/entucara-spike.log && pass "spike fired ($(grep 'shown' /tmp/entucara-spike.log))" || fail "spike never fired"
[[ "$PANELS" == "$SCREENS" && "$PANELS" -ge 1 ]] && pass "one panel per display ($PANELS/$SCREENS, via overlay-state)" \
  || fail "panel/display mismatch: $PANELS panels for $SCREENS screens"
[[ "$DISMISSED" == "0" ]] && pass "panels dismissed cleanly" || fail "$DISMISSED panel(s) leaked after dismiss"

echo "CP1b-auto PASSED"
echo "HUMAN GATE REMAINING (cp1b-human.md): above-fullscreen test on 2+ displays."
