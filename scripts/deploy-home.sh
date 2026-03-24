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

# Load deploy env so port checks match runtime settings.
if [ -f ./.env.selfhost ]; then
  set -a
  . ./.env.selfhost
  set +a
fi

API_PORT_EFFECTIVE=\"\${API_PORT:-8080}\"
UDP_PORT_EFFECTIVE=\"\${UDP_PORT:-4433}\"
GRPC_PORT_EFFECTIVE=\"\${GRPC_PORT:-50051}\"

if [ \"\$UDP_PORT_EFFECTIVE\" = \"\$GRPC_PORT_EFFECTIVE\" ]; then
  echo \"invalid config: UDP_PORT (\$UDP_PORT_EFFECTIVE) must differ from GRPC_PORT (\$GRPC_PORT_EFFECTIVE)\" >&2
  exit 1
fi
if [ \"\$API_PORT_EFFECTIVE\" = \"\$UDP_PORT_EFFECTIVE\" ] || [ \"\$API_PORT_EFFECTIVE\" = \"\$GRPC_PORT_EFFECTIVE\" ]; then
  echo \"invalid config: API_PORT (\$API_PORT_EFFECTIVE) must differ from UDP_PORT/GRPC_PORT\" >&2
  exit 1
fi

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

echo 'Force-freeing required ports before startup...'
find_pids_on_port() {
  proto=\"\$1\"
  port=\"\$2\"
  pids=\"\"

  if command -v ss >/dev/null 2>&1; then
    if [ \"\$proto\" = \"udp\" ]; then
      ss_out=\"\$(ss -H -lunp \"sport = :\$port\" 2>/dev/null || true)\"
    else
      ss_out=\"\$(ss -H -ltnp \"sport = :\$port\" 2>/dev/null || true)\"
    fi
    ss_pids=\"\$(printf '%s\n' \"\$ss_out\" | sed -n 's/.*pid=\\([0-9][0-9]*\\).*/\\1/p' | sort -u)\"
    if [ -n \"\$ss_pids\" ]; then
      pids=\"\$ss_pids\"
    fi
  fi

  if [ -z \"\$pids\" ] && command -v netstat >/dev/null 2>&1; then
    ns_pids=\"\$(netstat -lnp 2>/dev/null | awk -v proto=\"\$proto\" -v target=\"\$port\" '
      \$1 ~ proto {
        n=split(\$4,a,\":\")
        if (a[n] == target) {
          split(\$7,b,\"/\")
          if (b[1] ~ /^[0-9]+$/) print b[1]
        }
      }
    ' | sort -u)\"
    if [ -n \"\$ns_pids\" ]; then
      pids=\"\$ns_pids\"
    fi
  fi

  if [ -z \"\$pids\" ] && [ -r \"/proc/net/\$proto\" ]; then
    hex_port=\"\$(printf '%04X' \"\$port\")\"
    inodes=\"\$(awk -v want=\"\$hex_port\" 'NR>1 {
      split(\$2, a, \":\");
      if (toupper(a[2]) == want) print \$10;
    }' \"/proc/net/\$proto\" 2>/dev/null | sort -u)\"
    if [ -n \"\$inodes\" ]; then
      found=\"\"
      for pid_dir in /proc/[0-9]*; do
        [ -d \"\$pid_dir/fd\" ] || continue
        pid=\"\${pid_dir#/proc/}\"
        for fd in \"\$pid_dir\"/fd/*; do
          link=\"\$(readlink \"\$fd\" 2>/dev/null || true)\"
          case \"\$link\" in
            socket:\\[*\\])
              inode=\"\${link#socket:[}\"
              inode=\"\${inode%]}\"
              if printf '%s\n' \"\$inodes\" | grep -qx \"\$inode\"; then
                found=\"\$found \$pid\"
                break
              fi
              ;;
          esac
        done
      done
      if [ -n \"\$found\" ]; then
        pids=\"\$(printf '%s\n' \$found | sort -u)\"
      fi
    fi
  fi

  printf '%s\n' \"\$pids\" | awk 'NF'
}

show_pid_details() {
  pids=\"\$1\"
  [ -n \"\$pids\" ] || return 0
  for pid in \$pids; do
    cmdline=\"\"
    if [ -r \"/proc/\$pid/cmdline\" ]; then
      cmdline=\"\$(tr '\\0' ' ' < \"/proc/\$pid/cmdline\" 2>/dev/null || true)\"
    fi
    if [ -z \"\$cmdline\" ] && [ -r \"/proc/\$pid/comm\" ]; then
      cmdline=\"\$(cat \"/proc/\$pid/comm\" 2>/dev/null || true)\"
    fi
    if [ -n \"\$cmdline\" ]; then
      echo \"  pid=\$pid cmd=\$cmdline\"
    else
      echo \"  pid=\$pid\"
    fi
  done
}

force_free_port() {
  proto=\"\$1\"
  port=\"\$2\"
  label=\"\$port/\$proto\"

  pids=\"\$(find_pids_on_port \"\$proto\" \"\$port\")\"
  if [ -z \"\$pids\" ]; then
    echo \"port \$label is already free\"
    return 0
  fi

  echo \"port \$label is in use by:\"
  show_pid_details \"\$pids\"

  echo \"sending TERM to \$label owners...\"
  for pid in \$pids; do
    kill \"\$pid\" 2>/dev/null || true
  done
  sleep 1

  remaining=\"\$(find_pids_on_port \"\$proto\" \"\$port\")\"
  if [ -n \"\$remaining\" ]; then
    echo \"sending KILL to remaining \$label owners...\"
    show_pid_details \"\$remaining\"
    for pid in \$remaining; do
      kill -9 \"\$pid\" 2>/dev/null || true
    done
    sleep 1
  fi

  final=\"\$(find_pids_on_port \"\$proto\" \"\$port\")\"
  if [ -n \"\$final\" ]; then
    echo \"failed to free \$label\" >&2
    show_pid_details \"\$final\"
    return 1
  fi

  echo \"port \$label is now free\"
}

force_free_port tcp \"\$API_PORT_EFFECTIVE\"
force_free_port udp \"\$UDP_PORT_EFFECTIVE\"
force_free_port tcp \"\$GRPC_PORT_EFFECTIVE\"

./scripts/install-home-runtime.sh '${APP_DIR}'

if command -v podman >/dev/null 2>&1; then
  ./scripts/home-podman.sh recreate '${APP_DIR}'
else
  docker compose build --no-cache
  docker compose up -d
fi
"
