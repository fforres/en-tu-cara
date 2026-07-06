#!/usr/bin/env bash
# CP3 automated tier: full alarm lifecycle in ~45 seconds, no real calendar.
#   Injects an event starting +15 s via ENTUCARA_TEST_EVENTS, then asserts:
#     1. the default 5-min reminder fires on the first tick (already in-window)
#     2. Overlay panels appear on every display (layer 1000)
#     3. T-0 fires at start (±5 s)
#     4. A declined event injected alongside NEVER fires
set -euo pipefail
cd "$(dirname "$0")/../.."

APP="src-tauri/target/release/bundle/macos/En Tu Cara.app"
DATA="$HOME/Library/Application Support/dev.fforres.entucara"
WINLIST="scripts/bin/winlist"
pass() { printf '  \033[32m✓\033[0m %s\n' "$1"; }
fail() { printf '  \033[31m✗ %s\033[0m\n' "$1"; pkill -f "En Tu Cara" 2>/dev/null || true; exit 1; }

[[ -d "$APP" ]] || fail "packaged app missing — run: pnpm tauri build"
[[ -x "$WINLIST" ]] || bash scripts/checkpoints/cp1b-auto.sh >/dev/null 2>&1 || true
rm -f "$DATA/fire-log.jsonl" "$DATA/state.json" "$DATA/overlay-state.json" "$DATA/overlay-log.jsonl"
pkill -f "En Tu Cara" 2>/dev/null || true; sleep 1

echo "== CP3-auto: alarm lifecycle (event starts T+15s) =="
ENTUCARA_TEST_MODE=1 ENTUCARA_SILENT=1 \
ENTUCARA_TEST_EVENTS='[{"key":"(e2e @ now)","title":"E2E Test Meeting","start_in":15,"duration":60,"my_rsvp":"accepted"},{"key":"(declined @ now)","title":"Declined","start_in":15,"duration":60,"my_rsvp":"declined"}]' \
"$APP/Contents/MacOS/en-tu-cara" >/tmp/cp3.log 2>&1 &

LOGF="$DATA/overlay-log.jsonl"
sleep 10  # first tick (after 2s startup grace) fires T-5 and spawns the overlay
SCREENS=$("$WINLIST" | sed -E 's/screens=([0-9]+).*/\1/')
PANELS=$(python3 -c "
import json
mx = 0
for line in open('$LOGF'):
    mx = max(mx, len(json.loads(line)['overlays']))
print(mx)" 2>/dev/null || echo 0)
[[ "$PANELS" == "$SCREENS" && "$PANELS" -ge 1 ]] \
  && pass "overlay up on T-5 ($PANELS/$SCREENS displays, via overlay history)" \
  || fail "overlay missing at T-5: $PANELS/$SCREENS"

sleep 17  # past T+15 start → T-0 must have fired
pkill -f "En Tu Cara" 2>/dev/null || true

LOG="$DATA/fire-log.jsonl"
[[ -f "$LOG" ]] || fail "fire-log.jsonl missing"
python3 - "$LOG" <<'PY' || exit 1
import json, sys
from datetime import datetime
recs = [json.loads(l) for l in open(sys.argv[1])]
def p(m): print(f'  \033[32m✓\033[0m {m}')
def f(m): print(f'  \033[31m✗ {m}\033[0m'); sys.exit(1)

e2e = [r for r in recs if r["key"] == "(e2e @ now)"]
kinds = [r["kind"] for r in e2e]
if kinds[:2] != ["reminder_5", "t_zero"]: f(f"expected [reminder_5, t_zero], got {kinds}")
p(f"fire sequence correct: {kinds[:2]}")

t0 = next(r for r in e2e if r["kind"] == "t_zero")
lat = (datetime.fromisoformat(t0["fired_at_wall"]) - datetime.fromisoformat(t0["scheduled_for"])).total_seconds()
if not (0 <= lat <= 5): f(f"T-0 latency {lat:.1f}s out of [0,5]")
p(f"T-0 latency {lat:.1f}s (≤5s target)")

if any(r["key"] == "(declined @ now)" for r in recs): f("DECLINED EVENT FIRED — policy violation")
p("declined event never fired")
PY

echo "CP3-auto PASSED"
