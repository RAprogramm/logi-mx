# Installation Guide

## Unified Installer

```bash
# Build and install to /usr/local, enable autostart automatically
sudo ./scripts/install.sh

# Or install to ~/.local
PREFIX=~/.local ./scripts/install.sh
```

The installer detects the desktop environment and configures tray/UI autostart
when appropriate.

## Quick Install (Local Build)

```bash
# Build and install with makepkg
makepkg --skipdepcheck -si

# Enable and start daemon
systemctl --user enable --now logi-mx-daemon.socket logi-mx-daemon.service

# Enable and start the radial pie menu overlay
systemctl --user enable --now logi-mx-overlay.service

# Check status
systemctl --user status logi-mx-daemon
```

## Post-Install

After installation:
- Tray icon appears in system panel
- CLI commands available: `logi-mx info`, `logi-mx battery`
- GUI: `logi-mx-ui`
- Radial pie overlay: `logi-mx-overlay` (passive renderer, streams from the daemon)
- Config: `~/.config/logi-mx.toml`
- Logs: `journalctl --user -u logi-mx-daemon -f`

## Manual Installation

```bash
# Build release
cargo build --release

# Copy binaries
sudo install -Dm755 target/release/logi-mx /usr/bin/
sudo install -Dm755 target/release/logi-mx-daemon /usr/bin/
sudo install -Dm755 target/release/logi-mx-ui /usr/bin/
sudo install -Dm755 target/release/logi-mx-overlay /usr/bin/

# Install udev rules
sudo cp 90-logi-mx.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger

# Install systemd user units (the daemon runs from ~/.local/bin)
mkdir -p ~/.local/bin ~/.config/systemd/user
install -m 0755 target/release/logi-mx-daemon ~/.local/bin/
install -m 0755 target/release/logi-mx-overlay ~/.local/bin/
cp logi-mx-daemon.service ~/.config/systemd/user/
cp logi-mx-daemon.socket ~/.config/systemd/user/
cp overlay/logi-mx-overlay.service ~/.config/systemd/user/
sed -i "s#^ExecStart=.*#ExecStart=%h/.local/bin/logi-mx-daemon#" ~/.config/systemd/user/logi-mx-daemon.service
systemctl --user daemon-reload
systemctl --user enable --now logi-mx-daemon.socket logi-mx-daemon.service
systemctl --user enable --now logi-mx-overlay.service
```
