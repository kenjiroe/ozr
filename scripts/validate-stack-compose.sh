#!/usr/bin/env bash
# Validate docker-compose.stack.yml merges without starting services.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"
# shellcheck source=lib/stack-env.sh
source "$ROOT/scripts/lib/stack-env.sh"

ENV_FILE=".env.stack.example"
set -a
# shellcheck disable=SC1090
source "$ROOT/$ENV_FILE"
set +a

VENDOR="${SANDBOXD_DIR:-vendor/sandboxd}"
if [[ "$VENDOR" != /* ]]; then
  VENDOR="$ROOT/$VENDOR"
fi
VENDOR="$(cd "$(dirname "$VENDOR")" && pwd)/$(basename "$VENDOR")"
export SANDBOXD_DIR="$VENDOR"
export SANDBOXD_DATA_HOST_ABS="$(stack_data_host_abs "$ROOT")"
export SANDBOXD_LOG_DIR="$SANDBOXD_DATA_HOST_ABS/log"

if [[ ! -f "$VENDOR/docker-compose.yml" ]]; then
  echo "Cloning sandboxd into $VENDOR ..."
  mkdir -p "$(dirname "$VENDOR")"
  for attempt in 1 2 3; do
    if git clone --depth 1 https://github.com/tastyeffectco/sandboxd.git "$VENDOR"; then
      break
    fi
    if [[ "$attempt" -eq 3 ]]; then
      echo "failed to clone sandboxd after 3 attempts" >&2
      exit 1
    fi
    sleep 3
  done
fi

if [[ ! -f "$VENDOR/.env" ]]; then
  cp "$VENDOR/.env.example" "$VENDOR/.env"
  echo "created $VENDOR/.env from .env.example"
fi

# Compose loads --env-file after the shell environment; pin absolute paths so
# include: ${SANDBOXD_DIR}/docker-compose.yml resolves on Linux CI runners.
COMPOSE_ENV="$(mktemp)"
trap 'rm -f "$COMPOSE_ENV"' EXIT
cp "$ROOT/$ENV_FILE" "$COMPOSE_ENV"
{
  echo "SANDBOXD_DIR=$VENDOR"
  echo "SANDBOXD_DATA_HOST_ABS=$SANDBOXD_DATA_HOST_ABS"
  echo "SANDBOXD_DATA_DIR=$SANDBOXD_DATA_HOST_ABS"
  echo "SANDBOXD_LOG_DIR=$SANDBOXD_LOG_DIR"
} >> "$COMPOSE_ENV"

docker compose version
if ! docker compose --env-file "$COMPOSE_ENV" -f docker-compose.stack.yml config >/tmp/ozr-compose-config.out 2>/tmp/ozr-compose-config.err; then
  echo "docker compose config failed:" >&2
  cat /tmp/ozr-compose-config.err >&2
  exit 1
fi
echo "stack compose config OK"
