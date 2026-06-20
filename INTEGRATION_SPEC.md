# ozr External Tool Integration Spec

เอกสารนี้กำหนดวิธีนำเครื่องมือภายนอกเข้ามาเสริม ozr โดยยึดหลัก
- Local-first
- Security-first
- No provider lock-in
- Incremental rollout (เริ่มจาก low-risk ก่อน)

## 1) Scope

Tools ที่ประเมิน
- Spec Kitty: spec-driven workflow + governance runtime
- Ponytail: anti-overengineering ruleset สำหรับ agent coding
- sandboxd: sandbox runtime สำหรับรัน agent/task แบบแยกสภาพแวดล้อม
- memory-os: memory stack หลายชั้นสำหรับ long-term context

สิ่งที่นอกขอบเขตระยะต้น
- การผูกระบบทั้งหมดแบบ tightly-coupled ตั้งแต่วันแรก
- การแทนที่ core loop ของ ozr ด้วยโปรเจกต์ภายนอก

## 2) Compatibility Summary

### 2.1 Spec Kitty
- สถานะ: Compatible สูง (workflow layer)
- เหตุผล
  - local-first และ MIT
  - โฟกัส mission/spec/plan/tasks/review ที่เติม governance ให้ ozr ได้ตรงช่องว่าง
  - ไม่บังคับให้ผูกกับ model provider ใดรายเดียว
- บทบาทใน ozr
  - ใช้เป็น Mission Orchestration Layer (เหนือ agent loop)

### 2.2 Ponytail
- สถานะ: Compatible กลาง-สูง (policy/rules layer)
- เหตุผล
  - เน้นลด overengineering, ลด token/cost, ยังย้ำ safety
  - เหมาะเป็น rule profile สำหรับ code generation mode
- บทบาทใน ozr
  - ใช้เป็น optional Rule Pack (lite/full/ultra equivalent)

### 2.3 sandboxd
- สถานะ: Compatible สูง (execution isolation layer)
- เหตุผล
  - แยก sandbox ต่อผู้ใช้/งาน, มี API ชัดเจน, มี task SSE
  - เหมาะกับ Plan Mode + high-risk tool execution ของ ozr
- ข้อควรระวัง
  - ต้องเปิด auth ใน production
  - โฮสต์ Docker socket คือ trust boundary สำคัญ
- บทบาทใน ozr
  - เป็น Remote Execution Backend สำหรับ shell/file/network tools

### 2.4 memory-os
- สถานะ: Compatible กลาง (memory layer)
- เหตุผล
  - แนวคิดหลายชั้นและ context injection ดีมาก
  - แต่ stack ค่อนข้างหนัก (Qdrant/Redis/worker) สำหรับ MVP
- บทบาทใน ozr
  - ใช้แบบ staged adoption: เริ่มจากแนวคิด schema + retrieval policy ก่อน

## 3) Target Architecture Mapping

```text
ozr UI/CLI/API
  -> ozr Agent Loop
    -> Policy Engine + Plan Gate
    -> LLM Adapter
    -> Memory Orchestrator
    -> Tool Router
       -> Local MCP Tools
       -> sandboxd Backend (isolated execution)

Workflow Plane (optional): Spec Kitty
Rule Pack (optional): Ponytail profile in coding mode
Memory Plane (progressive): memory-os inspired layered memory
```

## 4) Integration Plan by Phase

## Phase A (ทันที, 1-2 สัปดาห์): Workflow + Guardrails
เป้าหมาย
- เพิ่มความเป็นระบบในการพัฒนาฟีเจอร์ ozr เอง

งาน
- ใช้ Spec Kitty เป็น development workflow ของทีม ozr
- สร้าง Ponytail profile ใน ozr coding mode เป็น toggle
- วัดผล baseline:
  - diff size
  - token usage
  - cycle time
  - defect rate หลัง review

Acceptance
- มี mission artifacts ต่อฟีเจอร์หลักทุกงาน
- มีการเปิด/ปิด Ponytail profile ได้จาก config

## Phase B (2-4 สัปดาห์): sandboxd Adapter
เป้าหมาย
- รันงานเสี่ยงใน sandbox แทน host โดยตรง

งาน
- สร้าง `SandboxExecutor` trait ใน ozr
- implementation 2 แบบ:
  - `HostExecutor` (เดิม)
  - `SandboxdExecutor` (ใหม่)
