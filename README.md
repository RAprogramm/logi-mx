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
- **Threshold**: Adjustable sensitivity (0-50)
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
- SmartShift tuning (threshold 0-50)
- Hi-res scroll control with inversion
- Button remapping for all 7 buttons
- Gesture support (4 directions: up, down, left, right)
- Battery monitoring with charge level
- Main scroll wheel configuration
- Thumb wheel horizontal scroll

**Supported Devices**
- Logitech MX Master 3S (USB, Bluetooth, Bolt receiver)
- MX Master 3S for Business

### Current Implementation Status

**Implemented:**
- DPI adjustment (200-8000)
- SmartShift configuration
- Hi-res scroll with inversion
- Basic gesture support (up/down/left/right)
- Battery status monitoring
- Button remapping via configuration
- Daemon with system tray integration
- GTK4/libadwaita GUI
- Scroll wheel speed configuration (lines per click)
- Thumb wheel speed configuration

**In Development:**
- Enhanced gesture system with visual feedback
- Mode-shift button configuration
- Per-application profiles
- Macro recording and playback
- Advanced button actions
- UI gesture configuration interface

**Planned:**
- Diagonal gesture support
- Gesture animations and visual indicators
- Smart Actions (multi-step workflows)
- Application-specific button mappings
- Profile switching per workspace
- Cloud profile synchronization

</details>

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
systemctl --user enable --now logi-mx-daemon

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

# Add your user to input group (required for scroll speed multiplier)
sudo usermod -aG input $USER

# Install systemd service
mkdir -p ~/.config/systemd/user
curl -o ~/.config/systemd/user/logi-mx-daemon.service \
  https://raw.githubusercontent.com/RAprogramm/logi-mx/main/logi-mx-daemon.service
systemctl --user enable --now logi-mx-daemon

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

# Add your user to input group (required for scroll speed multiplier)
sudo usermod -aG input $USER

# Install systemd service
mkdir -p ~/.config/systemd/user
cp logi-mx-daemon.service ~/.config/systemd/user/
systemctl --user enable --now logi-mx-daemon

# Logout and login for group changes to take effect
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
logi-mx set scroll --hires

# Configure scroll wheel speed
logi-mx set scroll-wheel --vertical-speed 5 --horizontal-speed 3 --smooth

# Configure thumb wheel speed
logi-mx set thumb-wheel --speed 7 --smooth

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

[devices.scroll_wheel]
vertical_speed = 3
horizontal_speed = 2
smooth_scrolling = false

[devices.thumbwheel]
speed = 5
smooth_scrolling = true

[devices.buttons.ThumbGesture]
Gestures = [
    { direction = "Up", mode = "OnRelease", action = { Keypress = { keys = ["KEY_UP"] } } },
    { direction = "Down", mode = "OnRelease", action = { Keypress = { keys = ["KEY_DOWN"] } } },
    { direction = "Left", mode = "OnRelease", action = { Keypress = { keys = ["KEY_LEFTCTRL", "KEY_LEFT"] } } },
    { direction = "Right", mode = "OnRelease", action = { Keypress = { keys = ["KEY_LEFTCTRL", "KEY_RIGHT"] } } },
]
```

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
| 0x2110 | SmartShift | Ratchet/free-spin control |
| 0x2121 | Hi-Res Wheel | High-resolution scrolling |

</details>

## Acknowledgments

<details>
<summary>Click to expand</summary>

- Logitech for HID++ protocol documentation
- Solaar project for protocol insights
- logiops for feature reference

</details>
