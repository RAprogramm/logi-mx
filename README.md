# logi-mx

[![CI](https://github.com/RAprogramm/logi-mx/workflows/CI/badge.svg)](https://github.com/RAprogramm/logi-mx/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/RAprogramm/logi-mx/branch/main/graph/badge.svg)](https://codecov.io/gh/RAprogramm/logi-mx)
[![CodSpeed Badge](https://img.shields.io/endpoint?url=https://app.codspeed.io//badge.json)](https://app.codspeed.io//RAprogramm/logi-mx?utm_source=badge)
[![AUR version](https://img.shields.io/aur/version/logi-mx)](https://aur.archlinux.org/packages/logi-mx)
[![AUR votes](https://img.shields.io/aur/votes/logi-mx)](https://aur.archlinux.org/packages/logi-mx)
[![AUR version (bin)](https://img.shields.io/aur/version/logi-mx-bin)](https://aur.archlinux.org/packages/logi-mx-bin)
[![AUR votes (bin)](https://img.shields.io/aur/votes/logi-mx-bin)](https://aur.archlinux.org/packages/logi-mx-bin)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![REUSE](https://api.reuse.software/badge/github.com/RAprogramm/logi-mx)](https://api.reuse.software/info/github.com/RAprogramm/logi-mx)
[![Rust Version](https://img.shields.io/badge/rust-1.99%2B-orange.svg)](https://www.rust-lang.org)

**Blazing fast Logitech MX series mouse driver and configuration tool for Linux**

<details>
<summary>📊 Code Coverage Graphs</summary>

### Sunburst
The inner-most circle is the entire project, moving away from the center are folders then, finally, a single file. The size and color of each slice is representing the number of statements and the coverage, respectively.

![Sunburst](https://codecov.io/github/RAprogramm/logi-mx/graphs/sunburst.svg?token=QMBZCQZJxN)

### Grid
Each block represents a single file in the project. The size and color of each block is represented by the number of statements and the coverage, respectively.

![Grid](https://codecov.io/github/RAprogramm/logi-mx/graphs/tree.svg?token=QMBZCQZJxN)

### Icicle
The top section represents the entire project. Proceeding with folders and finally individual files. The size and color of each slice is representing the number of statements and the coverage, respectively.

![Icicle](https://codecov.io/github/RAprogramm/logi-mx/graphs/icicle.svg?token=QMBZCQZJxN)

</details>


Professional-grade, production-ready HID++ driver written in pure Rust with zero-cost abstractions.

## MX Master 3S Hardware Overview

<details>
<summary><b>Buttons and Controls</b></summary>

The MX Master 3S features 7 programmable buttons and 2 scroll wheels:

**Primary Buttons:**
- Left Click
- Right Click
- Middle Click (scroll wheel press)

**Navigation Buttons:**
- Forward Button (thumb area)
- Back Button (thumb area)

**Special Function Buttons:**
- Gesture Button (thumb area) - Enables gesture-based navigation
- Mode-Shift Button (behind scroll wheel) - Switches scroll wheel modes
- Easy-Switch Button (bottom) - Multi-device connection switching

**Scroll Wheels:**
- **Main Scroll Wheel** - MagSpeed electromagnetic scrolling
  - Supports ratchet mode (line-by-line) and free-spin mode
  - SmartShift automatic mode switching based on scroll speed
  - Hi-res scrolling (up to 1000 lines per second)
  - Horizontal tilt capability

- **Thumb Wheel** (side) - Secondary scroll control
  - Horizontal scrolling by default
  - Customizable for volume, brightness, or other functions
  - Tactile feedback with precise control

</details>

<details>
<summary><b>Sensor Specifications</b></summary>

**DPI Range:** 200-8000 in 50 DPI increments
- Default: 1000 DPI
- Configurable up to 8000 DPI for high-precision work
- 8K DPI optical sensor with tracking on glass surfaces

</details>

<details>
<summary><b>Gesture System</b></summary>

The Gesture Button enables directional gestures:
- Gesture Button click - Toggle the HUD overlay (default)
- Up gesture - Configurable action
- Down gesture - Configurable action
- Left gesture - Configurable action (default: browser back)
- Right gesture - Configurable action (default: browser forward)
- Diagonal gestures - Advanced customization

Each gesture can trigger:
- Keyboard shortcuts
- Application switching
- Desktop navigation
- Custom key combinations

</details>

<details>
<summary><b>SmartShift Wheel Technology</b></summary>

Automatic ratchet-to-free-spin transition:
- **Ratchet Mode**: Precise line-by-line scrolling for documents
- **Free-Spin Mode**: Fast navigation through long pages
- **Mode-Shift Button**: Instantly toggles between ratchet and free-spin (default binding)
- **Auto-disengage threshold**: 1-50; higher values need more scroll force to switch to free-spin mode
  - Lower values: Easier transition to free-spin
  - Higher values: More force required for free-spin

</details>

<details>
<summary><b>Power and Connectivity</b></summary>

**Battery:**
- Up to 70 days on full charge
- Quick charge: 3 hours of use from 1-minute charge
- USB-C charging port

**Connection Options:**
- Logi Bolt USB receiver
- Bluetooth Low Energy 5.0+
- Multi-device support (up to 3 devices)

</details>

## Features

<details>
<summary>Click to expand</summary>

**High Performance**
- Zero-cost abstractions
- Async I/O with tokio
- Minimal memory allocations

**Professional Quality**
- HID++ 2.0 protocol implementation
- Comprehensive error handling with masterror
- 95%+ test coverage
- Enterprise-grade reliability

**Rich Functionality**
- DPI configuration (200-8000 in 50 DPI increments)
- SmartShift auto-disengage tuning (threshold 1-50)
- Hi-res scroll control
- Battery monitoring with charge level and level bar
- Easy-Switch host switching (`logi-mx set host <index>`)
- Button divert control (`logi-mx set button <id> --divert`)
- Gesture support (8 directions with per-application overrides)
- Radial pie menu on the diverted gesture button: hold to charge a ring that
  blooms into 2–8 equal wedges at the cursor — one per installed slot, the
  first at twelve o'clock; point, swipe or scroll to select, release to fire;
  a swipe that commits before the ring opens is an expert mark that fires the
  swipe binding instead; quick tap keeps the click binding, double click
  opens the menu at once; per-application rings let a game carry its own wheel
- Button remapping stored in configuration (wire-level reprogramming in development)
- Daemon with system tray integration and auto-refresh
- Desktop notifications (freedesktop D-Bus): connect/disconnect, low and
  critical battery, Easy-Switch host changes
- GTK4/libadwaita GUI with modern ToolbarView layout
- Automatic device discovery on Bolt receiver slots
- Unified installer with DE detection and autostart

**Supported Devices**
- Logitech MX Master 3S (USB, Bluetooth, Bolt receiver)
- MX Master 3S for Business

### Current Implementation Status

**Implemented:**
- DPI adjustment (200-8000)
- SmartShift configuration
- Hi-res scroll enable/disable (inversion applied on hardware)
- Battery status monitoring
- Daemon with udev hotplug and automatic config application
- GTK4/libadwaita GUI
- Automatic device discovery (no hardcoded receiver slot)
- Software gesture recognition with a Wayland overlay (charge ring, pie menu,
  highlight animations)
- Radial pie menu with per-slot commands, keybind picks and per-application
  rings, configured from the HUD Pie page
- Per-application profiles for gestures, buttons and pie rings

**Configured but not yet applied on the wire:**
- Button remapping (stored in `~/.config/logi-mx.toml`; HID++
  ReprogControls reprogramming is in development)
- Hi-res scroll inversion

**In Development:**
- Wire-level button reprogramming (HID++ 0x1B04)
- Mode-shift button configuration
- Macro recording and playback

**Planned:**
- Diagonal gesture support
- Smart Actions (multi-step workflows)
- Profile switching per workspace
- Cloud profile synchronization

</details>

## Native vs Daemon Mode

The mouse can operate in two modes:

### Native Mode (Default Linux Drivers)

When the daemon is stopped, your mouse uses the default Linux HID drivers:
- Basic mouse movement and clicks work normally
- Scroll wheel functions at default speed
- No configuration or customization available

### Daemon Mode (logi-mx)

When the daemon is running, you get full control:

| Feature | Native Mode | Daemon Mode |
|---------|-------------|-------------|
| Basic mouse movement | ✅ | ✅ |
| Button clicks | ✅ | ✅ |
| Scroll wheel | ✅ | ✅ |
| Custom DPI | ❌ | ✅ |
| SmartShift | ❌ | ✅ |
| Hi-res scrolling | ❌ | ✅ |
| Button remapping | ❌ | 🚧 (config stored, wire reprogramming in development) |
| Battery monitoring | ❌ | ✅ |
| Per-app settings | ❌ | 🚧 (planned) |

**Starting the daemon:**
```bash
systemctl --user start logi-mx-daemon.service
```

**Stopping the daemon:**
```bash
systemctl --user stop logi-mx-daemon.service
```

When you stop the daemon, all custom settings are reset and the mouse reverts to standard Linux behavior.

## Architecture

<details>
<summary>Click to expand</summary>

```
logi-mx/
├── driver/     # Core HID++ protocol library
├── daemon/     # Background service
├── cli/        # Command-line interface
└── ui/         # GTK4/libadwaita GUI
```

</details>

## Installation

<details open>
<summary>Click to expand</summary>

### Arch Linux (Recommended)

```bash
# From AUR
paru -S logi-mx
# or
yay -S logi-mx

# Add your user to input group (required for scroll speed multiplier)
sudo usermod -aG input $USER

# Enable and start daemon
systemctl --user enable --now logi-mx-daemon.socket logi-mx-daemon.service

# Logout and login for group changes to take effect
```

### From crates.io

```bash
# Install Rust toolchain if needed
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install binaries
cargo install logi-mx --locked

# Setup udev rules
sudo curl -o /etc/udev/rules.d/90-logi-mx.rules \
  https://raw.githubusercontent.com/RAprogramm/logi-mx/main/90-logi-mx.rules
sudo udevadm control --reload-rules && sudo udevadm trigger

# Load uinput at boot (required for gesture key injection)
sudo curl -o /etc/modules-load.d/uinput.conf \
  https://raw.githubusercontent.com/RAprogramm/logi-mx/main/uinput.conf
sudo modprobe uinput

# Add your user to input group (required for live telemetry and gesture injection)
sudo usermod -aG input $USER

# Install systemd user units (the daemon runs from ~/.local/bin)
mkdir -p ~/.local/bin ~/.config/systemd/user
install -m 0755 ~/.cargo/bin/logi-mx-daemon ~/.local/bin/
curl -o ~/.config/systemd/user/logi-mx-daemon.service \
  https://raw.githubusercontent.com/RAprogramm/logi-mx/main/logi-mx-daemon.service
curl -o ~/.config/systemd/user/logi-mx-daemon.socket \
  https://raw.githubusercontent.com/RAprogramm/logi-mx/main/logi-mx-daemon.socket
sed -i "s#^ExecStart=.*#ExecStart=%h/.local/bin/logi-mx-daemon#" ~/.config/systemd/user/logi-mx-daemon.service
systemctl --user enable --now logi-mx-daemon.socket logi-mx-daemon.service

# Logout and login for group changes to take effect
```

### From Source

```bash
git clone https://github.com/RAprogramm/logi-mx
cd logi-mx

# Build release
cargo build --release

# Install binaries
cargo install --path cli --locked
cargo install --path daemon --locked
cargo install --path ui --locked

# Setup udev rules and permissions
sudo cp 90-logi-mx.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger

# Load uinput at boot (required for gesture key injection)
sudo cp uinput.conf /etc/modules-load.d/
sudo modprobe uinput

# Add your user to input group (required for live telemetry and gesture injection)
sudo usermod -aG input $USER

# Install systemd user units (the daemon runs from ~/.local/bin)
mkdir -p ~/.local/bin ~/.config/systemd/user
install -m 0755 ~/.cargo/bin/logi-mx-daemon ~/.local/bin/
cp logi-mx-daemon.service ~/.config/systemd/user/
cp logi-mx-daemon.socket ~/.config/systemd/user/
sed -i "s#^ExecStart=.*#ExecStart=%h/.local/bin/logi-mx-daemon#" ~/.config/systemd/user/logi-mx-daemon.service
systemctl --user enable --now logi-mx-daemon.socket logi-mx-daemon.service

# Logout and login for group changes to take effect
```

### Unified Installer

The installer detects the desktop environment and configures the appropriate
components automatically:

```bash
# Build and install to /usr/local (requires sudo for udev rules)
sudo ./scripts/install.sh

# Or install to a custom prefix
PREFIX=~/.local ./scripts/install.sh

# Disable autostart prompt
AUTO_START=no ./scripts/install.sh
```

It installs binaries, udev rules, systemd user service, and (when a DE is
detected) tray/UI autostart entries.

### Justfile

For local development and maintenance:

```bash
just build
just test
just clippy
just deny
just check
just install
just run-daemon
just run-ui
```

### Dependencies

**Arch Linux:**
```bash
sudo pacman -S rust hidapi systemd gtk4 libadwaita dbus
```

**Ubuntu/Debian:**
```bash
sudo apt install cargo libhidapi-dev libudev-dev libgtk-4-dev libadwaita-1-dev libdbus-1-dev
```

### Hyprland Configuration

The easiest way: open the settings window → **Desktop Integration → Configure**.
The app detects Hyprland, appends the window rules and the overlay keybind to
`~/.config/hypr/hyprland.lua` (or legacy `hyprland.conf`) if missing, and
reloads Hyprland. Running it again is safe: it reports "Already configured".
The same action is available headless for scripts and installers:

```bash
logi-mx-ui --integrate-hyprland
```

Manual setup — Hyprland 0.55+ uses Lua configuration
(`~/.config/hypr/hyprland.lua`). Add these window rules for optimal UI
experience:

```lua
-- Logitech MX Master 3S Configuration Window
hl.window_rule({
    name  = "logi-mx-ui",
    match = { title = "^Logitech MX Master 3S$" },
    float = true,
    center = true,
})
```

Optional: bind the full-screen button-mapping overlay to a key:

```lua
hl.bind("SUPER + M", hl.dsp.exec_cmd("LOGI_MX_OVERLAY=1 logi-mx-ui"))
```

`exec_cmd` runs the command outside the compositor event loop, which is the
recommended pattern for external tools in Hyprland 0.57.

Desktop notifications from the daemon use the standard
`org.freedesktop.Notifications` D-Bus interface, so they work with any
notification server (dunst, mako, hydrus) without extra Hyprland setup.

If you still use the legacy `hyprland.conf` format:

```
windowrulev2 = float, title:(Logitech MX Master 3S)
windowrulev2 = center, title:(Logitech MX Master 3S)
```

Reload Hyprland config:
```bash
hyprctl reload
```

</details>

## Usage

<details>
<summary>Click to expand</summary>

### CLI

```bash
# Get device info
logi-mx info

# Set DPI
logi-mx set dpi 1600

# Configure SmartShift
logi-mx set smartshift --enabled --threshold 20

# Enable hi-res scroll
logi-mx set hires --enabled

# Get battery status
logi-mx battery

# Check daemon status
logi-mx daemon status
logi-mx daemon ping

# Enable autostart
logi-mx autostart install
logi-mx autostart status
logi-mx autostart uninstall

# Switch Easy-Switch host
logi-mx set host 1

# Divert a control to HID++ notifications
logi-mx set button 0x00C3 --divert
```

### Desktop Notifications

The daemon sends freedesktop desktop notifications (D-Bus
`org.freedesktop.Notifications`) for:

| Event | Urgency |
|-------|---------|
| Mouse connected / disconnected | normal |
| Battery at or below 20% | low |
| Battery at or below 10% | critical |
| Easy-Switch host changed | normal |

Battery alerts use hysteresis: they fire once per drop, only while
discharging, and re-arm after charging above the threshold plus 5%.
Works with any notification server (dunst, mako, GNOME Shell, KDE Plasma,
hydrus). Disable by building the daemon without the default `notify`
feature: `cargo build -p logi-mx-daemon --no-default-features --features tray`.

### UI and Overlay

```bash
# Settings window (Device / Pointer / Wheel tabs)
logi-mx-ui

# Full-screen button-mapping overlay
LOGI_MX_OVERLAY=1 logi-mx-ui
```

### Configuration File

Location: `~/.config/logi-mx.toml`

```toml
[[devices]]
name = "MX Master 3S"
dpi = 1000

[devices.smartshift]
enabled = true
threshold = 20

[devices.hiresscroll]
enabled = false
inverted = false

[devices.buttons.ThumbGesture]
Gestures = [
    { direction = "Up", mode = "OnRelease", action = { Keypress = { keys = ["KEY_UP"] } } },
    { direction = "Down", mode = "OnRelease", action = { Keypress = { keys = ["KEY_DOWN"] } } },
    { direction = "Left", mode = "OnRelease", action = { Keypress = { keys = ["KEY_LEFTCTRL", "KEY_LEFT"] } } },
    { direction = "Right", mode = "OnRelease", action = { Keypress = { keys = ["KEY_LEFTCTRL", "KEY_RIGHT"] } } },
]
```

> **Note:** button and gesture entries are loaded and stored by the daemon,
> but wire-level reprogramming (HID++ 0x1B04) is still in development.

</details>

## HID++ Protocol

<details>
<summary>Click to expand</summary>

This driver implements the Logitech HID++ 2.0 protocol:

- **Packet Types**: Short (7 bytes), Long (20 bytes)
- **Feature Discovery**: Dynamic feature table querying
- **Error Handling**: Comprehensive error mapping with retry logic
- **Device Communication**: Async I/O with timeout support

### Key Features Implemented

| Feature ID | Name | Description |
|------------|------|-------------|
| 0x0000 | Root | Protocol version, feature discovery |
| 0x0005 | Device Name | Get device name |
| 0x1000 | Battery Status | Legacy battery info |
| 0x1004 | Unified Battery | Modern battery interface |
| 0x2201 | Adjustable DPI | Sensor DPI control |
| 0x2110 | SmartShift | Ratchet/free-spin control (legacy) |
| 0x2111 | SmartShift Enhanced | MagSpeed wheel control (MX Master 3/3S/4) |
| 0x2121 | Hi-Res Wheel | High-resolution scrolling |

</details>

## Benchmarks

Performance is tracked continuously with
[CodSpeed](https://app.codspeed.io/RAprogramm/logi-mx?utm_source=badge): CPU
instruction counts and heap allocations are measured on every pull request and
on `main`, with a regression report on each PR. Allocation budgets run as
deterministic tests. See [BENCHMARKS.md](BENCHMARKS.md) for local runs,
comparison workflow and CI policy.

## Acknowledgments

<details>
<summary>Click to expand</summary>

- Logitech for HID++ protocol documentation
- Solaar project for protocol insights
- logiops for feature reference

</details>

