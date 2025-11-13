# logi-mx

[![CI](https://github.com/RAprogramm/logi-mx/workflows/CI/badge.svg)](https://github.com/RAprogramm/logi-mx/actions/workflows/ci.yml)
[![codecov](https://codecov.io/gh/RAprogramm/logi-mx/branch/main/graph/badge.svg)](https://codecov.io/gh/RAprogramm/logi-mx)
[![AUR version](https://img.shields.io/aur/version/logi-mx)](https://aur.archlinux.org/packages/logi-mx)
[![AUR votes](https://img.shields.io/aur/votes/logi-mx)](https://aur.archlinux.org/packages/logi-mx)
[![License: MIT](https://img.shields.io/badge/License-MIT-blue.svg)](https://opensource.org/licenses/MIT)
[![Rust Version](https://img.shields.io/badge/rust-1.91%2B-orange.svg)](https://www.rust-lang.org)

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

## Features

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
- DPI configuration (100-8000)
- SmartShift tuning
- Hi-res scroll control
- Button remapping
- Gesture support
- Battery monitoring

**Supported Devices**
- Logitech MX Master 3S (USB, Bluetooth, Bolt receiver)
- MX Master 3S for Business

## Architecture

```
logi-mx/
├── driver/     # Core HID++ protocol library
├── daemon/     # Background service
├── cli/        # Command-line interface
└── ui/         # GTK4/libadwaita GUI
```

## Installation

### Arch Linux (Recommended)

```bash
# From AUR
paru -S logi-mx
# or
yay -S logi-mx

# Enable and start daemon
systemctl --user enable --now logi-mx-daemon
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

# Install systemd service
mkdir -p ~/.config/systemd/user
curl -o ~/.config/systemd/user/logi-mx-daemon.service \
  https://raw.githubusercontent.com/RAprogramm/logi-mx/main/logi-mx-daemon.service
systemctl --user enable --now logi-mx-daemon
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

# Setup (same as above)
sudo cp 90-logi-mx.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules && sudo udevadm trigger
mkdir -p ~/.config/systemd/user
cp logi-mx-daemon.service ~/.config/systemd/user/
systemctl --user enable --now logi-mx-daemon
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

For Hyprland users, add these window rules to `~/.config/hypr/hyprland.conf` for optimal UI experience:

```
# Logitech MX Master 3S Configuration Window
windowrulev2 = float, title:(Logitech MX Master 3S)
windowrulev2 = center, title:(Logitech MX Master 3S)
```

Reload Hyprland config:
```bash
hyprctl reload
```

## Usage

### CLI

```bash
# Get device info
logi-mx info

# Set DPI
logi-mx set dpi 1600

# Configure SmartShift
logi-mx set smartshift --enabled --threshold 20

# Enable hi-res scroll
logi-mx set scroll --hires

# Get battery status
logi-mx battery
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
enabled = true
inverted = false

[devices.buttons.ThumbGesture]
Gestures = [
    { direction = "Up", mode = "OnRelease", action = { Keypress = { keys = ["KEY_UP"] } } },
    { direction = "Down", mode = "OnRelease", action = { Keypress = { keys = ["KEY_DOWN"] } } },
    { direction = "Left", mode = "OnRelease", action = { Keypress = { keys = ["KEY_LEFTCTRL", "KEY_LEFT"] } } },
    { direction = "Right", mode = "OnRelease", action = { Keypress = { keys = ["KEY_LEFTCTRL", "KEY_RIGHT"] } } },
]
```

## HID++ Protocol

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
| 0x2110 | SmartShift | Ratchet/free-spin control |
| 0x2121 | Hi-Res Wheel | High-resolution scrolling |

## Acknowledgments

- Logitech for HID++ protocol documentation
- Solaar project for protocol insights
- logiops for feature reference
