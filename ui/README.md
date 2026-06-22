# ozr GUI (Phase 4C alpha)

Desktop shell for ozr built with **Tauri + React + TypeScript + Tailwind**.

## Prerequisites

- Node.js 22+
- Rust **1.88+** for the Tauri shell (`ui/rust-toolchain.toml`)
- Core engine built once: `cargo build` from repo root (spawn mode only)

## Development

### Local spawn (default)

GUI spawns `ozr serve` on `http://127.0.0.1:18787` (not 9000 — often used by MinIO):

```bash
cargo build
cd ui
npm install
npm run tauri dev
```

Override port: `OZR_GUI_API_PORT=18788 npm run tauri dev`

If the binary is not on `PATH`:

```bash
export OZR_BINARY=/path/to/ozr/target/debug/ozr
```

### External API (Docker stack)

Start the stack from repo root, then connect the GUI without spawning a second ozr:

```bash
./scripts/docker-up-stack.sh

cd ui
OZR_GUI_API_BASE=http://127.0.0.1:8080 npm run tauri dev
```

Status bar shows `Connected (external API) · http://127.0.0.1:8080`.

For host-native ozr with Docker infra only:

```bash
./scripts/docker-up-infra.sh
./scripts/wire-sandboxd.sh
OZR_GUI_API_BASE=http://127.0.0.1:8080 npm run tauri dev   # if ozr serve runs on 8080
# or omit OZR_GUI_API_BASE to spawn locally on 18787
```

## UI flow

1. Prompt input → `POST /v1/run`
2. Poll `GET /v1/session/{id}` until `pending_approval` or `completed`
3. Approval modal → `POST /v1/session/{id}/approve`
4. Poll until `completed` and render markdown summary

Try prompt `run mystery shell task` to exercise the approval gate.

## Environment

| Variable | Default | Purpose |
|----------|---------|---------|
| `OZR_GUI_API_BASE` | _(unset)_ | Use external ozr API; skip local spawn |
| `OZR_GUI_API_PORT` | `18787` | Port when spawning locally |
| `OZR_BINARY` | repo `target/debug/ozr` | Path to ozr binary for spawn mode |
