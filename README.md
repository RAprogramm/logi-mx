# logi-mx

**Blazing fast Logitech MX series mouse driver and configuration tool for Linux**

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
sudo pacman -S rust hidapi systemd gtk4 libadwaita
```

**Ubuntu/Debian:**
```bash
sudo apt install cargo libhidapi-dev libudev-dev libgtk-4-dev libadwaita-1-dev
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

### Daemon

```bash
# Start daemon
systemctl --user start logi-mx-daemon

# Enable on boot
systemctl --user enable logi-mx-daemon

# Check status
systemctl --user status logi-mx-daemon
```

### GUI

```bash
# Launch GUI application
logi-mx-ui
```

## Development

### Build

```bash
# Debug build
cargo build

# Release build with optimizations
cargo build --release

# Build specific package
cargo build -p logi-mx-driver
```

### Test

```bash
# Run all tests
cargo test --workspace

# Run with coverage
cargo tarpaulin --all-features --out Html

# Run specific tests
cargo test -p logi-mx-driver
```

### Format

```bash
# Format code (requires nightly)
cargo +nightly fmt

# Check formatting
cargo +nightly fmt -- --check
```

### Lint

```bash
# Run clippy
cargo clippy --all-targets --all-features -- -D warnings
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

## Technical Specifications

- **Edition**: Rust 2024
- **MSRV**: 1.91
- **Profile**: LTO enabled, single codegen unit
- **Max Line**: 99 characters
- **Import Style**: StdExternalCrate grouping

## License

MIT License - see LICENSE file for details

## Contributing

Contributions are welcome! Please ensure:

- Code follows rustfmt configuration
- All tests pass
- Clippy shows no warnings
- Test coverage remains ≥95%

## Author

RAprogramm <andrey.rozanov.vl@gmail.com>

## Acknowledgments

- Logitech for HID++ protocol documentation
- Solaar project for protocol insights
- logiops for feature reference
