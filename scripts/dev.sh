#!/usr/bin/env bash
# Lokale Dev-Umgebung starten:
#   Variante A (Docker):  docker compose -f docker-compose.dev.yml up -d
#   Variante B (nativ):   service postgresql start && service redis-server start
#   danach:               ./scripts/dev.sh
#
# Startet in diesem Shell:
#   - Rust-Backend (cargo run) im Hintergrund -> Port 8080
#   - Python-Dev-Proxy (Frontend + /api Proxy) -> http://localhost:5173
# Stoppen mit Ctrl+C.

set -euo pipefail
cd "$(dirname "$0")/.."

if [[ ! -f .env ]]; then
  echo "fehlt: .env (kopiere .env.example nach .env und passe Werte an)"
  exit 1
fi

# .env laden, damit cargo DATABASE_URL etc. sieht
set -a
# shellcheck disable=SC1091
source .env
set +a

cleanup() {
  echo
  echo "[dev] stopping..."
  [[ -n "${BACKEND_PID:-}" ]] && kill "$BACKEND_PID" 2>/dev/null || true
  [[ -n "${PROXY_PID:-}" ]] && kill "$PROXY_PID" 2>/dev/null || true
  wait 2>/dev/null || true
}
trap cleanup INT TERM EXIT

echo "[dev] cargo run -p backend (port 8080)"
cargo run -p backend &
BACKEND_PID=$!

# Kurz warten bis Backend hört
for _ in {1..30}; do
  if curl -fsS http://127.0.0.1:8080/api/v1/health >/dev/null 2>&1; then
    break
  fi
  sleep 1
done

echo "[dev] dev-proxy :5173 -> frontend + /api -> :8080"
python3 scripts/dev_proxy.py &
PROXY_PID=$!

echo
echo "  → http://localhost:5173  (Frontend)"
echo "  → http://localhost:8080/api/v1/health (Backend)"
echo

wait
