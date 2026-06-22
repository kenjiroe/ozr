#!/usr/bin/env bash
# One-click ozr stack: sandboxd vendor bootstrap + ozr-api + qdrant.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
# shellcheck source=lib/stack-env.sh
source "$ROOT/scripts/lib/stack-env.sh"

ENV_FILE=".env.stack.example"
COMPOSE=(docker compose --env-file "$ENV_FILE" -f docker-compose.stack.yml)

if [[ -f .env.stack ]]; then
  ENV_FILE=".env.stack"
  COMPOSE=(docker compose --env-file "$ENV_FILE" -f docker-compose.stack.yml)
fi

set -a
# shellcheck disable=SC1090
source "$ROOT/$ENV_FILE"
set +a

export SANDBOXD_DIR="${SANDBOXD_DIR:-$ROOT/vendor/sandboxd}"
export SANDBOXD_DATA_HOST_ABS="$(stack_data_host_abs "$ROOT")"

"$ROOT/scripts/bootstrap-sandboxd-vendor.sh"

echo "Starting ozr stack ($ENV_FILE) ..."
"${COMPOSE[@]}" up -d --build

SANDBOXD_PORT="${SANDBOXD_HTTP_PORT:-9090}"
echo "Waiting for sandboxd on :${SANDBOXD_PORT} ..."
for _ in $(seq 1 90); do
  if curl -sf "http://127.0.0.1:${SANDBOXD_PORT}/healthz" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

echo "Waiting for ozr API on :${OZR_HTTP_PORT:-8080} ..."
for _ in $(seq 1 60); do
  if curl -sf "http://127.0.0.1:${OZR_HTTP_PORT:-8080}/health" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

echo ""
echo "Stack status:"
"${COMPOSE[@]}" ps

echo ""
echo "Endpoints (standard ports):"
echo "  ozr API:  http://127.0.0.1:${OZR_HTTP_PORT:-8080}/health"
echo "  sandboxd: http://127.0.0.1:${SANDBOXD_HTTP_PORT:-9090}/healthz"
echo "  qdrant:   http://127.0.0.1:${QDRANT_HTTP_PORT:-6333}/healthz"
echo ""
echo "Stop stack:"
echo "  docker compose --env-file $ENV_FILE -f docker-compose.stack.yml down"
echo ""
echo "Smoke (approval + sandboxd shell):"
echo "  OZR_API_BASE=http://127.0.0.1:${OZR_HTTP_PORT:-8080} ./scripts/test-sandboxd-shell-api.sh"
