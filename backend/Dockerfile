# syntax=docker/dockerfile:1
# ── Stage 1: Build ────────────────────────────────────────────────────────────
# Rust 1.85+ required: transitive deps (time-core v0.1.8) use edition2024
FROM rust:1.85-slim-bookworm AS builder

WORKDIR /build

# System deps for OpenSSL + libpq
RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev libpq-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Cache dependency layer — build dummy binary first
COPY Cargo.toml Cargo.lock* ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
ENV SQLX_OFFLINE=true
RUN cargo build --release --bin yeet-server 2>/dev/null || true
RUN rm -rf src

# Build real source
COPY . .
RUN touch src/main.rs
RUN cargo build --release --bin yeet-server

# ── Stage 2: Runtime ──────────────────────────────────────────────────────────
FROM debian:bookworm-slim AS runtime

WORKDIR /app

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 libpq5 curl \
    && rm -rf /var/lib/apt/lists/*

# Non-root user
RUN useradd -r -s /bin/false yeet
USER yeet

COPY --from=builder /build/target/release/yeet-server /app/yeet-server
COPY --from=builder /build/migrations /app/migrations

EXPOSE 8080

HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -f http://localhost:8080/api/v1/health || exit 1

CMD ["/app/yeet-server"]
