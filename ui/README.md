# ozr GUI (Phase 4C alpha)

Desktop shell for ozr built with **Tauri + React + TypeScript + Tailwind**.

## Prerequisites

- Node.js 22+
- Rust **1.88+** for the Tauri shell (`ui/rust-toolchain.toml`)
- Core engine built once: `cargo build` from repo root

## Development

```bash
# terminal 1 — build core if needed
cargo build

# terminal 2 — GUI
cd ui
npm install
npm run tauri dev
```

The GUI spawns `ozr serve` on `http://127.0.0.1:18787` by default (not 9000 — often used by MinIO). Override with `OZR_GUI_API_PORT`.

If the binary is not on `PATH`, set:

```bash
export OZR_BINARY=/path/to/ozr/target/debug/ozr
```

## UI flow

1. Prompt input → `POST /v1/run`
2. Poll `GET /v1/session/{id}` until `pending_approval` or `completed`
3. Approval modal → `POST /v1/session/{id}/approve`
4. Poll until `completed` and render markdown summary

Try prompt `run mystery shell task` to exercise the approval gate.
