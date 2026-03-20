#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

REMOTE_HOST="${REMOTE_HOST:-kotatsu.ruxel.net}"
REMOTE_IP="${REMOTE_IP:-}"
API_PORT="${API_PORT:-8080}"
QUIC_PORT="${QUIC_PORT:-4433}"
RTT_SAMPLES="${RTT_SAMPLES:-10}"

resolve_host() {
  local host="$1"
  local resolved=""

  if command -v dig >/dev/null 2>&1; then
    resolved="$(dig +short "$host" | tail -n 1)"
  fi

  if [[ -z "$resolved" ]] && command -v getent >/dev/null 2>&1; then
    resolved="$(getent ahostsv4 "$host" 2>/dev/null | awk 'NR == 1 { print $1 }')"
  fi

  if [[ -z "$resolved" ]] && command -v host >/dev/null 2>&1; then
    resolved="$(host "$host" 2>/dev/null | awk '/has address/ { print $NF; exit }')"
  fi

  printf '%s' "$resolved"
}

if [[ -z "$REMOTE_IP" ]]; then
  REMOTE_IP="$(resolve_host "$REMOTE_HOST")"
fi

if [[ -z "$REMOTE_IP" ]]; then
  echo "failed to resolve REMOTE_HOST=$REMOTE_HOST" >&2
  exit 1
fi

API_BASE_URL="${API_BASE_URL:-http://${REMOTE_IP}:${API_PORT}}"
QUIC_OVERRIDE_URL="${QUIC_OVERRIDE_URL:-quic://${REMOTE_IP}:${QUIC_PORT}}"

echo "remote host: ${REMOTE_HOST} (${REMOTE_IP})"
echo "api base: ${API_BASE_URL}"
echo "quic override: ${QUIC_OVERRIDE_URL}"

RTT_SAMPLES="$RTT_SAMPLES" \
API_BASE_URL="$API_BASE_URL" \
QUIC_OVERRIDE_URL="$QUIC_OVERRIDE_URL" \
cargo run -p kotatsu-test-client --bin remote-rtt
