#!/usr/bin/env bash
set -euo pipefail

cd /Users/uzak/Projects/kotatsu/backend/realtime-split

API_PORT=18080
GRPC_PORT=15051
UDP_PORT=14433

cleanup() {
  if [[ -n "${API_PID:-}" ]]; then kill "$API_PID" 2>/dev/null || true; fi
  if [[ -n "${RT_PID:-}" ]]; then kill "$RT_PID" 2>/dev/null || true; fi
}
trap cleanup EXIT

mkdir -p .logs

GRPC_ADDR="127.0.0.1:${GRPC_PORT}" \
UDP_BIND_ADDR="127.0.0.1:${UDP_PORT}" \
PUBLIC_HOSTNAME="127.0.0.1" \
UDP_PORT="${UDP_PORT}" \
cargo run -p kotatsu-realtime-server-split > .logs/realtime.log 2>&1 &
RT_PID=$!

for _ in {1..60}; do
  if nc -z 127.0.0.1 "$GRPC_PORT" >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done

API_ADDR="127.0.0.1:${API_PORT}" \
CONTROL_PLANE_URL="http://127.0.0.1:${GRPC_PORT}" \
cargo run -p kotatsu-api-server-split > .logs/api.log 2>&1 &
API_PID=$!

for _ in {1..60}; do
  if curl -fsS "http://127.0.0.1:${API_PORT}/health" >/dev/null 2>&1; then
    break
  fi
  sleep 0.2
done

API_BASE_URL="http://127.0.0.1:${API_PORT}" \
TICK_MS=32 \
TICKS=90 \
cargo run -p kotatsu-test-client

echo "--- realtime log tail ---"
tail -n 20 .logs/realtime.log || true

echo "--- api log tail ---"
tail -n 20 .logs/api.log || true
