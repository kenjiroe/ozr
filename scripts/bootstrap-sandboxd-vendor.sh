#!/usr/bin/env bash
# Clone sandboxd into vendor/ (if missing) and build its Docker images for the ozr stack.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
VENDOR="${SANDBOXD_DIR:-$ROOT/vendor/sandboxd}"

if [[ ! -f "$VENDOR/docker-compose.yml" ]]; then
  echo "Cloning sandboxd into $VENDOR ..."
  mkdir -p "$(dirname "$VENDOR")"
  git clone --depth 1 https://github.com/tastyeffectco/sandboxd.git "$VENDOR"
fi

cd "$VENDOR"

if [[ ! -f .env ]]; then
  cp .env.example .env
  echo "created $VENDOR/.env from .env.example"
fi

set -a
# shellcheck disable=SC1091
source "$ROOT/docker/sandboxd.stack.env"
set +a

BASE_IMAGE="${SANDBOXD_IMAGE:-sandboxd-base:1.0.0}"
TAG="${BASE_IMAGE##*:}"

echo "Building sandbox base image ($BASE_IMAGE) ..."
DOCKER="${DOCKER:-docker}" SANDBOXD_IMAGE="$BASE_IMAGE" bash image/build.sh "$TAG"

echo "Building sandboxd control plane ..."
docker compose --env-file "$ROOT/docker/sandboxd.stack.env" build sandboxd

echo "sandboxd vendor ready at $VENDOR"
