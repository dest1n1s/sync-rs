# sync-rs

A simple repository synchronization tool written in Rust. Largely a wrapper around `git` and `rsync`. It will respect `.gitignore` files and sync the repository to a target remote directory, and remember the last sync state.

## Usage

To activate interactive mode to sync the current directory to the remote directory, run:

```bash
sync-rs
```

To sync the current directory to the remote directory, run:

```bash
sync-rs <remote-host> <remote-dir> # The remote host and directory to sync to. The remote directory is a relative path to the remote host's home directory.
```

Some optional flags are available:

```bash
sync-rs <remote-host> <remote-dir> -p <post-command> # Run a command after syncing
sync-rs <remote-host> <remote-dir> -o <override-path> # Sync the override path despite the .gitignore. This is helpful to sync custom experimental files.
sync-rs <remote-host> <remote-dir> -s # Open an interactive shell in the remote directory using ssh after syncing
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

## License

[MIT](LICENSE)
