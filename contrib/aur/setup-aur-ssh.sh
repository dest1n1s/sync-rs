#!/bin/bash
set -e

if [ -z "$1" ]; then
  echo "Usage: $0 <ssh_private_key>"
  exit 1
fi

SSH_KEY="$1"

# Create SSH directory if it doesn't exist
mkdir -p ~/.ssh

# Write SSH key to file
echo "$SSH_KEY" > ~/.ssh/aur
chmod 600 ~/.ssh/aur

# Create SSH config file with StrictHostKeyChecking disabled
cat > ~/.ssh/config << EOF
Host aur.archlinux.org
  IdentityFile ~/.ssh/aur
  User aur
  StrictHostKeyChecking no
EOF

echo "AUR SSH configuration completed" 