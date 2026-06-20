# Sandboxd Production Deployment Guide

คู่มือนี้เสริม `ozr sandboxd-checklist` สำหรับ deploy sandboxd + ozr ใน production
โดยครอบคลุม HTTPS termination, token rotation และ egress control

## 1) Architecture baseline

```text
Client (ozr CLI/agent)
  -> TLS ingress (nginx/traefik/Caddy)
    -> sandboxd API (private network)
      -> per-user/task sandboxes (Docker/k8s)
```

หลักการ
- เปิด sandboxd API ให้ ozr เท่านั้น ไม่ expose ตรงสู่ public internet ถ้าเป็นไปได้
- บังคับ auth ทุก request ใน production
- จำกัด egress ของ sandbox ให้เหลือเฉพาะ domain ที่จำเป็น

## 2) ozr configuration (production)

ตั้งค่าใน `.ozr/config.env` หรือ secret manager แล้ว inject เป็น environment

```text
OZR_FEATURE_SANDBOXD_EXECUTOR=true
OZR_SANDBOXD_API_BASE=https://sandboxd.internal.example
OZR_SANDBOXD_SANDBOX_ID=ozr-prod-isolated
OZR_SANDBOXD_API_TOKEN=<scoped bearer token>
OZR_SANDBOXD_REQUIRE_AUTH=true
OZR_SANDBOXD_HTTPS_ONLY=true
OZR_SANDBOXD_CAPTURE_EVENTS=true
OZR_SANDBOXD_EVENTS_MAX_TIME_S=5
```

ตรวจสอบก่อน deploy

```bash
ozr sandboxd-checklist
```

- PASS/WARN/FAIL report อยู่ที่ `.ozr/audit/sandboxd-checklist.md`
- ถ้ามี FAIL ให้แก้ config ก่อนเปิด traffic

## 3) HTTPS termination

### แนวทางที่แนะนำ
- Terminate TLS ที่ reverse proxy (nginx/traefik/Caddy)
- sandboxd ฟัง HTTP ภายใน private network
- ozr ชี้ `OZR_SANDBOXD_API_BASE` ไปที่ HTTPS endpoint ของ ingress

### nginx ตัวอย่าง (minimal)

```nginx
server {
  listen 443 ssl http2;
  server_name sandboxd.internal.example;

  ssl_certificate     /etc/ssl/certs/sandboxd.crt;
  ssl_certificate_key /etc/ssl/private/sandboxd.key;

  location / {
    proxy_pass http://127.0.0.1:9090;
    proxy_set_header Host $host;
    proxy_set_header X-Forwarded-Proto https;
    proxy_read_timeout 300s;
  }
}
```

Checklist
- ใช้ TLS 1.2+ และ cipher suite ที่ทันสมัย
- เปิด HSTS ถ้า endpoint เป็น public-facing จริง
- จำกัด source IP ของ ozr runner ที่ ingress/firewall

## 4) Token rotation

### Token model
- ใช้ bearer token แยกต่อ environment (dev/staging/prod)
- scope ให้แคบ: sandbox เดียว + API actions ที่จำเป็น
- เก็บ token ใน secret manager (Vault, AWS SM, GCP SM) ไม่ commit ใน git

### Rotation procedure (zero/low downtime)

1. สร้าง token ใหม่ (token-B) ที่ identity provider/sandboxd admin
2. อัปเดต secret ของ ozr ให้รองรับ token-B (dual-read window ถ้ารองรับ)
3. รัน smoke test

```bash
OZR_RUN_INTEGRATION=1 cargo test --test integration_live sandboxd_live_fixture -- --ignored
```

4. revoke token เก่า (token-A)
5. บันทึก rotation ใน change log / audit

### Rotation cadence
- Production: ทุก 30–90 วัน หรือทันทีเมื่อมี incident
- Staging: ทุก sprint/release train
- บังคับ rotate ทันทีเมื่อ token รั่ว, engineer offboarding, หรือ policy breach

## 5) Egress control

sandbox ควร outbound ได้เฉพาะสิ่งที่ agent task ต้องใช้จริง

### Policy tiers

| Tier | Allow | Deny by default |
|---|---|---|
| Strict | package registries ที่อนุมัติ, internal APIs | public internet อื่นๆ |
| Balanced | registries + docs sites + approved SaaS | broad crawl / unknown domains |
| Dev-only | wider outbound | ใช้เฉพาะ local/dev |

### Docker/network controls
- ใช้ dedicated Docker network แยก sandboxd control plane vs sandbox workloads
- จำกัด `--network` และ published ports ต่อ sandbox
- block metadata endpoints (169.254.169.254) และ internal admin ranges

### DNS + firewall
- บังคับ DNS ผ่าน resolver ที่ filter (allowlist)
- egress firewall rule: default deny, allowlist per CIDR/domain
- log denied egress เพื่อ tune allowlist

### ozr-side guardrails
- high-risk tools (shell/network/write) route เข้า sandboxd executor
- เปิด `OZR_SANDBOXD_CAPTURE_EVENTS=true` เพื่อ audit replay
- ตรวจ `.ozr/audit/sandboxd-events-*.json` หลัง run ที่ sensitive

## 6) Operational runbook

### Pre-deploy
1. `ozr init` (ถ้ายังไม่มี workspace)
2. ตั้ง `.ozr/config.env` production values
3. `ozr sandboxd-checklist` ต้องไม่มี FAIL
4. (optional) `./scripts/run-integration.sh`

### Post-deploy monitoring
- อัตรา task failed/succeeded จาก sandboxd
- 401/403 spike (auth misconfig หรือ stale token)
- latency p95 ของ task polling
- audit volume ใน `.ozr/audit/`

### Incident response
1. revoke token ที่สงสัย
2. เปลี่ยน `OZR_SANDBOXD_SANDBOX_ID` เป็น sandbox ใหม่ถ้ามี compromise
3. ปิด `OZR_FEATURE_SANDBOXD_EXECUTOR` ชั่วคราว → fallback host executor ตาม policy
4. เก็บ `.ozr/audit/sandboxd-events-*.json` เป็น evidence

## 7) CI integration profile

Unit tests (default CI)

```bash
cargo test --lib
```

Live fixtures (optional)

```bash
./scripts/run-integration.sh
# หรือ
OZR_RUN_INTEGRATION=1 cargo test --test integration_live -- --ignored
```

GitHub Actions
- `CI` workflow: unit tests ทุก PR/push
- `Integration` workflow: manual dispatch
  - qdrant fixture รันกับ service container อัตโนมัติ
  - sandboxd fixture เปิดด้วย input `run_sandboxd=true` + repo secrets:
    - `OZR_SANDBOXD_API_BASE`
    - `OZR_SANDBOXD_API_TOKEN`
    - `OZR_SANDBOXD_SANDBOX_ID`

## 8) Related artifacts

- Template: `.ozr/sandboxd-production-checklist.md` (สร้างจาก `ozr init`)
- Automated report: `.ozr/audit/sandboxd-checklist.md`
- Integration env sample: `tests/fixtures/integration.env.example`
- Spec: `INTEGRATION_SPEC.md`
