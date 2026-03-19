#!/bin/sh
set -eu

SCRIPT_DIR="$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)"
APP_DIR_INPUT="${1:-${SCRIPT_DIR}/..}"
APP_DIR="$(cd "${APP_DIR_INPUT}" && pwd)"
SYSCTL_SOURCE="${APP_DIR}/.sysctl.selfhost"
BOOT_SCRIPT="${APP_DIR}/scripts/home-podman.sh"

as_root() {
  if [ "$(id -u)" -eq 0 ]; then
    "$@"
  elif command -v doas >/dev/null 2>&1; then
    doas "$@"
  elif command -v sudo >/dev/null 2>&1; then
    sudo "$@"
  else
    return 127
  fi
}

warn_root_skip() {
  echo "warning: skipping ${1}; root, doas, or sudo is required" >&2
}

install_sysctl() {
  if [ ! -f "${SYSCTL_SOURCE}" ]; then
    return 0
  fi

  if ! as_root true >/dev/null 2>&1; then
    warn_root_skip "sysctl persistence"
    return 0
  fi

  tmp_file="$(mktemp)"
  cp "${SYSCTL_SOURCE}" "${tmp_file}"

  as_root mkdir -p /etc/sysctl.d
  as_root install -m 0644 "${tmp_file}" /etc/sysctl.d/99-kotatsu.conf
  rm -f "${tmp_file}"

  if command -v sysctl >/dev/null 2>&1; then
    as_root sh -c "sysctl --load /etc/sysctl.d/99-kotatsu.conf >/dev/null 2>&1 || sysctl -p /etc/sysctl.d/99-kotatsu.conf >/dev/null"
  fi
}

install_openrc_boot_hook() {
  if ! command -v podman >/dev/null 2>&1; then
    return 0
  fi

  if ! command -v rc-update >/dev/null 2>&1 || [ ! -d /etc/local.d ]; then
    return 1
  fi

  if ! as_root true >/dev/null 2>&1; then
    warn_root_skip "OpenRC boot hook"
    return 0
  fi

  start_tmp="$(mktemp)"
  stop_tmp="$(mktemp)"

  cat > "${start_tmp}" <<EOF
#!/bin/sh
exec "${BOOT_SCRIPT}" up "${APP_DIR}"
EOF

  cat > "${stop_tmp}" <<EOF
#!/bin/sh
exec "${BOOT_SCRIPT}" down "${APP_DIR}"
EOF

  as_root install -m 0755 "${start_tmp}" /etc/local.d/kotatsu.start
  as_root install -m 0755 "${stop_tmp}" /etc/local.d/kotatsu.stop
  rm -f "${start_tmp}" "${stop_tmp}"

  as_root rc-update add local default >/dev/null 2>&1 || true
}

install_systemd_boot_hook() {
  if ! command -v podman >/dev/null 2>&1; then
    return 0
  fi

  if ! command -v systemctl >/dev/null 2>&1; then
    return 1
  fi

  if ! as_root true >/dev/null 2>&1; then
    warn_root_skip "systemd boot hook"
    return 0
  fi

  unit_tmp="$(mktemp)"

  cat > "${unit_tmp}" <<EOF
[Unit]
Description=Kotatsu backend containers
After=network-online.target
Wants=network-online.target

[Service]
Type=oneshot
RemainAfterExit=yes
ExecStart=${BOOT_SCRIPT} up ${APP_DIR}
ExecStop=${BOOT_SCRIPT} down ${APP_DIR}

[Install]
WantedBy=multi-user.target
EOF

  as_root install -m 0644 "${unit_tmp}" /etc/systemd/system/kotatsu-backend.service
  rm -f "${unit_tmp}"

  as_root systemctl daemon-reload
  as_root systemctl enable kotatsu-backend.service >/dev/null
}

main() {
  install_sysctl

  if install_openrc_boot_hook; then
    return 0
  fi

  install_systemd_boot_hook || true
}

main "$@"
