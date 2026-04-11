#!/bin/bash
# YEET Social — Backend Start Script
# Run this after docker pull or VPS restart
# Usage: bash /tmp/start_backend.sh

docker rm -f yeet-backend 2>/dev/null || true

docker pull ghcr.io/zauni1984/yeet-social/backend:main

docker run -d --name yeet-backend \
  --network yeet-social_yeet-net \
  -p 8080:8080 \
  -e DATABASE_URL="postgres://yeet:YeetDB_5254a44ceae0a4a7!@yeet-postgres:5432/yeet" \
  -e REDIS_URL="redis://yeet-redis:6379" \
  -e JWT_SECRET="f270e9a02377765cf70ac4ccf1e35af55be8e7d3bac3c71e08e5e17eed62a6c2310d8a24b3d23e4d" \
  -e RUST_LOG="backend=info,tower_http=warn" \
  ghcr.io/zauni1984/yeet-social/backend:main

echo "Backend started on :8080"
sleep 5
curl -s http://127.0.0.1:8080/api/v1/health
