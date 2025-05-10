# sync-rs

A simple tool for syncing local directories to remote servers using rsync and SSH.

## Features

- Sync local directories to remote servers using rsync
- Support for multiple remote configurations per directory
- Automatic .gitignore filtering
- Post-sync command execution
- Interactive remote shell access
- Preferred remote selection for automatic use
- Cache-based configuration management

## Installation

```bash
cargo install sync-rs
```

## Usage

Basic usage:

```bash
sync-rs user@host remote_dir
```

### Command Line Options

- `-o, --override-path`: Additional paths to sync (can specify multiple)
- `-c, --post-command`: Post-sync command to execute
- `-s, --shell`: Open an interactive shell in the remote directory after syncing
- `-n, --name`: Name for this remote configuration (used when managing multiple remotes)
- `-l, --list`: List all remote configurations for the current directory
- `-r, --remove`: Remove a remote configuration by name
- `-d, --delete-override`: Enable delete mode for override paths (default: disabled)
- `-P, --preferred`: Set this remote as the preferred one for this directory

### Examples

1. Sync to a remote server:

```bash
sync-rs user@host remote_dir
```

2. Sync with additional paths and post-sync command:

```bash
sync-rs user@host remote_dir -o path1 -o path2 -c "npm install"
```

3. Open an interactive shell after syncing:

```bash
sync-rs user@host remote_dir -s
```

4. Create a named remote configuration:

```bash
sync-rs user@host remote_dir -n my-remote
```

5. List all remote configurations:

```bash
sync-rs -l
```

6. Remove a remote configuration:

```bash
sync-rs -r my-remote
```

7. Set a remote as preferred:

```bash
sync-rs -n my-remote -P
```

### Preferred Remotes

When you have multiple remote configurations for a directory, you can set one as preferred:

1. Set a remote as preferred:

```bash
sync-rs -n my-remote -P
```

2. List remotes to see which one is preferred:

```bash
sync-rs -l
```

When running sync without specifying a remote, it will automatically use the preferred remote if one exists. If no preferred remote is set, it will prompt you to select one.

## Requirements

- Unix-like environment (Linux or macOS)
- rsync
- SSH

## License

MIT
