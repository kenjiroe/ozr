# Local sandboxd + ozr

ozr routes **Write / Shell / Network** tool actions through [sandboxd](https://github.com/tastyeffectco/sandboxd) when `OZR_FEATURE_SANDBOXD_EXECUTOR=true` and `OZR_SANDBOXD_SANDBOX_ID` is set.

## 1. Install sandboxd

Requirements: Docker Engine + Compose.

```bash
git clone https://github.com/tastyeffectco/sandboxd.git ~/Github/sandboxd
cd ~/Github/sandboxd
```

Use a user-writable data directory (avoids `sudo` for `/var/lib/sandboxed`):

```bash
# in sandboxd/.env
SANDBOXD_DATA_DIR=$HOME/Github/sandboxd/data
SANDBOXD_LOG_DIR=$HOME/Github/sandboxd/data/log
```

Then install and start:

```bash
./install.sh
curl http://127.0.0.1:9090/healthz   # -> ok
```

If port 80 is taken (Rancher Desktop / Docker Desktop k3s), set `HTTP_PORT=8080` in sandboxd `.env` and restart Traefik.

## 2. Wire ozr

From the ozr repo root (with sandboxd running):

```bash
chmod +x scripts/wire-sandboxd.sh
./scripts/wire-sandboxd.sh
```

This creates a sandbox via `POST /sandbox`, then writes:

- `OZR_FEATURE_SANDBOXD_EXECUTOR=true`
- `OZR_SANDBOXD_API_BASE=http://127.0.0.1:9090`
- `OZR_SANDBOXD_SANDBOX_ID=<ulid>`

into `.ozr/config.env`.

## 3. Verify

```bash
cargo build --bin ozr
./target/debug/ozr sandboxd-checklist
OZR_RUN_INTEGRATION=1 cargo test --test integration_live sandboxd_live_fixture -- --ignored
```

## 4. Run through ozr

High-risk shell still requires human approval (by design):

```bash
./target/debug/ozr run "run mystery shell task"
# or via API / GUI — approve when prompted
```

ozr submits a sandboxd task with agent `opencode` (see `OZR_SANDBOXD_AGENT`). The isolated agent runs inside the sandbox container; results are polled from `GET /v1/sandboxes/{id}/tasks/{taskId}`.

Recommended poll budget for opencode tasks (set automatically by `wire-sandboxd.sh`):

```bash
OZR_SANDBOXD_POLL_ATTEMPTS=120
OZR_SANDBOXD_POLL_INTERVAL_MS=2000
OZR_SANDBOXD_POLL_MAX_INTERVAL_MS=10000
OZR_BUDGET_MAX_RUN_SECONDS=900
```

**Tauri GUI:** after wiring, restart `npm run tauri dev` so the spawned `ozr serve` loads `.ozr/config.env` from the repo root (includes sandboxd settings).

Live API smoke (approve + wait for sandboxd task):

```bash
chmod +x scripts/test-sandboxd-shell-api.sh
./target/debug/ozr serve   # from repo root
./scripts/test-sandboxd-shell-api.sh
```

Optional: capture SSE events to `.ozr/audit/sandboxd-events-*.json`:

```bash
# in .ozr/config.env
OZR_SANDBOXD_CAPTURE_EVENTS=true
OZR_SANDBOXD_EVENTS_MAX_TIME_S=5
```

## API mapping (ozr → sandboxd)

| ozr action kind | sandboxd path |
|-----------------|---------------|
| Read | local MCP (not sandboxd) |
| Write / Shell / Network | `POST /v1/sandboxes/{id}/tasks` → poll status → optional events |

## Stop sandboxd

```bash
cd ~/Github/sandboxd && docker compose down
```
