#!/bin/bash
set -e

# Check arguments
if [ -z "$1" ]; then
  echo "Usage: ./update-homebrew.sh <version>"
  exit 1
fi

VERSION=$1
WORKDIR=$(mktemp -d)
BREW_TAP_REPO="https://github.com/Dest1n1s/homebrew-tap.git"
TARBALL_URL="https://github.com/Dest1n1s/sync-rs/archive/v${VERSION}.tar.gz"

echo "Preparing Homebrew formula for sync-rs v${VERSION}..."

# Download the source tarball to calculate its SHA256
echo "Downloading source tarball..."
curl -sL "${TARBALL_URL}" -o "${WORKDIR}/sync-rs-${VERSION}.tar.gz"
SHA256=$(sha256sum "${WORKDIR}/sync-rs-${VERSION}.tar.gz" | cut -d' ' -f1)

# Create formula from template
echo "Creating formula..."
TEMPLATE_DIR=$(dirname "$0")
cat "${TEMPLATE_DIR}/sync-rs.rb.template" | \
  sed "s/__VERSION__/${VERSION}/g" | \
  sed "s/__SHA256__/${SHA256}/g" > "${WORKDIR}/sync-rs.rb"

# Clone Homebrew tap repo
echo "Cloning Homebrew tap repository..."
git clone "${BREW_TAP_REPO}" "${WORKDIR}/tap"

# Update formula
mkdir -p "${WORKDIR}/tap/Formula"
cp "${WORKDIR}/sync-rs.rb" "${WORKDIR}/tap/Formula/"

# Commit and push
cd "${WORKDIR}/tap"
git add Formula/sync-rs.rb
git commit -m "Update sync-rs to v${VERSION}"
git push

echo "Homebrew formula updated successfully!"
rm -rf "${WORKDIR}" 