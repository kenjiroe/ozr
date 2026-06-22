#!/usr/bin/env bash
# Start sandboxd + Qdrant infra only (host-native ozr / Tauri GUI dev).
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

echo "Starting infra (traefik + sandboxd + qdrant) ..."
"${COMPOSE[@]}" up -d traefik sandboxd qdrant

SANDBOXD_PORT="${SANDBOXD_HTTP_PORT:-9090}"
echo "Waiting for sandboxd on :${SANDBOXD_PORT} ..."
for _ in $(seq 1 90); do
  if curl -sf "http://127.0.0.1:${SANDBOXD_PORT}/healthz" >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

if ! curl -sf "http://127.0.0.1:${SANDBOXD_PORT}/healthz" >/dev/null 2>&1; then
  echo "sandboxd did not become healthy on http://127.0.0.1:${SANDBOXD_PORT}" >&2
  exit 1
fi

echo ""
echo "Infra ready:"
echo "  sandboxd: http://127.0.0.1:${SANDBOXD_PORT}/healthz"
echo "  qdrant:   http://127.0.0.1:${QDRANT_HTTP_PORT:-6333}/healthz"
echo ""
echo "Wire host-native ozr:"
echo "  ./scripts/wire-sandboxd.sh"
echo ""
echo "Then run CLI/GUI from repo root:"
echo "  cargo run -- serve"
echo "  cd ui && npm run tauri dev"
