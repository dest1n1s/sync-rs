#!/bin/bash
set -e

# Check arguments
if [ -z "$1" ]; then
  echo "Usage: ./update-aur.sh <version>"
  exit 1
fi

VERSION=$1
WORKDIR=$(mktemp -d)
REPO_URL="https://aur.archlinux.org/sync-rs.git"
TARBALL_URL="https://github.com/Dest1n1s/sync-rs/archive/v${VERSION}.tar.gz"

echo "Preparing AUR package for sync-rs v${VERSION}..."

# Download the source tarball to calculate its SHA256
echo "Downloading source tarball..."
curl -sL "${TARBALL_URL}" -o "${WORKDIR}/sync-rs-${VERSION}.tar.gz"
SHA256=$(sha256sum "${WORKDIR}/sync-rs-${VERSION}.tar.gz" | cut -d' ' -f1)

# Create PKGBUILD from template
echo "Creating PKGBUILD..."
TEMPLATE_DIR=$(dirname "$0")
cat "${TEMPLATE_DIR}/PKGBUILD.template" | \
  sed "s/__VERSION__/${VERSION}/g" | \
  sed "s/__SHA256__/${SHA256}/g" > "${WORKDIR}/PKGBUILD"

# Clone AUR repo
echo "Cloning AUR repository..."
git clone "${REPO_URL}" "${WORKDIR}/repo"

# Update files
cp "${WORKDIR}/PKGBUILD" "${WORKDIR}/repo/"

# Update .SRCINFO
cd "${WORKDIR}/repo"
makepkg --printsrcinfo > .SRCINFO

# Commit and push
git add PKGBUILD .SRCINFO
git commit -m "Update to v${VERSION}"
git push

echo "AUR package updated successfully!"
rm -rf "${WORKDIR}" 