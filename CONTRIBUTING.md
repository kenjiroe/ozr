# Contributing to ozr

Thank you for helping improve ozr. This project is security-first: changes that weaken approval gates, bypass policy, or expose secrets will not be accepted.

## Prerequisites

- Rust **1.85+** (see `ui/rust-toolchain.toml` for the GUI shell)
- Node.js **22+** (GUI and MCP filesystem test fixture only)

## Setup

```bash
git clone <your-fork-url>
cd ozr
cargo build
```

Optional local config:

```bash
mkdir -p .ozr
cp .env.example .ozr/config.env
# edit .ozr/config.env — keep secrets out of git
```

## Before opening a PR

Run these from the repository root:

```bash
cargo fmt --all
cargo clippy --all-targets
cargo test
```

For MCP integration tests:

```bash
npm ci --prefix tests/fixtures/mcp-filesystem
cargo test --test mcp_stdio --test mcp_filesystem
```

Live integration fixtures (optional, requires running services):

```bash
OZR_RUN_INTEGRATION=1 cargo test --test integration_live -- --ignored
```

## Pull request guidelines

- Keep changes focused; one logical change per PR.
- Add or update tests when behavior changes.
- Do not commit `.ozr/`, `.env`, API keys, or personal paths.
- Describe security impact when touching policy, approval, or sandbox execution.

## Architecture references

- [ozr.md](ozr.md) — product blueprint (Thai/English)
- [INTEGRATION_SPEC.md](INTEGRATION_SPEC.md) — integration phases and contracts
- [docs/sandboxd-production.md](docs/sandboxd-production.md) — sandboxd deployment notes

## Code of conduct

Be respectful and constructive. Security reports with reproduction steps are especially welcome.
