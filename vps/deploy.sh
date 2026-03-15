#!/bin/bash
set -euo pipefail
cd /opt/yeet
echo "[$(date)] Deploy gestartet..."
docker compose pull --quiet
docker compose up -d --remove-orphans
sleep 10
curl -sf http://localhost:8080/api/v1/feed > /dev/null && \
    echo "✅ Health check OK" || echo "⚠️  Health check fehlgeschlagen"
docker image prune -f --filter "until=24h" > /dev/null
echo "[$(date)] Deploy abgeschlossen"
