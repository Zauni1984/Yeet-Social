# syntax=docker/dockerfile:1
# Uses rust:latest to always get newest stable Rust
# Required: time@0.3.47 + home@0.5.12 need rustc >= 1.88.0
FROM rust:latest AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev libpq-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Cache deps layer
COPY Cargo.toml ./
ENV SQLX_OFFLINE=true
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release --bin yeet-server 2>/dev/null || true
RUN rm -rf src

# Build real source
COPY . .
RUN touch src/main.rs
RUN cargo build --release --bin yeet-server

FROM debian:bookworm-slim AS runtime
WORKDIR /app
RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 libpq5 curl \
    && rm -rf /var/lib/apt/lists/*
RUN useradd -r -s /bin/false yeet
USER yeet
COPY --from=builder /build/target/release/yeet-server /app/yeet-server
COPY --from=builder /build/migrations /app/migrations
EXPOSE 8080
HEALTHCHECK --interval=30s --timeout=5s --start-period=15s --retries=3 \
    CMD curl -f http://localhost:8080/api/v1/health || exit 1
CMD ["/app/yeet-server"]
