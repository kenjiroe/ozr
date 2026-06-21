#!/usr/bin/env bash
# Start sandboxd stack, then ozr on the shared sandboxd_net network.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
SANDBOXD_DIR="${SANDBOXD_DIR:-$ROOT/../sandboxd}"

if [[ ! -f "$SANDBOXD_DIR/docker-compose.yml" ]]; then
  echo "sandboxd repo not found at $SANDBOXD_DIR" >&2
  echo "Clone it first:" >&2
  echo "  git clone https://github.com/tastyeffectco/sandboxd.git $SANDBOXD_DIR" >&2
  exit 1
fi

echo "Starting sandboxd at $SANDBOXD_DIR ..."
(cd "$SANDBOXD_DIR" && docker compose up -d)

echo "Waiting for sandboxd health ..."
for _ in $(seq 1 60); do
  if curl -sf "http://127.0.0.1:9090/healthz" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if ! curl -sf "http://127.0.0.1:9090/healthz" >/dev/null 2>&1; then
  echo "sandboxd did not become healthy on http://127.0.0.1:9090" >&2
  exit 1
fi

echo "Starting ozr (full stack overlay) ..."
cd "$ROOT"
docker compose -f docker-compose.yml -f docker-compose.full.yml up -d --build

echo ""
echo "ozr API:  http://127.0.0.1:${OZR_HTTP_PORT:-8080}/health"
echo "sandboxd: http://127.0.0.1:9090/healthz"
echo ""
echo "Smoke test (from host, against published ozr port):"
echo "  OZR_API_BASE=http://127.0.0.1:${OZR_HTTP_PORT:-8080} ./scripts/test-sandboxd-shell-api.sh"
