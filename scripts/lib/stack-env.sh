#!/usr/bin/env bash
# Resolve absolute host path for sandboxd data (required for docker-run workspace bind mounts).
stack_data_host_abs() {
  local root="$1"
  local rel="${SANDBOXD_DATA_HOST_DIR:-./.docker/sandboxd-data}"
  if [[ "$rel" != /* ]]; then
    rel="$root/$rel"
  fi
  mkdir -p "$rel/log"
  chmod 0777 "$rel/log" 2>/dev/null || true
  (cd "$rel" && pwd)
}
