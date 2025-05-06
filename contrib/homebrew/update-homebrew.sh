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
if ! curl -sL "${TARBALL_URL}" -o "${WORKDIR}/sync-rs-${VERSION}.tar.gz"; then
  echo "Error: Failed to download source tarball from ${TARBALL_URL}"
  exit 1
fi

SHA256=$(sha256sum "${WORKDIR}/sync-rs-${VERSION}.tar.gz" | cut -d' ' -f1)

# Create formula from template
echo "Creating formula..."
TEMPLATE_DIR=$(dirname "$0")
if [ ! -f "${TEMPLATE_DIR}/sync-rs.rb.template" ]; then
  echo "Error: Formula template not found at ${TEMPLATE_DIR}/sync-rs.rb.template"
  exit 1
fi

cat "${TEMPLATE_DIR}/sync-rs.rb.template" | \
  sed "s/__VERSION__/${VERSION}/g" | \
  sed "s/__SHA256__/${SHA256}/g" > "${WORKDIR}/sync-rs.rb"

# Clone Homebrew tap repo
echo "Cloning Homebrew tap repository..."
if ! git clone "${BREW_TAP_REPO}" "${WORKDIR}/tap" 2>/dev/null; then
  echo "Error: Failed to clone Homebrew tap repository. Check Git credentials."
  exit 1
fi

# Update formula
mkdir -p "${WORKDIR}/tap/Formula"
cp "${WORKDIR}/sync-rs.rb" "${WORKDIR}/tap/Formula/"

# Commit and push
cd "${WORKDIR}/tap"
git add Formula/sync-rs.rb
git commit -m "Update sync-rs to v${VERSION}"

echo "Pushing changes to Homebrew tap..."
if ! git push; then
  echo "Error: Failed to push changes to Homebrew tap. Check your Git credentials."
  exit 1
fi

echo "Homebrew formula updated successfully!"
rm -rf "${WORKDIR}" 