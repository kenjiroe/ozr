#!/usr/bin/env bash
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
cd "$ROOT"

if [[ -f tests/fixtures/integration.env.example ]]; then
  # shellcheck disable=SC1091
  set -a
  source tests/fixtures/integration.env.example
  set +a
fi

if [[ -f .ozr/config.env ]]; then
  # shellcheck disable=SC1091
  set -a
  source .ozr/config.env
  set +a
fi

export OZR_RUN_INTEGRATION=1

echo "Running ozr live integration fixtures..."
cargo test --test integration_live -- --ignored --nocapture
