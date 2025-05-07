# Packaging for sync-rs

This directory contains templates and scripts for packaging sync-rs for various package managers.

## AUR (Arch User Repository)

The `aur` directory contains:

- `PKGBUILD.template`: Template for the AUR package

The AUR package is automatically updated via GitHub Actions using the [github-actions-deploy-aur](https://github.com/KSXGitHub/github-actions-deploy-aur) action.

## Homebrew

The `homebrew` directory contains:

- `sync-rs.rb.template`: Template for the Homebrew formula
- `update-homebrew.sh`: Script to update the Homebrew formula
- `setup-git-auth.sh`: Script to set up Git authentication for GitHub Actions

To manually update the Homebrew formula:

```bash
./homebrew/update-homebrew.sh <version>
```

## Automation

These packages are automatically updated when a new version is released via GitHub Actions. The following secrets need to be set in your GitHub repository:

### Required GitHub Secrets

- `AUR_SSH_PRIVATE_KEY`: SSH key for AUR access
- `HOMEBREW_GITHUB_TOKEN`: GitHub token with repo access to your Homebrew tap repository
