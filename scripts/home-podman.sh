#!/bin/sh
set -eu

if [ "$#" -lt 1 ] || [ "$#" -gt 2 ]; then
  echo "Usage: $0 <recreate|up|down> [app_dir]" >&2
  exit 1
fi

ACTION="$1"
SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
APP_DIR_INPUT="${2:-${SCRIPT_DIR}/..}"
APP_DIR="$(cd "${APP_DIR_INPUT}" && pwd)"

REALTIME_CONTAINER="kotatsu-backend-realtime"
API_CONTAINER="kotatsu-backend-api"
REALTIME_IMAGE="kotatsu-backend-realtime"
API_IMAGE="kotatsu-backend-api"

if ! command -v podman >/dev/null 2>&1; then
  echo "podman is required" >&2
  exit 1
fi

if [ ! -f "${APP_DIR}/.env.selfhost" ]; then
  echo "${APP_DIR}/.env.selfhost is required" >&2
  exit 1
fi

cd "${APP_DIR}"
set -a
. ./.env.selfhost
set +a

container_exists() {
  podman container exists "$1"
}

image_exists() {
  podman image exists "$1"
}

ensure_images() {
  if ! image_exists "${REALTIME_IMAGE}"; then
    podman build -f Dockerfile.realtime -t "${REALTIME_IMAGE}" .
  fi

  if ! image_exists "${API_IMAGE}"; then
    podman build -f Dockerfile.api -t "${API_IMAGE}" .
  fi
}

run_realtime() {
  podman run -d --replace \
    --name "${REALTIME_CONTAINER}" \
    --network host \
    --restart always \
    --env-file .env.selfhost \
    -e "GRPC_ADDR=0.0.0.0:${GRPC_PORT:-50051}" \
    -e "UDP_BIND_ADDR=0.0.0.0:${UDP_PORT:-4433}" \
    -e "UDP_PUBLIC_URL=${UDP_PUBLIC_URL:-udp://127.0.0.1:4433}" \
    "${REALTIME_IMAGE}"
}

run_api() {
  podman run -d --replace \
    --name "${API_CONTAINER}" \
    --network host \
    --restart always \
    --env-file .env.selfhost \
    -e "API_ADDR=0.0.0.0:${API_PORT:-8080}" \
    -e "CONTROL_PLANE_URL=http://127.0.0.1:${GRPC_PORT:-50051}" \
    "${API_IMAGE}"
}

recreate() {
  podman build --no-cache -f Dockerfile.realtime -t "${REALTIME_IMAGE}" .
  podman build --no-cache -f Dockerfile.api -t "${API_IMAGE}" .
  run_realtime
  run_api
}

up() {
  ensure_images

  if container_exists "${REALTIME_CONTAINER}"; then
    podman start "${REALTIME_CONTAINER}" >/dev/null || true
  else
    run_realtime
  fi

  if container_exists "${API_CONTAINER}"; then
    podman start "${API_CONTAINER}" >/dev/null || true
  else
    run_api
  fi
}

down() {
  podman stop -t 10 "${API_CONTAINER}" >/dev/null 2>&1 || true
  podman stop -t 10 "${REALTIME_CONTAINER}" >/dev/null 2>&1 || true
}

case "${ACTION}" in
  recreate)
    recreate
    ;;
  up)
    up
    ;;
  down)
    down
    ;;
  *)
    echo "Unknown action: ${ACTION}" >&2
    exit 1
    ;;
esac
