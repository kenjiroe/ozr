**Released:** 2026-06-28

**ozr** is a security-first, local-first async agent harness for MCP — CLI, HTTP API, OpenAI-compatible shim, and Tauri GUI (alpha).

## Highlights

- **Security-first guardrails** — unknown MCP tools escalate to Shell; human approval for medium/high risk
- **Docker full stack** — ozr API + sandboxd + Qdrant (`./scripts/docker-up-stack.sh`)
- **Production policy pack** — `OZR_POLICY_PACK=production` requires sandboxd for Shell/Write/Network
- **OpenAI SSE streaming shim** — `POST /v1/chat/completions` with `"stream": true`
- **GUI external API mode** — `OZR_GUI_API_BASE` for Docker-hosted API
- **Open-source docs** — SECURITY, architecture guide, secret audit in CI

## Quick start

```bash
git clone https://github.com/kenjiroe/ozr.git
cd ozr
cargo build --release
./target/release/ozr run "read docs"
```

Docker stack:

```bash
./scripts/docker-up-stack.sh
curl http://127.0.0.1:8080/health
```

## Standard ports

| Service   | Port  |
|-----------|-------|
| ozr API   | 8080  |
| sandboxd  | 9090  |
| Qdrant    | 6333  |
| GUI dev   | 18787 |

## Full changelog

https://github.com/kenjiroe/ozr/blob/main/CHANGELOG.md

**Alpha:** API and behavior may change between tags. Pin a release for experiments.
