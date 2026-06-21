# syntax=docker/dockerfile:1.6
#
# ozr API server (release). GUI/Tauri is host-only for now.
#
#   docker build -t ozr:local .
#   docker run --rm -p 8080:8080 -v ozr-data:/app/.ozr ozr:local

FROM rust:1-bookworm AS builder
WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY src ./src
RUN cargo build --release --bin ozr

FROM debian:bookworm-slim AS runtime

RUN apt-get update \
 && apt-get install -y --no-install-recommends ca-certificates curl python3 \
 && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /app/target/release/ozr /usr/local/bin/ozr
COPY .env.example ./.env.example
COPY scripts/wire-sandboxd.sh ./scripts/wire-sandboxd.sh
COPY docker/entrypoint.sh /entrypoint.sh

RUN chmod +x /entrypoint.sh scripts/wire-sandboxd.sh

ENV OZR_API_BIND=0.0.0.0:8080 \
    OZR_LLM_BACKEND=mock \
    OZR_MCP_BACKEND=mock

EXPOSE 8080
VOLUME ["/app/.ozr"]

HEALTHCHECK --interval=15s --timeout=3s --start-period=5s --retries=3 \
  CMD curl -sf "http://127.0.0.1:8080/health" | grep -qx ok

ENTRYPOINT ["/entrypoint.sh"]
CMD ["serve"]
