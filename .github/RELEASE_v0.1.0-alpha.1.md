# Release notes — v0.1.0-alpha.1

Copy into GitHub **Releases → Draft new release → Tag `v0.1.0-alpha.1`**.

---

**ozr** is a security-first, local-first async agent harness for MCP — CLI, HTTP API, OpenAI-compatible shim, and Tauri GUI (alpha).

## Highlights

- **Docker full stack** — `docker-compose.stack.yml` (ozr API + sandboxd + Qdrant)
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

https://github.com/kenjiroe/ozr/blob/master/CHANGELOG.md

**Alpha:** API and behavior may change between tags. Pin a release for experiments.
