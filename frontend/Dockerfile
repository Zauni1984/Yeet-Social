FROM rust:1.78-slim-bookworm AS builder

RUN apt-get update && apt-get install -y \
    pkg-config libssl-dev curl \
    && rm -rf /var/lib/apt/lists/*

RUN rustup target add wasm32-unknown-unknown
RUN cargo install trunk

WORKDIR /app

# Workspace Cargo.toml + alle Members kopieren
COPY Cargo.toml ./
COPY shared/ shared/
COPY backend/ backend/
COPY frontend/ frontend/

WORKDIR /app/frontend
ARG API_URL=http://76.13.150.206/api/v1
ENV API_URL=$API_URL
RUN trunk build --release

FROM nginx:alpine
COPY --from=builder /app/frontend/dist /usr/share/nginx/html
EXPOSE 80
CMD ["nginx", "-g", "daemon off;"]
