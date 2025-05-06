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

# Check if SSH is properly configured
if [ ! -f ~/.ssh/config ]; then
  echo "Error: SSH config file not found. Please run setup-aur-ssh.sh first."
  exit 1
fi

# Download the source tarball to calculate its SHA256
echo "Downloading source tarball..."
if ! curl -sL "${TARBALL_URL}" -o "${WORKDIR}/sync-rs-${VERSION}.tar.gz"; then
  echo "Error: Failed to download source tarball from ${TARBALL_URL}"
  exit 1
fi

SHA256=$(sha256sum "${WORKDIR}/sync-rs-${VERSION}.tar.gz" | cut -d' ' -f1)

# Create PKGBUILD from template
echo "Creating PKGBUILD..."
TEMPLATE_DIR=$(dirname "$0")
if [ ! -f "${TEMPLATE_DIR}/PKGBUILD.template" ]; then
  echo "Error: PKGBUILD template not found at ${TEMPLATE_DIR}/PKGBUILD.template"
  exit 1
fi

cat "${TEMPLATE_DIR}/PKGBUILD.template" | \
  sed "s/__VERSION__/${VERSION}/g" | \
  sed "s/__SHA256__/${SHA256}/g" > "${WORKDIR}/PKGBUILD"

# Clone AUR repo
echo "Cloning AUR repository..."
if ! git clone "${REPO_URL}" "${WORKDIR}/repo" 2>/dev/null; then
  echo "Error: Failed to clone AUR repository. Check SSH credentials."
  exit 1
fi

# Update files
cp "${WORKDIR}/PKGBUILD" "${WORKDIR}/repo/"

# Create .SRCINFO manually instead of using makepkg
cd "${WORKDIR}/repo"
echo "Generating .SRCINFO manually..."

cat > .SRCINFO << EOF
pkgbase = sync-rs
	pkgdesc = A CLI tool to sync files between directories
	pkgver = ${VERSION}
	pkgrel = 1
	url = https://github.com/dest1n1s/sync-rs
	arch = x86_64
	license = MIT
	makedepends = cargo
	depends = gcc-libs
	depends = rsync
	depends = openssh
	source = sync-rs-${VERSION}.tar.gz::https://github.com/Dest1n1s/sync-rs/archive/v${VERSION}.tar.gz
	sha256sums = ${SHA256}

pkgname = sync-rs
EOF

# Commit and push
git add PKGBUILD .SRCINFO
git commit -m "Update to v${VERSION}"

echo "Pushing changes to AUR..."
if ! git push; then
  echo "Error: Failed to push changes to AUR. Check your SSH key and access rights."
  exit 1
fi

echo "AUR package updated successfully!"
rm -rf "${WORKDIR}" 