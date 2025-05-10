# sync-rs

A simple repository synchronization tool written in Rust. Largely a wrapper around `git` and `rsync`. It will respect `.gitignore` files and sync the repository to a target remote directory, and remember the last sync state.

## Usage

To activate interactive mode to sync the current directory to the remote directory, run:

```bash
sync-rs
```

If this is your first time running in the current directory, you'll be prompted to set up a remote configuration.

### Basic Commands

To sync the current directory to a remote:

```bash
sync-rs <remote-host> <remote-dir> # The remote host and directory to sync to. The remote directory is a relative path to the remote host's home directory.
```

### Multiple Remote Support

You can now configure multiple remote destinations for a single local directory:

```bash
# Add a new remote with a specific name
sync-rs <remote-host> <remote-dir> --name <remote-name>

# List all configured remotes for the current directory
sync-rs --list

# Sync to a specific remote by name
sync-rs --name <remote-name>

# Remove a remote configuration
sync-rs --remove <remote-name>
```

If multiple remotes are configured and no name is specified, you'll be prompted to select one.

### Optional Flags

```bash
sync-rs <remote-host> <remote-dir> -p <post-command> # Run a command after syncing
sync-rs <remote-host> <remote-dir> -o <override-path> # Sync the override path despite the .gitignore. This is helpful to sync custom experimental files.
sync-rs <remote-host> <remote-dir> -s # Open an interactive shell in the remote directory using ssh after syncing
sync-rs <remote-host> <remote-dir> -d # Enable delete mode for override paths (by default, only the main directory uses --delete)
```

### Complete Options

```
Options:
  -o, --override-path <OVERRIDE_PATH>  Additional paths to sync (can specify multiple)
  -p, --post-command <POST_COMMAND>    Post-sync command to execute
  -s, --shell                          Open an interactive shell in the remote directory after syncing
  -d, --delete-override                Enable delete mode for override paths
  -n, --name <NAME>                    Name for this remote configuration (used when managing multiple remotes)
  -l, --list                           List all remote configurations for the current directory
  -r, --remove <REMOVE>                Remove a remote configuration by name
  -h, --help                           Print help
  -V, --version                        Print version
```

## Installation

### General

You can install `sync-rs` using `cargo` from [crates.io](https://crates.io/crates/sync-rs):

```bash
cargo install sync-rs
```

Note that `sync-rs` requires a Unix-like operating system, so it will not work on Windows.

### Arch Linux

You can install `sync-rs` from the AUR using your preferred AUR helper (e.g. `yay`):

```bash
yay -S sync-rs
```

## Cache & Configuration

sync-rs stores configuration in `~/.config/sync-rs/cache.json`. The cache format is versioned and automatically migrated when you upgrade to a new version.

## License

[MIT](LICENSE)
