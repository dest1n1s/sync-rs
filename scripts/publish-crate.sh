#!/bin/bash
set -e

# Bump version if needed (use cargo-edit)
cargo install cargo-edit 2>/dev/null || true
cargo set-version $1

# Publish to crates.io
cargo publish

# Tag release
git tag v$(cargo metadata --format-version 1 | jq -r '.packages[] | select(.name=="sync-rs") | .version')
git push origin --tags