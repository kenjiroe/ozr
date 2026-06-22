# Security Policy

ozr is a **security-first** agent harness. We treat policy bypasses, secret leaks, and unsafe execution paths as high-severity issues.

## Supported versions

| Version | Supported |
|---------|-----------|
| `v0.1.x` (alpha) | Best-effort fixes for security reports with reproduction steps |

Alpha releases may change behavior between tags. Pin a git tag or commit for production experiments.

## Reporting a vulnerability

**Do not open public GitHub issues for undisclosed security bugs.**

1. Email **sarjonator@gmail.com** with subject `[ozr security]`.
2. Include: affected version/commit, steps to reproduce, impact, and suggested fix if any.
3. We aim to acknowledge within **72 hours** and share a remediation timeline when confirmed.

We appreciate coordinated disclosure. Credit will be given in release notes when you agree.

## Secrets and configuration

- Never commit API keys, bearer tokens, or `.env` files with real values.
- Use `.env.example` / `.env.stack.example` as templates only.
- Store runtime secrets in `.ozr/config.env` (gitignored) or your secret manager.
- Run `./scripts/audit-secrets.sh` before tagging a public release.

`ozr config` prints whether keys are set — never the secret values.

## Security model (summary)

- **Guardrail** is the single Plan Mode entry point (`Guardrail::check_plan`).
- Unknown MCP tools default to **Shell** (high scrutiny) — never auto-approved.
- **Policy packs:** `production` requires sandboxd for Shell/Write/Network actions.
- Risky execution can route to **sandboxd** when `OZR_FEATURE_SANDBOXD_EXECUTOR=true` and wired.
- HTTP API binds to `127.0.0.1` by default — expose only behind a trusted reverse proxy.

See [docs/architecture.md](docs/architecture.md) and [docs/sandboxd-production.md](docs/sandboxd-production.md).

## Safe defaults for self-hosting

```env
OZR_POLICY_PACK=production
OZR_FEATURE_SANDBOXD_EXECUTOR=true
OZR_APPROVAL_MODE=prompt
OZR_API_BIND=127.0.0.1:8080
```

Review `ozr sandboxd-checklist` output before production use.
