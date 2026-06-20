# Project Blueprint: ozr (AI Agent Harness)

ozr คือ Open-source AI Agent Platform/Harness ที่วางตัวเป็น Exosuit ให้ LLM โดยเน้น 3 หลัก: Local-first, Security-first, Provider-agnostic (No lock-in)

## 1) Product Identity
- ชื่อ ozr สื่อภาพหุ่นยนต์โค้งคำนับ: เชื่อฟัง, สุภาพ, ทำงานตามคำสั่งอย่างปลอดภัย
- Positioning: Orchestrator ระหว่าง User, LLM, และ MCP Server
- Core promise:
  - ใช้ได้กับหลายโมเดล (OpenAI/Anthropic/Gemini/Ollama)
  - คุมสิทธิ์เครื่องมือได้ละเอียด
  - ตรวจสอบย้อนหลังได้ (Audit-ready)

## 2) System Overview

```text
User/UI
  |
  v
Agentic Loop (state machine)
  <-> LLM Protocol Adapter
  <-> MCP Client (JSON-RPC over stdio/http+sse)
  -> Policy Engine / Plan Mode Gate
  -> Budget Guard / Timeout Guard
  -> Memory Store (.ozr/markdown)
  -> Audit & Telemetry
  |
  v
MCP Servers (filesystem, shell, db, api, etc.)
```

## 3) Core Modules
1. Agentic Loop
- วงจร: Think -> Propose Plan -> Approve/Reject -> Execute -> Observe -> Summarize
- รองรับ interrupt และ resume

2. LLM Protocol Adapter
- แปลง unified message schema ไปยังแต่ละ provider
- normalize tool-call format และ usage metrics
- รองรับ model hot-swap โดยไม่ทำให้ state เพี้ยน

3. MCP Client
- discovery tools (`list_tools`) + schema cache
- call tools (`call_tool`) พร้อม timeout/retry policy
- รองรับ transport stdio และ http/sse

4. Memory Management
- โครงสร้างไฟล์ใน `.ozr/`:
  - `sessions/`
  - `project/`
  - `feedback/`
  - `artifacts/`
- เก็บเป็น markdown/json ที่มนุษย์อ่านแก้ไขได้

5. Policy Engine
- policy ระดับเครื่องมือและ path แบบ allow/deny
- risk tier: low/medium/high
- medium/high บังคับ human approval

6. Budget + Loop Guard
- token cap ต่อ run/goal
- wall-clock timeout ต่อ step/run
- max iterations + loop anomaly detection

7. Audit & Observability
- run_id, trace_id, step_id
- บันทึก decision log, tool IO summary, cost
- export log สำหรับ replay/debug

## 4) Execution Flow (Canonical)
1. Discovery: ozr อ่าน config แล้วเชื่อม MCP servers -> `list_tools`
2. Prompt Synthesis: รวม user intent + tool schemas + policy constraints
3. Model Decision: LLM เสนอ plan/tool calls (ยังไม่ execute)
4. Plan Gate:
- low risk: auto approve (ถ้า policy อนุญาต)
- medium risk: approve/reject/modify
- high risk: typed confirmation + reason
5. Execute: ozr เรียก MCP tool จริง พร้อม timeout/retry
6. Feedback: ส่งผล tool กลับให้ LLM สรุปคำตอบ
7. Persist: เก็บ memory + audit + usage

## 5) Functional Requirements
- Multi-surface:
  - CLI เป็น first-class
  - เตรียมชั้น abstraction สำหรับ Desktop GUI และ Messaging Bridge
- Plan Mode (default on):
  - ทุก action ที่กระทบ file, shell, network ต้องผ่าน gate ตาม risk tier
- Budget & Cost Control:
  - ตั้ง budget ต่อ goal และ hard stop เมื่อเกิน
- OpenAI-compatible API Mode:
  - รัน ozr เป็น API server เพื่อเรียกผ่านเครื่องมือภายนอก
- Session Recovery:
  - recover run ที่ค้าง/หลุด แล้วทำต่อได้

## 6) Non-Functional Requirements (NFR)

| Area | Target (MVP) |
|---|---|
| Reliability | Tool call success rate >= 98% (excluding provider outage) |
| Latency | p95 end-to-end for simple task <= 8s (cloud model) |
| Cost Control | Budget breach escapes = 0 |
| Security | High-risk action without approval = 0 |
| Auditability | 100% runs have run_id + decision log |
| Portability | macOS/Linux first, Windows next |

