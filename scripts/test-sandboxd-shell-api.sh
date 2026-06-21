#!/usr/bin/env bash
# Live API test: approval-gated shell routed to sandboxd (requires running ozr serve + sandboxd).
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

API="${OZR_API_BASE:-http://127.0.0.1:8080}"
MAX_POLLS="${OZR_TEST_MAX_POLLS:-180}"

health="$(curl -sf "${API}/health" || true)"
if [[ "$health" != "ok" ]]; then
  echo "ozr API not ready at $API — start with: ./target/debug/ozr serve" >&2
  exit 1
fi

echo "POST /v1/run (mystery shell) → $API"
run_json="$(curl -sf -XPOST "${API}/v1/run" \
  -H 'content-type: application/json' \
  -d '{"prompt":"run mystery shell task"}')"
session_id="$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['session_id'])" "$run_json")"
echo "session=$session_id"

for _ in $(seq 1 60); do
  view="$(curl -sf "${API}/v1/session/${session_id}")"
  status="$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['status'])" "$view")"
  if [[ "$status" == "pending_approval" ]]; then
    echo "pending_approval — approving..."
    curl -sf -XPOST "${API}/v1/session/${session_id}/approve" \
      -H 'content-type: application/json' \
      -d '{"decision":"approve","reason":"sandboxd live api test"}' >/dev/null
    break
  fi
  sleep 0.2
done

echo "waiting for completed (max ${MAX_POLLS} polls, sandboxd/opencode may take several minutes)..."
for _ in $(seq 1 "$MAX_POLLS"); do
  view="$(curl -sf "${API}/v1/session/${session_id}")"
  status="$(python3 -c "import json,sys; print(json.loads(sys.argv[1])['status'])" "$view")"
  if [[ "$status" == "completed" ]]; then
    echo "$view" | python3 -m json.tool
    result="$(python3 -c "import json,sys; print(json.loads(sys.argv[1]).get('result',''))" "$view")"
    if [[ "$result" != *"sandboxd_task="* ]]; then
      echo "WARN: result missing sandboxd_task marker" >&2
      exit 1
    fi
    echo "OK: live sandboxd shell E2E passed"
    exit 0
  fi
  if [[ "$status" == "failed" ]]; then
    echo "$view" | python3 -m json.tool
    exit 1
  fi
  sleep 2
done

echo "timeout waiting for session $session_id" >&2
exit 1
