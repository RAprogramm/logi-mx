#!/usr/bin/env bash
set -euo pipefail

PREFIX="${PREFIX:-/usr/local}"
BIN_DIR="${PREFIX}/bin"
UDEV_RULES_DIR="${PREFIX}/lib/udev/rules.d"

BUILD="${BUILD:-release}"
ROOT_DIR="$(cd "$(dirname "$0")/.." && pwd)"

# Resolve the invoking user so user-scoped files land in the right home under sudo
if [ -n "${SUDO_USER:-}" ] && [ "${SUDO_USER}" != "root" ]; then
  TARGET_USER="${SUDO_USER}"
  TARGET_HOME="$(getent passwd "${SUDO_USER}" | cut -d: -f6)"
  TARGET_HOME="${TARGET_HOME:-${HOME}}"
else
  TARGET_USER="$(id -un)"
  TARGET_HOME="${HOME}"
fi
USER_BIN_DIR="${TARGET_HOME}/.local/bin"
SYSTEMD_USER_DIR="${TARGET_HOME}/.config/systemd/user"
APPLICATIONS_DIR="${TARGET_HOME}/.local/share/applications"
AUTOSTART_DIR="${TARGET_HOME}/.config/autostart"

run_user_systemctl() {
  if [ "$(id -u)" -eq 0 ] && [ "${TARGET_USER}" != "root" ]; then
    sudo -u "${TARGET_USER}" XDG_RUNTIME_DIR="/run/user/$(id -u "${TARGET_USER}")" systemctl --user "$@"
  else
    systemctl --user "$@"
  fi
}

echo "=== logi-mx installer ==="
echo "Building (${BUILD})..."
cargo build --manifest-path "${ROOT_DIR}/Cargo.toml" --"${BUILD}"

BIN_SRC="${ROOT_DIR}/target/${BUILD}"
install -d "${BIN_DIR}"
install -m 0755 "${BIN_SRC}/logi-mx" "${BIN_DIR}/logi-mx"
install -m 0755 "${BIN_SRC}/logi-mx-daemon" "${BIN_DIR}/logi-mx-daemon"
install -m 0755 "${BIN_SRC}/logi-mx-ui" "${BIN_DIR}/logi-mx-ui"
install -m 0755 "${BIN_SRC}/logi-mx-overlay" "${BIN_DIR}/logi-mx-overlay"

echo "Installing udev rules..."
if [ -w "${UDEV_RULES_DIR}" ] || [ "$(id -u)" -eq 0 ]; then
  install -d "${UDEV_RULES_DIR}"
  install -m 0644 "${ROOT_DIR}/90-logi-mx.rules" "${UDEV_RULES_DIR}/90-logi-mx.rules"
  udevadm control --reload-rules 2>/dev/null || true
  udevadm trigger 2>/dev/null || true
else
  echo "  (skipped: need root for ${UDEV_RULES_DIR})"
fi

echo "Installing modules-load.d config..."
MODULES_LOAD_DIR="${PREFIX}/lib/modules-load.d"
if [ -w "${MODULES_LOAD_DIR}" ] || [ "$(id -u)" -eq 0 ]; then
  install -d "${MODULES_LOAD_DIR}"
  install -m 0644 "${ROOT_DIR}/uinput.conf" "${MODULES_LOAD_DIR}/uinput.conf"
  modprobe uinput 2>/dev/null || true
else
  echo "  (skipped: need root for ${MODULES_LOAD_DIR})"
fi

DETECTED_DE="${XDG_CURRENT_DESKTOP:-${DESKTOP_SESSION:-}}"
HAS_DISPLAY=0
if [ -n "${WAYLAND_DISPLAY:-}" ] || [ -n "${DISPLAY:-}" ]; then
  HAS_DISPLAY=1
fi

echo "Detected desktop environment: ${DETECTED_DE:-none}"
if [ "${HAS_DISPLAY}" -eq 1 ]; then
  echo "Desktop detected: installing tray/UI integration."
fi

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

# systemd user units
if command -v systemctl >/dev/null 2>&1; then
  install -d "${USER_BIN_DIR}"
  install -m 0755 "${BIN_SRC}/logi-mx-daemon" "${USER_BIN_DIR}/logi-mx-daemon"
  install -m 0755 "${BIN_SRC}/logi-mx-overlay" "${USER_BIN_DIR}/logi-mx-overlay"
  install -d "${SYSTEMD_USER_DIR}"
  install -m 0644 "${ROOT_DIR}/logi-mx-daemon.service" "${SYSTEMD_USER_DIR}/logi-mx-daemon.service"
  install -m 0644 "${ROOT_DIR}/logi-mx-daemon.socket" "${SYSTEMD_USER_DIR}/logi-mx-daemon.socket"
  install -m 0644 "${ROOT_DIR}/overlay/logi-mx-overlay.service" "${SYSTEMD_USER_DIR}/logi-mx-overlay.service"
  # user units always run the binaries from the target user's ~/.local/bin
  sed -i "s#^ExecStart=.*#ExecStart=%h/.local/bin/logi-mx-daemon#" "${SYSTEMD_USER_DIR}/logi-mx-daemon.service"
  sed -i "s#^ExecStart=.*#ExecStart=%h/.local/bin/logi-mx-overlay#" "${SYSTEMD_USER_DIR}/logi-mx-overlay.service"
  run_user_systemctl daemon-reload 2>/dev/null || true
  if run_user_systemctl enable --now logi-mx-daemon.socket logi-mx-daemon.service 2>/dev/null; then
    echo "  systemd user service and socket installed and enabled"
  else
    echo "  warning: could not enable systemd user units. As ${TARGET_USER}, run: systemctl --user enable --now logi-mx-daemon.socket logi-mx-daemon.service"
  fi
  run_user_systemctl enable --now logi-mx-overlay.service 2>/dev/null || true
fi

# Hyprland integration (window rules + overlay keybind), offered once
if command -v hyprctl >/dev/null 2>&1 || [ -d "${TARGET_HOME}/.config/hypr" ]; then
  if [ -t 0 ]; then
    printf "Hyprland detected. Configure integration (window rules + overlay keybind)? [Y/n] "
    read -r answer
    case "${answer:-Y}" in
      [nN]*) echo "  skipped: run 'logi-mx-ui' and press Configure in Desktop Integration later" ;;
      *) "${BIN_DIR}/logi-mx-ui" --integrate-hyprland || true ;;
    esac
  else
    echo "Hyprland detected: run 'logi-mx-ui --integrate-hyprland' to configure it."
  fi
fi

echo "=== Installation complete ==="
echo "Run: ${BIN_DIR}/logi-mx info"