## 7) Security Model (Required)
- Trust boundaries:
  - LLM ห้ามเข้าถึงเครื่องโดยตรง
  - ทุก side effect ผ่าน MCP + policy engine เท่านั้น
- Secret handling:
  - redaction log สำหรับ token/key/password
  - ห้ามพิมพ์ secret ลง transcript แบบ plaintext
- Filesystem safety:
  - default deny นอก workspace
  - write ต้องผ่าน allowlist หรือ explicit approval
- Shell safety:
  - blocklist คำสั่ง destructive
  - high-risk shell ต้อง typed confirmation
- Network safety:
  - outbound allowlist รายโดเมนได้

## 8) Failure Semantics (Must-have)
- Timeout policy:
  - per tool call timeout
  - per step timeout
  - per run timeout
- Retry policy:
  - exponential backoff สำหรับ transient errors
  - no retry สำหรับ deterministic user errors
- Idempotency:
  - tool call ที่เสี่ยง side effect ต้องมี idempotency key
- Circuit breaker:
  - ปิด provider/tool ชั่วคราวเมื่อ fail เกิน threshold

## 9) API/Provider Compatibility Contract
- Capability Registry:
  - รองรับ/ไม่รองรับ tool calling, json mode, parallel tool calls
- Fallback routing:
  - ถ้า provider A ไม่รองรับ capability ให้ route ไป provider B
- Context policy:
  - truncation/compaction strategy แบบ deterministic

## 10) MVP Scope (6 weeks)
Week 1-2
- CLI loop + 1 provider + local file tools
- plan gate พื้นฐาน + usage tracking

Week 3-4
- MCP client (stdio) + tool schema cache
- approval UX + retry/timeout guards

Week 5-6
- memory store `.ozr/` + session recovery
- audit log + replay-lite + budget hard stop

## 11) Acceptance Criteria by Phase
Phase 1
- รับ prompt -> เรียก tool -> สรุปผลครบลูปได้
- มี unit tests สำหรับ loop transitions สำคัญ

Phase 2
- MCP server จริงอย่างน้อย 1 ตัวทำงานครบ discovery/call
- action เสี่ยงถูกดัก approve ได้ครบ

Phase 3
- resume session ได้จาก crash/restart
- replay run ล่าสุดได้พร้อมสาเหตุ fail

Phase 4 (หลัง MVP)
- GUI alpha
- API mode compatible กับ client ภายนอกที่กำหนด

## 12) Suggested Repo Layout
```text
ozr/
  cmd/
  core/
    agent_loop/
    policy/
    budget/
    memory/
    audit/
    llm_adapter/
    mcp_client/
  api/
  ui/
  configs/
  tests/
  .ozr/
```

## 13) Benchmarking Focus
- thClaws: single-binary performance + markdown memory
- CheetahClaws: context compaction + loop ergonomics
- Dr. Claw: developer UX + session recovery

## 14) Implementation Checklist (Start Here)
- [ ] เลือกภาษา implementation หลัก (Rust หรือ Python)
- [ ] นิยาม unified message schema + tool schema adapter
- [ ] ลง policy model (risk tiers + allow/deny rules)
- [ ] ทำ MCP stdio client พร้อม timeout/retry
- [ ] ทำ plan approval CLI UX
- [ ] ทำ budget/token hard-stop
- [ ] ทำ audit log (run_id/trace_id/decision log)
- [ ] ทำ session recovery ขั้นแรก
- [ ] เขียน smoke tests แบบ end-to-end 10 เคส

## 15) Architecture Decisions Needed Now
1. จะ optimize speed หรือ security ก่อนใน MVP
2. จะ lock provider เดียวก่อน หรือทำ multi-provider ตั้งแต่วันแรก
3. API compatibility ต้องถึงระดับใด (minimal vs drop-in)
4. multi-agent จะอยู่ใน roadmap หลัง single-agent stable หรือไม่

## 16) External Integration Track
- ดูสเปกการนำเครื่องมือภายนอกเข้าระบบได้ที่ INTEGRATION_SPEC.md
- เป้าหมายคือเพิ่ม workflow governance, ลด overengineering, เสริม sandbox isolation, และยกระดับ memory แบบค่อยเป็นค่อยไป

---
เอกสารนี้ตั้งใจให้เป็น single source of truth สำหรับเริ่มพัฒนา ozr โดยลดความซ้ำซ้อน และเพิ่มข้อกำหนดที่วัดผลได้เพื่อเดินงานจริงได้ทันที
