# syntax=docker/dockerfile:1
# Pinned base images (audit I-M7): bump deliberately, not by accident.
FROM rust:1.97.1-bookworm AS builder

WORKDIR /build

RUN apt-get update && apt-get install -y --no-install-recommends \
    pkg-config libssl-dev libpq-dev ca-certificates \
    && rm -rf /var/lib/apt/lists/*

# Accept DATABASE_URL as build argument for sqlx compile-time checks
ARG DATABASE_URL
ENV DATABASE_URL=${DATABASE_URL}

# Cache dependency layer
COPY Cargo.toml ./
RUN mkdir src && echo 'fn main() {}' > src/main.rs
RUN cargo build --release --bin yeet-server 2>/dev/null || true
RUN rm -rf src

# Build real source
COPY . .
RUN touch src/main.rs
RUN cargo build --release --bin yeet-server

FROM debian:bookworm-20261104-slim AS runtime

RUN apt-get update && apt-get install -y --no-install-recommends \
    ca-certificates libssl3 libpq5 \
    && rm -rf /var/lib/apt/lists/*

# Unprivileged service user (audit I-M7). Fixed uid/gid so volume
# ownership is stable across image rebuilds.
RUN groupadd --gid 10001 yeet \
    && useradd --uid 10001 --gid 10001 --home-dir /app --no-create-home --shell /usr/sbin/nologin yeet \
    && mkdir -p /app/uploads /app/private \
    && chown -R yeet:yeet /app

COPY --from=builder /build/target/release/yeet-server /usr/local/bin/yeet-server
COPY docker-entrypoint.sh /usr/local/bin/docker-entrypoint.sh
RUN chmod 0755 /usr/local/bin/docker-entrypoint.sh

WORKDIR /app
EXPOSE 3000
# The entrypoint starts as root only to chown the mounted volumes, then
# drops to `yeet` via setpriv (util-linux, part of the base image).
ENTRYPOINT ["/usr/local/bin/docker-entrypoint.sh"]
CMD ["yeet-server"]
