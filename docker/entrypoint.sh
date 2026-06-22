#!/usr/bin/env bash
set -euo pipefail

cd /app

mkdir -p .ozr/audit .ozr/sessions

if [[ ! -f .ozr/config.env ]]; then
  cp .env.example .ozr/config.env
  echo "initialized .ozr/config.env from .env.example"
fi

export OZR_API_BIND="${OZR_API_BIND:-0.0.0.0:8080}"

sandboxd_base="${OZR_SANDBOXD_API_BASE:-http://host.docker.internal:9090}"
auto_wire="${OZR_AUTO_WIRE_SANDBOXD:-false}"

if [[ "$auto_wire" == "true" ]]; then
  health_url="${sandboxd_base%/}/healthz"
  for _ in $(seq 1 90); do
    if curl -sf "$health_url" >/dev/null 2>&1; then
      break
    fi
    sleep 2
  done
  if curl -sf "$health_url" >/dev/null 2>&1; then
    current_id="$(grep -E '^OZR_SANDBOXD_SANDBOX_ID=' .ozr/config.env 2>/dev/null | cut -d= -f2- || true)"
    if [[ -z "${current_id// /}" ]]; then
      echo "auto-wiring sandboxd at $sandboxd_base ..."
      ./scripts/wire-sandboxd.sh --api-base "$sandboxd_base" || true
    fi
  else
    echo "sandboxd not reachable at $health_url (skipping auto-wire)" >&2
  fi
fi

exec ozr "$@"
