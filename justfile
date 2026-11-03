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
    ./scripts/install.sh

# Uninstall local installation
uninstall:
    rm -f ~/.local/bin/logi-mx ~/.local/bin/logi-mx-daemon ~/.local/bin/logi-mx-ui ~/.local/bin/logi-mx-overlay
    rm -f ~/.config/systemd/user/logi-mx-daemon.service ~/.config/systemd/user/logi-mx-overlay.service
    systemctl --user daemon-reload || true
    echo "Uninstalled"

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
