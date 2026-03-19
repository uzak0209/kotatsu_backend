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
./scripts/install-home-runtime.sh '${APP_DIR}'
if command -v podman >/dev/null 2>&1; then
  ./scripts/home-podman.sh recreate '${APP_DIR}'
else
  docker compose up -d --build
fi
"
