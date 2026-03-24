#!/usr/bin/env bash
set -euo pipefail

if [[ $# -lt 2 || $# -gt 4 ]]; then
  echo "Usage: $0 <host> <user> [app_dir] [ssh_port]" >&2
  exit 1
fi

HOST="$1"
USER_NAME="$2"
APP_DIR="${3:-/home/${USER_NAME}/kotatsu-backend}"
SSH_PORT="${4:-22}"
SSH_TARGET="${USER_NAME}@${HOST}"
SSH_KEY_PATH="${SSH_KEY_PATH:-}"
SSH_OPTS=(-p "${SSH_PORT}")
SCP_OPTS=(-P "${SSH_PORT}")

if [[ -n "${SSH_KEY_PATH}" ]]; then
  SSH_OPTS+=(-i "${SSH_KEY_PATH}")
  SCP_OPTS+=(-i "${SSH_KEY_PATH}")
fi

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
REPO_ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"

if [[ ! -f "${REPO_ROOT}/.env.selfhost" ]]; then
  echo ".env.selfhost is required at repo root" >&2
  echo "Create it from .env.selfhost.example before deploy." >&2
  exit 1
fi

ssh "${SSH_OPTS[@]}" "${SSH_TARGET}" "mkdir -p '${APP_DIR}'"

tar \
  --exclude='.git' \
  --exclude='node_modules' \
  --exclude='realtime-split/target' \
  --exclude='realtime-split/.logs' \
  --exclude='.env.selfhost' \
  -C "${REPO_ROOT}" \
  -cf - . | ssh "${SSH_OPTS[@]}" "${SSH_TARGET}" "tar -C '${APP_DIR}' -xf -"

scp "${SCP_OPTS[@]}" "${REPO_ROOT}/.env.selfhost" "${SSH_TARGET}:${APP_DIR}/.env.selfhost"

ssh "${SSH_OPTS[@]}" "${SSH_TARGET}" "
set -e
cd '${APP_DIR}'

# Stop all running containers first
echo 'Stopping existing containers...'
if command -v podman >/dev/null 2>&1; then
  podman stop kotatsu-backend-realtime kotatsu-backend-api 2>/dev/null || true
  podman rm kotatsu-backend-realtime kotatsu-backend-api 2>/dev/null || true
  # Wait for ports to be released
  sleep 2
else
  docker compose down
  # Wait for ports to be released
  sleep 2
fi

echo 'Checking required ports before startup...'
check_port_free() {
  local proto=\"\$1\"
  local port=\"\$2\"
  if command -v ss >/dev/null 2>&1; then
    if [ \"\$proto\" = \"udp\" ]; then
      if ss -H -lun \"sport = :\$port\" | grep -q .; then
        echo \"port \$port/\$proto is already in use\" >&2
        ss -lun \"sport = :\$port\" || true
        return 1
      fi
    else
      if ss -H -ltn \"sport = :\$port\" | grep -q .; then
        echo \"port \$port/\$proto is already in use\" >&2
        ss -ltn \"sport = :\$port\" || true
        return 1
      fi
    fi
  fi
}

check_port_free tcp \"\${API_PORT:-8080}\"
check_port_free udp \"\${UDP_PORT:-4433}\"
check_port_free tcp \"\${GRPC_PORT:-50051}\"

./scripts/install-home-runtime.sh '${APP_DIR}'

if command -v podman >/dev/null 2>&1; then
  ./scripts/home-podman.sh recreate '${APP_DIR}'
else
  docker compose build --no-cache
  docker compose up -d
fi
"
