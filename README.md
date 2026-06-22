# ozr

**Security-first, local-first async agent harness for the Model Context Protocol (MCP).**

ozr orchestrates LLM planning, MCP tool execution, human approval, and audit logging in a pure async Rust runtime. Use it from the CLI, HTTP API, OpenAI-compatible clients, or the Tauri desktop GUI.

## Why ozr?

- **Security-first guardrails** — unknown tools escalate to high-risk `Shell`; medium/high actions block on human approval.
- **Non-blocking runtime** — agent runs spawn on Tokio; API sessions poll independently (approval + concurrent `/v1/run` supported).
- **Provider-agnostic** — mock, OpenAI-compatible, Anthropic, Gemini, and Ollama LLM backends; MCP via mock or stdio.
Optional isolated execution via [sandboxd](docs/sandboxd-local.md) (local setup guide).
- **Audit-ready** — run logs, session checkpoints, replay reports under `.ozr/`.

## Quick start

### Build

```bash
cargo build --release
```

### CLI (mock backend, no API keys)

```bash
./target/release/ozr run "read docs"
./target/release/ozr run "run mystery shell task"   # triggers approval gate
```

Initialize local config on first run:

```bash
./target/release/ozr init    # creates .ozr/config.env from defaults
```

### HTTP API

```bash
./target/release/ozr serve
curl http://127.0.0.1:8080/health

curl -s http://127.0.0.1:8080/v1/run \
  -H 'content-type: application/json' \
  -d '{"prompt":"read docs"}'
```

**OpenAI-compatible shim:**

```bash
curl -s http://127.0.0.1:8080/v1/chat/completions \
  -H 'content-type: application/json' \
  -d '{"model":"ozr","messages":[{"role":"user","content":"read docs"}]}'
```

Endpoints: `POST /v1/run` · `GET /v1/session/{id}` · `POST /v1/session/{id}/approve` · `POST /v1/chat/completions`

### Desktop GUI (alpha)

```bash
cargo build
cd ui && npm install && npm run tauri dev
```

The GUI spawns `ozr serve` on `http://127.0.0.1:18787` by default. See [ui/README.md](ui/README.md).

### Docker

**Standard ports:** ozr API **8080** · sandboxd **9090** · Qdrant **6333** · GUI dev **18787**

**Full stack** (ozr API + sandboxd + Qdrant — recommended for self-hosted):

```bash
chmod +x scripts/docker-up-stack.sh
./scripts/docker-up-stack.sh
```

**Infra only** (host-native `cargo` / Tauri GUI — sandboxd + Qdrant in Docker):

```bash
chmod +x scripts/docker-up-infra.sh
./scripts/docker-up-infra.sh
./scripts/wire-sandboxd.sh
cd ui && npm run tauri dev
```

**API only** (mock backends, no sandboxd):

```bash
docker compose up -d --build
curl http://127.0.0.1:8080/health
```

Config template: `.env.stack.example` · Details: [docs/sandboxd-local.md](docs/sandboxd-local.md)

## Configuration

All settings use `OZR_*` environment variables or `.ozr/config.env`.

```bash
cp .env.example .ozr/config.env
```

Key flags:

| Variable | Default | Purpose |
|----------|---------|---------|
| `OZR_LLM_BACKEND` | `mock` | LLM provider |
| `OZR_MCP_BACKEND` | `mock` | MCP transport |
| `OZR_APPROVAL_MODE` | `prompt` | CLI approval (`auto` / `deny` / `prompt`) |
| `OZR_FEATURE_SANDBOXD_EXECUTOR` | `false` | Route risky actions to sandboxd |
| `OZR_API_BIND` | `127.0.0.1:8080` | HTTP API listen address |

Run `ozr config` to print the effective configuration (secrets shown as set/unset only).

## Repository layout

```text
src/          Rust core — agent loop, policy, API, CLI
ui/           Tauri + React desktop GUI (Phase 4C alpha)
tests/        Integration and E2E tests
docs/         Production guides
ozr.md        Product blueprint
INTEGRATION_SPEC.md
```

## Development

```bash
cargo test
cargo fmt --all -- --check
cargo clippy --all-targets
```

See [CONTRIBUTING.md](CONTRIBUTING.md).

## Status

**v0.1.0-alpha** — core async engine, Axum API, OpenAI shim, approval gate, GUI alpha, sandboxd executor (host + Docker deploy). Streaming shim and production hardening are ongoing.

## License

MIT — see [LICENSE](LICENSE).
