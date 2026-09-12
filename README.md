# sync-rs

A simple tool for syncing local directories to remote servers using rsync and SSH.

## Features

- Sync local directories to remote servers using rsync
- Support for multiple remote configurations per directory
- Excludes exactly what git ignores
- Worktrees and branches mirror to their own remote directories
- Additional ignore patterns support
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
- `-p, --post-command`: Post-sync command to execute
- `-s, --shell`: Open an interactive shell in the remote directory after syncing
- `-n, --name`: Name for this remote configuration (used when managing multiple remotes)
- `-l, --list`: List all remote configurations for the current directory
- `-r, --remove`: Remove a remote configuration by name
- `-d, --delete-override`: Enable delete mode for override paths (default: disabled)
- `-P, --preferred`: Set this remote as the preferred one for this directory
- `-i, --ignore`: Patterns to ignore (can specify multiple)
- `-b, --branch`: Sync this branch instead of the working tree
- `--prune`: Remove remote branch mirrors, all stale ones or the one for `-b`
- `-v, --verbose`: Show the git, rsync and ssh commands underneath; `-vv` also lists every exclude rule
- `-q, --quiet`: Only report warnings and errors

### Examples

1. Sync to a remote server:

```bash
sync-rs user@host remote_dir
```

2. Sync with additional paths and post-sync command:

```bash
sync-rs user@host remote_dir -o path1 -o path2 -p "npm install"
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

8. Sync with additional ignore patterns:

```bash
sync-rs user@host remote_dir -i "*.tmp" -i "build/"
```

### Worktrees and Branches

A linked worktree shares the remote configuration of its main worktree and mirrors to `<remote_dir>@<branch>` (or `@<directory name>` when detached), so `cd ../proj-feature && sync-rs` lands in `proj@feature` and leaves the main mirror alone. `.git` is not copied for such mirrors.

To try a branch remotely without touching your working tree:

```bash
sync-rs -b feature -p "cargo test"
```

This syncs from the worktree checked out on `feature`, or from a temporary detached checkout that is removed afterwards, to `<remote_dir>@feature`. Slashes in branch names become dashes.

Branch mirrors accumulate on the remote. `sync-rs --prune` lists them, and after confirmation removes those whose name no longer matches any local worktree, branch, tag or remote-tracking ref; `sync-rs --prune -b feature` removes that one mirror.

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

### Ignore Patterns

By default, sync-rs excludes whatever git ignores: every `.gitignore` in the tree, `.git/info/exclude` and the global excludes file, with nested repositories and submodules judged by their own rules. Tracked files are always synced, and a directory outside any work tree still has its `.gitignore` files honored. Without git installed, rsync reads the `.gitignore` files itself and negation patterns are not supported.

You can specify additional patterns to ignore:

```bash
sync-rs -i "*.tmp" -i "build/" -i "node_modules/"
```

These patterns are applied on top of git's rules and follow rsync's exclude format.

## Requirements

- Unix-like environment (Linux or macOS)
- rsync
- SSH
- git (optional; without it rsync reads `.gitignore` files itself)

## License

MIT
