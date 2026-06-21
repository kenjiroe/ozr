#!/usr/bin/env bash
# Wire ozr to a local sandboxd control plane (default http://127.0.0.1:9090).
#
# Prerequisites:
#   - sandboxd installed and running (see docs/sandboxd-local.md)
#   - curl, python3
#
# Usage:
#   ./scripts/wire-sandboxd.sh
#   ./scripts/wire-sandboxd.sh --api-base http://127.0.0.1:9090

set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

API_BASE="http://127.0.0.1:9090"
while [[ $# -gt 0 ]]; do
  case "$1" in
    --api-base)
      API_BASE="${2:?missing value for --api-base}"
      shift 2
      ;;
    -h | --help)
      echo "Usage: $0 [--api-base URL]"
      exit 0
      ;;
    *)
      echo "Unknown argument: $1" >&2
      exit 1
      ;;
  esac
done

HEALTH_URL="${API_BASE%/}/healthz"
if ! curl -sf "$HEALTH_URL" >/dev/null; then
  echo "sandboxd not reachable at $API_BASE ($HEALTH_URL)" >&2
  echo "Start sandboxd first — see docs/sandboxd-local.md" >&2
  exit 1
fi

echo "sandboxd health: ok ($HEALTH_URL)"

CREATE_BODY='{"ports":[3000]}'
SANDBOX_JSON="$(curl -sf -XPOST "${API_BASE%/}/sandbox" \
  -H 'content-type: application/json' \
  -d "$CREATE_BODY")"

SANDBOX_ID="$(python3 - <<'PY' "$SANDBOX_JSON"
import json, sys
print(json.loads(sys.argv[1])["id"])
PY
)"

echo "created sandbox: $SANDBOX_ID"

mkdir -p .ozr
CONFIG=".ozr/config.env"
if [[ ! -f "$CONFIG" ]]; then
  cp .env.example "$CONFIG"
  echo "initialized $CONFIG from .env.example"
fi

upsert_env() {
  local key="$1"
  local value="$2"
  if grep -q "^${key}=" "$CONFIG" 2>/dev/null; then
    python3 - <<'PY' "$CONFIG" "$key" "$value"
import pathlib, sys
path, key, value = sys.argv[1:4]
lines = pathlib.Path(path).read_text().splitlines()
out = []
found = False
for line in lines:
    if line.startswith(f"{key}="):
        out.append(f"{key}={value}")
        found = True
    else:
        out.append(line)
if not found:
    out.append(f"{key}={value}")
pathlib.Path(path).write_text("\n".join(out) + "\n")
PY
  else
    echo "${key}=${value}" >>"$CONFIG"
  fi
}

upsert_env OZR_FEATURE_SANDBOXD_EXECUTOR true
upsert_env OZR_SANDBOXD_API_BASE "$API_BASE"
upsert_env OZR_SANDBOXD_SANDBOX_ID "$SANDBOX_ID"
upsert_env OZR_SANDBOXD_REQUIRE_AUTH false
upsert_env OZR_SANDBOXD_HTTPS_ONLY false
upsert_env OZR_SANDBOXD_AGENT opencode
upsert_env OZR_SANDBOXD_POLL_ATTEMPTS 120
upsert_env OZR_SANDBOXD_POLL_INTERVAL_MS 2000
upsert_env OZR_SANDBOXD_POLL_MAX_INTERVAL_MS 10000
upsert_env OZR_BUDGET_MAX_RUN_SECONDS 900

echo ""
echo "Wrote sandboxd settings to $CONFIG"
echo ""
echo "Verify:"
echo "  ./target/debug/ozr sandboxd-checklist"
echo "  OZR_RUN_INTEGRATION=1 cargo test --test integration_live sandboxd_live_fixture -- --ignored"
echo ""
echo "Try a guarded shell run (approval required):"
echo "  ./target/debug/ozr run \"run mystery shell task\""
