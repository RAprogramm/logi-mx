#!/usr/bin/env bash
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
BIN_DIR="${PREFIX}/bin"
SYSTEMD_USER_DIR="${HOME}/.config/systemd/user"
UDEV_RULES_DIR="${PREFIX}/lib/udev/rules.d"
APPLICATIONS_DIR="${HOME}/.local/share/applications"
AUTOSTART_DIR="${HOME}/.config/autostart"

BUILD="${BUILD:-release}"
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

echo "=== logi-mx installer ==="
echo "Building (${BUILD})..."
cargo build --manifest-path "${ROOT_DIR}/Cargo.toml" --${BUILD}

BIN_SRC="${ROOT_DIR}/target/${BUILD}"
install -d "${BIN_DIR}"
install -m 0755 "${BIN_SRC}/logi-mx" "${BIN_DIR}/logi-mx"
install -m 0755 "${BIN_SRC}/logi-mx-daemon" "${BIN_DIR}/logi-mx-daemon"
install -m 0755 "${BIN_SRC}/logi-mx-ui" "${BIN_DIR}/logi-mx-ui"

echo "Installing udev rules..."
if [ -w "${UDEV_RULES_DIR}" ] || [ "$(id -u)" -eq 0 ]; then
  install -d "${UDEV_RULES_DIR}"
  install -m 0644 "${ROOT_DIR}/90-logi-mx.rules" "${UDEV_RULES_DIR}/90-logi-mx.rules"
  udevadm control --reload-rules 2>/dev/null || true
  udevadm trigger 2>/dev/null || true
else
  echo "  (skipped: need root for ${UDEV_RULES_DIR})"
fi

DETECTED_DE="${XDG_CURRENT_DESKTOP:-${DESKTOP_SESSION:-}}"
HAS_DE=0
if [ -n "${WAYLAND_DISPLAY:-}" ] || [ -n "${DISPLAY:-}" ]; then
  HAS_DE=1
fi

echo "Detected desktop environment: ${DETECTED_DE:-none}"

if [ "${HAS_DE}" -eq 1 ]; then
  echo "Desktop detected: installing tray/UI integration."

  install -d "${APPLICATIONS_DIR}"
  cat > "${APPLICATIONS_DIR}/logi-mx-ui.desktop" <<DESKTOP
[Desktop Entry]
Name=Logitech MX Configuration
Exec=${BIN_DIR}/logi-mx-ui
Type=Application
Icon=input-mouse
Categories=Settings;HardwareSettings;
DESKTOP

  # Autostart daemon (enabled by default for professional UX)
  if [ "${AUTO_START:-yes}" != "no" ]; then
    install -d "${AUTOSTART_DIR}"
    cat > "${AUTOSTART_DIR}/logi-mx-daemon.desktop" <<DESKTOP
[Desktop Entry]
Name=Logitech MX Daemon
Exec=${BIN_DIR}/logi-mx-daemon
Type=Application
X-GNOME-Autostart-enabled=true
DESKTOP
    echo "  autostart enabled"
  fi
else
  echo "No desktop detected: installing daemon service only."
fi

# systemd user units
if command -v systemctl >/dev/null 2>&1; then
  install -d "${SYSTEMD_USER_DIR}"
  install -m 0644 "${ROOT_DIR}/logi-mx-daemon.service" "${SYSTEMD_USER_DIR}/logi-mx-daemon.service"
  install -m 0644 "${ROOT_DIR}/logi-mx-daemon.socket" "${SYSTEMD_USER_DIR}/logi-mx-daemon.socket"
  # fix ExecStart path
  sed -i "s#^ExecStart=.*#ExecStart=${BIN_DIR}/logi-mx-daemon#" "${SYSTEMD_USER_DIR}/logi-mx-daemon.service"
  systemctl --user daemon-reload 2>/dev/null || true
  systemctl --user enable --now logi-mx-daemon.socket logi-mx-daemon.service 2>/dev/null || true
  echo "  systemd user service and socket installed and enabled"
fi

echo "=== Installation complete ==="
echo "Run: ${BIN_DIR}/logi-mx info"
