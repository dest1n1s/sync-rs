#!/bin/bash
set -e

if [ -z "$1" ]; then
  echo "Usage: ./release.sh <version>"
  exit 1
fi

# Validate tests
cargo test --all-features

# Bump version if needed (use cargo-edit)
cargo install cargo-edit 2>/dev/null || true
cargo set-version $1

# Publish to crates.io
cargo publish

# Tag release
git tag v$(cargo metadata --format-version 1 | jq -r '.packages[] | select(.name=="sync-rs") | .version') -m "Release v$1"
git add .
git commit -m "chore: Release v$1"
git push origin --tags

echo "Release v$1 completed successfully!"