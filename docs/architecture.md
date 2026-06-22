# ozr Architecture

ozr is a **security-first, local-first async agent harness** for the [Model Context Protocol (MCP)](https://modelcontextprotocol.io/). It sits between users (CLI, HTTP API, GUI), LLM providers, and MCP tool servers — enforcing policy and human approval before risky actions execute.

For the product blueprint (including Thai notes), see [ozr.md](../ozr.md). For integration contracts, see [INTEGRATION_SPEC.md](../INTEGRATION_SPEC.md).

## High-level flow

```text
User (CLI / HTTP / GUI)
        |
        v
+------------------+
|   AgentLoop      |  async Tokio runtime
|  (run_once)      |
+--------+---------+
         |
    +----+----+-------------+------------------+
    v         v             v                  v
 LLM       MCP Client    Guardrail         BudgetGuard
 adapter   (stdio/mock)  (Plan Mode)       (tokens/time)
    |         |             |                  |
    +----+----+-------------+------------------+
         |
         v
   ApprovalGate (CLI prompt / API poll / auto modes)
         |
         v
   SandboxExecutor -----> Host (dev) | sandboxd API (isolated)
         |
         v
   Audit + Memory + Session checkpoint (.ozr/)
```

## Canonical execution path

1. **Discover tools** — MCP `list_tools`; build catalog with `ActionKind` (Read / Write / Shell / Network).
2. **Propose plan** — LLM selects a tool; `action_kind` is synced from the MCP catalog at plan time (single source of truth).
3. **Guardrail** — `Guardrail::check_plan` evaluates risk tier; medium/high require approval. Unknown tools fall back to **Shell** (security-first).
4. **Approve** — operator approves, denies, skips, retries, or edits plan params.
5. **Execute** — `SandboxExecutor` routes Read via MCP; Write/Shell/Network via host or sandboxd depending on config and policy pack.
6. **Summarize** — LLM produces final answer; audit + memory events persisted.

## Core modules

| Module | Path | Role |
|--------|------|------|
| Agent loop | `src/core/agent_loop.rs` | Orchestrates plan → gate → execute → summarize |
| Guardrail | `src/core/guardrail.rs` | **Only** Plan Mode entry; wraps `PolicyEngine` |
| Policy + packs | `src/core/policy.rs`, `policy_pack.rs` | Risk tiers, ponytail modes, `production` sandboxd requirement |
| MCP client | `src/core/mcp_client.rs` | Mock + stdio (NDJSON / Content-Length) |
| LLM adapter | `src/core/llm_adapter.rs` | Mock + HTTP providers |
| Sandbox executor | `src/core/sandbox_executor.rs` | Host / stub / sandboxd API |
| HTTP API | `src/api/` | Axum routes, session store, OpenAI shim |
| CLI | `src/cli.rs` | `run`, `serve`, `init`, replay, checklist commands |
| GUI | `ui/` | Tauri + React workspace (alpha) |

## HTTP API surfaces

| Endpoint | Purpose |
|----------|---------|
| `GET /health` | Liveness |
| `POST /v1/run` | Start async agent session |
| `GET /v1/session/{id}` | Poll status / result / pending approval |
| `POST /v1/session/{id}/approve` | Submit approval decision |
| `POST /v1/chat/completions` | OpenAI-compatible chat (JSON or SSE stream shim) |

Agent runs are **non-blocking**: the API spawns work on Tokio and clients poll for completion or approval.

## Security layers

```text
MCP catalog ActionKind
        |
        v
   Guardrail (tier + requires_approval)
        |
        v
   ApprovalGate
        |
        v
   PolicyPack::production? --> require sandboxd executor
        |
        v
   SandboxExecutor::uses_host_execution() runtime guard
```

- **Shell is never auto-approved** in any policy pack.
- **`OZR_POLICY_PACK=production`** fails startup without sandboxd wiring; blocks risky actions on host executor at runtime.

## Deployment modes

| Mode | Command | Use case |
|------|---------|----------|
| Local dev | `cargo run -- run "..."` | Mock LLM/MCP, fast iteration |
| API server | `ozr serve` | OpenAI clients, automation |
| Docker stack | `./scripts/docker-up-stack.sh` | Self-hosted production-like stack |
| GUI + Docker | `OZR_GUI_API_BASE=http://127.0.0.1:8080` | Desktop UI against container API |

## Data on disk (`.ozr/`)

```text
.ozr/
  config.env          # local settings (gitignored)
  audit/runs.log      # append-only run events
  sessions/           # checkpoint + event log
  memory.db           # SQLite + FTS5 (optional layered memory)
```

## Testing strategy

- **Unit tests** (`cargo test --lib`) — policy, guardrail, JSON parsing, replay.
- **Smoke scenarios** (`tests/smoke_scenarios.rs`) — 10 end-to-end loop paths.
- **MCP E2E** — real `@modelcontextprotocol/server-filesystem` via stdio.
- **API / sandboxd mock** — approval flow + isolated shell routing.
- **Live integration** (`tests/integration_live.rs`, `#[ignore]`) — optional sandboxd/Qdrant.

Run `./scripts/audit-secrets.sh` before public releases.
