# Changelog

All notable changes to ozr are documented here. The format follows [Keep a Changelog](https://keepachangelog.com/en/1.1.0/).

## [v0.1.0-alpha.2] - 2026-06-22

### Added

- Open-source publication docs: `SECURITY.md`, `CODE_OF_CONDUCT.md`, `docs/architecture.md`, `scripts/audit-secrets.sh`, `scripts/validate-stack-compose.sh`.
- CI secret audit step and GitHub README CI badge.
- Launch materials in `.github/LAUNCH.md`.

### Fixed

- CI: portable secret audit (no `rg` dependency), rustfmt, flaky lib tests, core `docker-compose.yml` validation.
- `main` branch synced with release-ready code on GitHub.

## [v0.1.0-alpha.1] - 2026-06-20

### Added

- Docker full stack (`docker-compose.stack.yml`): ozr API + sandboxd + Qdrant + Traefik helpers.
- Production policy pack (`OZR_POLICY_PACK=production`) — requires sandboxd for Shell/Write/Network.
- GUI external API mode (`OZR_GUI_API_BASE`) to attach Tauri UI to a Docker-hosted API.
- OpenAI SSE streaming shim for `POST /v1/chat/completions` with `stream=true`.
- CI: `docker build`, compose config validation, `clippy -- -D warnings`.
- Open-source docs: `SECURITY.md`, `CODE_OF_CONDUCT.md`, `docs/architecture.md`, `scripts/audit-secrets.sh`.

### Fixed

- Approval gate lost-wakeup race (`Notify` double-check pattern).
- sandboxd bind-mount path for stack deployments (host absolute path under `.docker/`).

### Changed

- Standard stack ports: ozr **8080**, sandboxd **9090**, Qdrant **6333**, GUI dev **18787**.

## [v0.1.0-alpha] - 2026-06-19

### Added

- MIT license and open-source repository scaffolding (README, CONTRIBUTING, issue/PR templates).
- Async Rust core: AgentLoop, Guardrail, policy packs, budget guards, session recovery, replay.
- Axum HTTP API: `/v1/run`, session polling, approval endpoints.
- OpenAI-compatible `/v1/chat/completions` (non-streaming).
- MCP stdio client with NDJSON framing; filesystem server E2E tests.
- Multi-provider LLM adapter (OpenAI-compatible, Anthropic, Gemini, Ollama).
- sandboxd executor abstraction (host, stub, API task submit + poll).
- Tauri desktop GUI (alpha).
- SQLite memory layer with FTS5; optional Qdrant vector recall.
- GitHub Actions CI: fmt, clippy, unit/smoke/MCP/API/sandboxd tests.

[Unreleased]: https://github.com/kenjiroe/ozr/compare/v0.1.0-alpha.2...HEAD
[v0.1.0-alpha.2]: https://github.com/kenjiroe/ozr/compare/v0.1.0-alpha.1...v0.1.0-alpha.2
[v0.1.0-alpha.1]: https://github.com/kenjiroe/ozr/compare/v0.1.0-alpha...v0.1.0-alpha.1
[v0.1.0-alpha]: https://github.com/kenjiroe/ozr/releases/tag/v0.1.0-alpha