- map tool actions -> sandboxd API
  - create sandbox
  - submit task
  - stream events
  - read/write files
  - stop/destroy

Security requirements
- production บังคับ token auth
- default deny env passthrough ยกเว้น allowlist
- high-risk action ต้องมี approval reason

Acceptance
- high-risk tool call ถูก route เข้า sandboxd ได้ 100%
- host direct execution ลดลงตาม policy ที่ตั้ง

## Phase C (4-6 สัปดาห์): Memory Layer Expansion
เป้าหมาย
- เพิ่มคุณภาพ recall โดยยังคุมความซับซ้อน

งาน
- นำ memory-os concept มาออกแบบ `MemoryOrchestrator` ใน ozr
- เริ่มจาก 3 ชั้นก่อน
  - Layer 1: workspace docs (`.ozr/` markdown)
  - Layer 2: session index (SQLite + FTS)
  - Layer 3: structured facts (SQLite + trust score)
- เก็บ vector layer (Qdrant) เป็น optional plugin

Acceptance
- context relevance score ดีขึ้นตามชุด eval
- token overhead จาก recall เพิ่มไม่เกิน budget ที่กำหนด

## Phase D (หลัง MVP): Deep Coupling and Automation
เป้าหมาย
- เสถียรภาพและ scale

งาน
- เพิ่ม replay/debug trace ข้าม sandbox + memory events
- เพิ่ม policy packs หลายแบบ (strict/balanced/fast)
- พิจารณา k8s backend abstraction แทน docker-only

## 5) Interface Contracts (for ozr codebase)

### 5.1 Sandbox Executor
```text
trait SandboxExecutor {
  create(env, ports) -> sandbox_id
  exec(sandbox_id, cmd, timeout) -> result
  run_task(sandbox_id, prompt, agent, timeout) -> task_id
  stream_events(sandbox_id, task_id) -> event_stream
  read_file(sandbox_id, path) -> bytes
  write_file(sandbox_id, path, content) -> ok
  stop(sandbox_id) -> ok
  destroy(sandbox_id, purge) -> ok
}
```

### 5.2 Memory Orchestrator
```text
trait MemoryOrchestrator {
  ingest(run_event) -> ok
  recall(query, budget) -> memory_bundle
  score(bundle, query) -> relevance_score
  compact(session_id) -> summary_artifact
}
```

### 5.3 Rule Profile (Ponytail-like)
```text
mode: off | lite | full | ultra
apply_to: coding_tasks_only
guarantees:
  - never remove validation/security/accessibility requirements
```

## 6) Risk Register

1. Over-coupling with external projects
- Mitigation: adapter pattern + feature flags + graceful fallback

2. sandboxd security misconfiguration
- Mitigation: prod checklist, token auth required, egress control

3. memory stack complexity explosion
- Mitigation: progressive layers, optional vector backend

4. rule profile over-pruning
- Mitigation: keep safety invariants non-negotiable + review gate

## 7) Rollout Flags

```text
OZR_FEATURE_SPEC_KITTY_WORKFLOW=true|false
OZR_FEATURE_PONYTAIL_PROFILE=off|lite|full|ultra
OZR_FEATURE_SANDBOXD_EXECUTOR=true|false
OZR_FEATURE_MEMORY_LAYERED=true|false
OZR_FEATURE_VECTOR_BACKEND=none|qdrant
```

## 8) Success Metrics
- Security
  - host-side high-risk execution rate
  - unauthorized execution incidents
- Quality
  - review rejection rate
  - regression count per feature
- Efficiency
  - median tokens per completed task
  - median time-to-complete
- Memory
  - recall hit-rate
  - irrelevant memory injection rate

## 9) Recommended Decision Now
- Adopt now
  - Spec Kitty as dev workflow
  - Ponytail profile as optional coding rule pack
- Adopt next
  - sandboxd executor for high-risk actions
- Adopt progressively
  - memory-os concepts first, full stack later as optional plugin

