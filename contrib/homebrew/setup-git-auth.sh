#!/bin/bash
set -e

if [ -z "$1" ]; then
  echo "Usage: $0 <github_token>"
  exit 1
fi

GITHUB_TOKEN="$1"

# Configure git
git config --global user.email "github-actions@github.com"
git config --global user.name "GitHub Actions"

# Use the token for auth
git config --global url."https://oauth2:${GITHUB_TOKEN}@github.com".insteadOf "https://github.com"

echo "Git authentication configured" 