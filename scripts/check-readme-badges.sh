#!/bin/bash
set -euo pipefail

# Verify the static README badges never drift from the workspace manifest,
# the single source of truth for license and MSRV. The dynamic badges (CI,
# codecov, AUR) resolve their values at render time and need no sync.

MANIFEST="${1:-Cargo.toml}"
README="${2:-README.md}"

rust_version=$(grep '^rust-version' "$MANIFEST" | head -1 | sed 's/.*"\(.*\)".*/\1/')
license=$(grep '^license' "$MANIFEST" | head -1 | sed 's/.*"\(.*\)".*/\1/')

if [[ -z "$rust_version" ]] || [[ -z "$license" ]]; then
    echo "error: could not read rust-version/license from ${MANIFEST}"
    exit 1
fi

fail=0

if ! grep -qF "rust-${rust_version}%2B" "$README"; then
    echo "error: ${README} has no rust-${rust_version}+ badge, but ${MANIFEST} sets rust-version = \"${rust_version}\""
    fail=1
fi

if ! grep -qF "License-${license}-" "$README"; then
    echo "error: ${README} has no License: ${license} badge, but ${MANIFEST} sets license = \"${license}\""
    fail=1
fi

if [[ "$fail" -ne 0 ]]; then
    echo "Fix the badge block in ${README} to match ${MANIFEST}."
    exit 1
fi

echo "README badges match ${MANIFEST} (rust ${rust_version}, license ${license})."
