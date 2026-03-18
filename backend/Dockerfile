# syntax=docker/dockerfile:1
FROM rust:latest AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev libpq-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# SQLX_OFFLINE=true so query_unchecked! macros compile without DB
ENV SQLX_OFFLINE=true

# Cache dependency layer
COPY Cargo.toml ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release --bin yeet-server 2>/dev/null || true
RUN rm -rf src

# Build real source
COPY . .
RUN touch src/main.rs
RUN cargo build --release --bin yeet-server

FROM debian:bookworm-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 libpq5 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /build/target/release/yeet-server /usr/local/bin/yeet-server

EXPOSE 3000
CMD ["yeet-server"]
