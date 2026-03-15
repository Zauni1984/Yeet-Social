FROM rust:1.78-slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev libpq-dev \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Temporaeres Workspace Cargo.toml ohne frontend
COPY Cargo.toml ./
RUN sed -i 's|"frontend",||g' Cargo.toml && \
    sed -i 's|"frontend"||g' Cargo.toml

COPY shared/ shared/
COPY backend/ backend/

RUN cargo build --release --bin yeet-server

FROM debian:bookworm-slim

RUN apt-get update && apt-get install -y \
    ca-certificates libpq5 libssl3 \
    && rm -rf /var/lib/apt/lists/*

COPY --from=builder /app/target/release/yeet-server /usr/local/bin/yeet-server
COPY --from=builder /app/backend/migrations /migrations

EXPOSE 8080
CMD ["yeet-server"]
