#!/bin/bash
set -e

if [ -z "$1" ]; then
  echo "Usage: ./release.sh <version>"
  exit 1
fi

# Validate tests
cargo test --all-features

# Create tag
git tag -a v$1 -m "Release v$1"
git push origin v$1

# Publish to crates.io
./scripts/publish-crate.sh $1

echo "Release v$1 completed successfully!"