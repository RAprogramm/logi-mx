# Justfile for logi-mx
# https://just.systems

set shell := ["bash", "-cu"]

# Build all crates (debug)
build:
    cargo build --workspace

# Build all crates (release)
build-release:
    cargo build --workspace --release

# Run all tests
test:
    cargo test --workspace

# Run clippy
clippy:
    cargo clippy --workspace --all-targets

# Run cargo deny
deny:
    cargo deny check

# Full local verification
check: build test clippy deny
    @echo "All checks passed"

# Install locally (release)
install: build-release
    PREFIX="${PREFIX:-$HOME/.local}" ./scripts/install.sh

# Uninstall local installation
uninstall:
    systemctl --user disable --now logi-mx-daemon.service logi-mx-daemon.socket logi-mx-overlay.service || true
    rm -f ~/.local/bin/logi-mx ~/.local/bin/logi-mx-daemon ~/.local/bin/logi-mx-ui ~/.local/bin/logi-mx-overlay
    rm -f ~/.config/systemd/user/logi-mx-daemon.service ~/.config/systemd/user/logi-mx-daemon.socket ~/.config/systemd/user/logi-mx-overlay.service
    rm -f ~/.config/autostart/logi-mx-daemon.desktop ~/.local/share/applications/logi-mx-ui.desktop
    systemctl --user daemon-reload || true
    echo "Uninstalled user installation (for a system-wide install also run: sudo rm -f /usr/local/bin/logi-mx* /usr/local/lib/udev/rules.d/90-logi-mx.rules /usr/local/lib/modules-load.d/uinput.conf)"

# Run daemon in foreground
run-daemon:
    cargo run -p logi-mx-daemon

# Run UI
run-ui:
    cargo run -p logi-mx-ui

# Run CLI help
run-cli:
    cargo run -p logi-mx -- --help

# Show daemon status
status:
    cargo run -p logi-mx -- daemon status

# Clean build artifacts
clean:
    cargo clean
