# Local sandboxd + ozr

ozr routes **Write / Shell / Network** tool actions through [sandboxd](https://github.com/tastyeffectco/sandboxd) when `OZR_FEATURE_SANDBOXD_EXECUTOR=true` and `OZR_SANDBOXD_SANDBOX_ID` is set.

**sandboxd runs only via the ozr Docker stack** — there is no separate manual install under `~/Github/sandboxd`.

Workspace data is stored under **`.docker/sandboxd-data/`** (host bind mount) so sandboxd can spawn per-sandbox containers via the Docker socket.

## Standard ports

| Service | Port | Notes |
|---------|------|--------|
| ozr API (container) | **8080** | `docker-up-stack.sh` |
| ozr API (GUI spawn) | **18787** | Tauri dev default |
| sandboxd | **9090** | shared by stack + host-native ozr |
| Qdrant | **6333** | vector store |
| Traefik previews | **8081** | sandbox preview URLs |

## 1. Start Docker infra

**Full stack** (ozr API + sandboxd + Qdrant):

```bash
chmod +x scripts/docker-up-stack.sh
./scripts/docker-up-stack.sh
curl http://127.0.0.1:8080/health
curl http://127.0.0.1:9090/healthz
```

**Infra only** (for host-native CLI / Tauri GUI):

```bash
chmod +x scripts/docker-up-infra.sh
./scripts/docker-up-infra.sh
```

Stop everything:

```bash
docker compose --env-file .env.stack.example -f docker-compose.stack.yml down
```

## 2. Wire host-native ozr

When running `cargo run`, Tauri GUI, or `./target/debug/ozr serve` on the host (not the ozr container):

```bash
./scripts/wire-sandboxd.sh
```

This creates a sandbox via `POST /sandbox` and writes into `.ozr/config.env`:

- `OZR_FEATURE_SANDBOXD_EXECUTOR=true`
- `OZR_SANDBOXD_API_BASE=http://127.0.0.1:9090`
- `OZR_SANDBOXD_SANDBOX_ID=<ulid>`
- `OZR_QDRANT_URL=http://127.0.0.1:6333`

The ozr **container** auto-wires on first start (`OZR_AUTO_WIRE_SANDBOXD=true`).

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

Recommended poll budget (set by `wire-sandboxd.sh` and the stack):

```bash
OZR_SANDBOXD_POLL_ATTEMPTS=120
OZR_SANDBOXD_POLL_INTERVAL_MS=2000
OZR_SANDBOXD_POLL_MAX_INTERVAL_MS=10000
OZR_BUDGET_MAX_RUN_SECONDS=900
```

**Tauri GUI:** after wiring, restart `npm run tauri dev` so the spawned `ozr serve` loads `.ozr/config.env` from the repo root.

Live API smoke (approve + wait for sandboxd task):

```bash
chmod +x scripts/test-sandboxd-shell-api.sh
./target/debug/ozr serve   # from repo root, or use stack on :8080
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
