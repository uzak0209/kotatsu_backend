#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
APP_DIR_INPUT="${1:-${SCRIPT_DIR}/..}"
APP_DIR="$(cd "${APP_DIR_INPUT}" && pwd)"
IMAGE="kotatsu-backend-ddns"
STATE_DIR="${HOME}/.cache/cloudflare-ddns"

if ! command -v podman >/dev/null 2>&1; then
  echo "podman is required" >&2
  exit 1
fi

if [ ! -f "${APP_DIR}/.env.ddns" ]; then
  echo "${APP_DIR}/.env.ddns is required" >&2
  exit 1
fi

mkdir -p "${STATE_DIR}"

cd "${APP_DIR}"
set -a
. ./.env.ddns
set +a

if ! podman image exists "${IMAGE}"; then
  podman build -f Dockerfile.ddns -t "${IMAGE}" .
fi

podman run --rm \
  --network host \
  --env CF_API_TOKEN \
  --env CF_ZONE_ID \
  --env CF_RECORD_NAME \
  --env CF_RECORD_TYPE \
  --env CF_TTL \
  --env CF_PROXIED \
  --env IP_CHECK_URL \
  --env DRY_RUN \
  --env STATE_FILE=/state/ddns-state.txt \
  -v "${STATE_DIR}:/state:Z" \
  "${IMAGE}"
