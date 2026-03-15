# ── Build stage (Trunk + wasm-pack) ──────────────────────────────────────────
FROM rust:1.78-slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev curl \
    && rm -rf /var/lib/apt/lists/*

# Install wasm target + Trunk
RUN rustup target add wasm32-unknown-unknown
RUN cargo install trunk

WORKDIR /app
COPY Cargo.toml Cargo.lock ./
COPY shared/ shared/
COPY frontend/ frontend/

WORKDIR /app/frontend
RUN trunk build --release

# ── Runtime: serve with nginx ─────────────────────────────────────────────────
FROM nginx:alpine
COPY --from=builder /app/frontend/dist /usr/share/nginx/html
COPY frontend/nginx.conf /etc/nginx/conf.d/default.conf
EXPOSE 80