## 10) Implementation Status (Current)
- Done now in ozr
  - Ponytail mode in policy evaluation (off/lite/full/ultra)
  - Feature flags from environment
  - Sandbox executor abstraction (host + sandboxd stub + sandboxd API task submit + task polling + optional event capture)
  - Spec Kitty workflow adapter (`workflow` command)
  - Layered memory orchestrator adapter (`memory` command + prompt enrichment)
  - Persistent memory index + trust scoring (`OZR_MEMORY_BACKEND=sqlite`)
  - Real SQLite + FTS5 memory store via `rusqlite` (Rust 1.85+ pinned in `rust-toolchain.toml`)
  - Approval insights command with automated alerts and policy tuning suggestions
  - Optional Qdrant vector recall plugin (`OZR_FEATURE_VECTOR_BACKEND=qdrant`)
  - Strict JSON parsing for sandboxd/MCP/LLM via shared `json_util` + `serde_json`
  - Sandboxd SSE event capture schema v1 with per-event summary and audit artifact export
  - Sandboxd transport/auth policy gates (`OZR_SANDBOXD_REQUIRE_AUTH`, `OZR_SANDBOXD_HTTPS_ONLY`)
  - Qdrant embedding pipeline via OpenAI-compatible embeddings API (`OZR_VECTOR_EMBEDDINGS=true`)
  - Sandboxd production checklist command (`ozr sandboxd-checklist`) with PASS/WARN/FAIL audit report

### Sandboxd production checklist
- Command: `ozr sandboxd-checklist`
- Output: `.ozr/audit/sandboxd-checklist.md`
- Template on init: `.ozr/sandboxd-production-checklist.md`
- Checks include: token presence, HTTPS policy, sandbox id, auth flags, poll budget, event capture, token rotation, egress control
- Deployment guide: `docs/sandboxd-production.md` (HTTPS termination, token rotation, egress control)
  - Live integration fixtures (`tests/integration_live.rs`) with optional CI profile:
  - local: `./scripts/run-integration.sh` (`OZR_RUN_INTEGRATION=1`)
  - CI unit: `.github/workflows/ci.yml`
  - CI integration: `.github/workflows/integration.yml` (qdrant service + optional sandboxd secrets)
  - Guardrail module for Plan Mode policy evaluation (`src/core/guardrail.rs`)
  - Session recovery checkpoint (`.ozr/sessions/checkpoint.json`) with `ozr session status|resume`
  - Replay-lite audit reports (`ozr replay [run_id]`) with failure diagnosis + sandboxd artifact correlation
  - Tokio async core engine entrypoint (`#[tokio::main]`, async `AgentLoop::run_once`)
  - Plan Mode security gate centralized in `Guardrail::check_plan` (`PolicyEngine::evaluate` is crate-private)
  - Smoke scenarios: `tests/smoke_scenarios.rs` (10 end-to-end loop cases)
  - Configurable run budget via `OZR_BUDGET_MAX_TOKENS`, `OZR_BUDGET_MAX_ITERATIONS`, `OZR_BUDGET_MAX_RUN_SECONDS`
  - Cross-trace replay in `ozr replay` (audit + session checkpoint + memory events + sandboxd summaries)
  - Async sandboxd I/O via `tokio::process` + `async-trait` `SandboxExecutor`
  - Policy packs via `OZR_POLICY_PACK=strict|balanced|fast`
  - Agent loop transition tests + memory events tagged with `run_id`
  - Production MCP stdio client (initialize + initialized + framed JSON-RPC, timeout/retry, tool catalog)
  - Async MCP stdio client via `tokio::process` + `async-trait` `McpClient`
  - MCP fixture server + `tests/mcp_stdio.rs` + `tests/mcp_filesystem.rs` + `ozr mcp list`
  - Async LLM HTTP via `tokio::process` curl (`HttpLlmProvider` + `async-trait` `LlmProvider`)
  - Phase 4A Axum HTTP API (`ozr serve`) with session store + approval polling endpoints
  - `propose_plan` receives MCP tool catalog and sets `action_kind` at plan time
  - Unknown tools outside catalog fall back to `Shell` (security-first MVP)
  - Security policy: shell never auto-approved in any policy pack

---
สรุป: ทั้ง 4 โปรเจกต์สอดคล้องกับวิสัยทัศน์ ozr ได้ หากนำเข้าแบบแยกชั้นผ่าน adapter และ feature flags โดยเริ่มจาก workflow/rules ก่อน แล้วค่อยขยายไป execution isolation และ advanced memory ตามลำดับ