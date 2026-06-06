#!/usr/bin/env bash
# CP1a automated tier (PLAN §2 Phase 1a): EventKit-in-bundle proof.
#   - Launches the PACKAGED app with ENTUCARA_SPIKE_DUMP=1
#   - First run triggers the TCC prompt (❗HUMAN grants it once)
#   - Validates dump schema: calendars w/ account+color, events w/ occurrence_key,
#     RSVP/status fields present
#   - Occurrence-identity proof: ≥1 recurring series expands to >1 rows w/ distinct starts
#   - Relaunches and asserts permission persisted (no re-prompt path: status FullAccess at launch)
set -euo pipefail
cd "$(dirname "$0")/../.."

APP="src-tauri/target/release/bundle/macos/En Tu Cara.app"
DUMP="$HOME/Library/Application Support/dev.fforres.entucara/spike-dump.json"
pass() { printf '  \033[32m✓\033[0m %s\n' "$1"; }
fail() { printf '  \033[31m✗ %s\033[0m\n' "$1"; exit 1; }

[[ -d "$APP" ]] || fail "packaged app missing — run: pnpm tauri build"

run_dump() {
  rm -f "$DUMP"
  pkill -f "En Tu Cara" 2>/dev/null || true; sleep 1
  # Direct exec of the bundle's main executable: env var passes through and TCC
  # still attributes to the bundle (it IS the bundle's declared executable).
  ENTUCARA_SPIKE_DUMP=1 "$APP/Contents/MacOS/en-tu-cara" >/dev/null 2>&1 &
  local app_pid=$!
  for _ in $(seq 1 60); do [[ -f "$DUMP" ]] && break; sleep 2; done
  kill "$app_pid" 2>/dev/null || pkill -f "En Tu Cara" 2>/dev/null || true
  [[ -f "$DUMP" ]] || fail "dump not written after 120s (TCC prompt pending? grant and re-run)"
}

echo "== CP1a: first launch (may prompt for calendar access) =="
run_dump
python3 - "$DUMP" <<'PY' || exit 1
import json, sys
d = json.load(open(sys.argv[1]))
def p(m): print(f'  \033[32m✓\033[0m {m}')
def f(m): print(f'  \033[31m✗ {m}\033[0m'); sys.exit(1)

if not d.get("granted"): f(f"access not granted (status={d.get('auth_status_at_launch')})")
p(f"access granted (launch status: {d['auth_status_at_launch']})")

cals = d.get("calendars", [])
if not cals: f("no calendars")
accounts = {c.get("account") for c in cals if c.get("account")}
colored = sum(1 for c in cals if c.get("color"))
p(f"{len(cals)} calendars across {len(accounts)} accounts ({colored} with color)")
if len(accounts) < 2: print(f'  \033[33m⚠ only {len(accounts)} account(s) — expected multiple (jsconf.cl + skyward.ai)\033[0m')

evs = d.get("events", [])
if not evs: f("no events in ±7d window")
need = {"occurrence_key","id","title","start","end","status","availability","attendee_count"}
missing = need - set(evs[0].keys())
if missing: f(f"EventDto missing fields: {missing}")
p(f"{len(evs)} events, schema complete")

keys = [e["occurrence_key"] for e in evs]
if len(keys) != len(set(keys)): f("occurrence_key NOT unique across events!")
p("occurrence_key unique across all events")

proof = d.get("expanded_series_proof", [])
ok = [s for s in proof if s["occurrences"] > 1 and s["occurrences"] == s["distinct_starts"]]
if not ok: f("NO recurring series expanded with distinct starts — eventkit-rs occurrence expansion UNPROVEN (go/no-go!)")
p(f"occurrence expansion PROVEN: {len(ok)} series expanded (e.g. {ok[0]['occurrences']} occurrences, all distinct)")

rsvp = sum(1 for e in evs if e.get("my_rsvp"))
p(f"RSVP populated on {rsvp}/{len(evs)} events" if rsvp else "RSVP: 0 events (verify manually — may be OK for self-created events)")
PY

echo "== CP1a: relaunch — permission persistence =="
run_dump
STATUS=$(python3 -c "import json,sys; print(json.load(open('$DUMP'))['auth_status_at_launch'])")
[[ "$STATUS" == "FullAccess" ]] && pass "permission persisted across relaunch (status=$STATUS)" \
  || fail "permission did NOT persist (status=$STATUS) — ad-hoc signing TCC issue, see PROGRESS decisions"

echo "CP1a-auto PASSED"
echo "REMAINING HUMAN STEPS: (1) iPhone test event freshness probe (24-48h), see PLAN CP1a."
