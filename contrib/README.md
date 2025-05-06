# Packaging for sync-rs

This directory contains scripts and templates for packaging sync-rs for various package managers.

## AUR (Arch User Repository)

The `aur` directory contains:

- `PKGBUILD.template`: Template for the AUR package
- `update-aur.sh`: Script to update the AUR package

To manually update the AUR package:

```bash
./aur/update-aur.sh <version>
```

## Homebrew

The `homebrew` directory contains:

- `sync-rs.rb.template`: Template for the Homebrew formula
- `update-homebrew.sh`: Script to update the Homebrew formula

To manually update the Homebrew formula:

```bash
./homebrew/update-homebrew.sh <version>
```

## Automation

These packages are automatically updated when a new version is released via GitHub Actions. The following secrets need to be set in your GitHub repository:

- `AUR_SSH_PRIVATE_KEY`: SSH key for AUR access
- `HOMEBREW_GITHUB_TOKEN`: GitHub token with repo access to your Homebrew tap repository
