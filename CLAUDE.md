# CLAUDE.md

This file provides guidance to Claude Code (claude.ai/code) when working with code in this repository.

## What this is

`sync-rs` is a Unix-only CLI that mirrors the current local directory to a remote
host over `rsync`+`ssh`, with per-directory remote configurations persisted to a
cache file. Windows is rejected at compile time (`compile_error!` in `main.rs`).

## Commands

```bash
cargo build --release        # build the binary (target/release/sync-rs)
cargo test --all-features    # run tests (also: make test)
cargo test <name>            # run a single test by substring
cargo run -- <args>          # run locally, e.g. cargo run -- user@host remote_dir -l
make release VERSION=x.y.z   # bump version, commit, tag, push (triggers release CI)
```

Releases are also driven by release-please on `master`; a pushed `v*` tag runs
`.github/workflows/release.yml`, which cross-builds and publishes to crates.io and AUR.

## Architecture

Binary (`src/main.rs`) over a library (`src/lib.rs`) split into six modules:

- **`sync.rs`** — rsync and ssh invocations. `sync_directory` invokes `rsync -azP` (plus
  `--delete` when requested), taking filter rules as `--filter` arguments or streamed
  NUL-separated on stdin (`Rules`), and gates on `check_rsync_version` requiring rsync ≥ 3.
  `get_remote_home` runs `ssh host 'echo $HOME'`; `execute_ssh_command` / `open_remote_shell`
  handle post-sync commands and the interactive `-s` shell; `list_remote_siblings` /
  `remove_remote_dirs` back `--prune`.
- **`ignore.rs`** — `git_ignored_paths` turns git's own ignore decision (`ls-files --others
  --ignored --exclude-standard --directory`) into anchored rsync exclude patterns, recursing
  into nested repositories and submodules; outside a work tree it evaluates the tree against
  a throwaway bare repository.
- **`worktree.rs`** — `Location::discover` places the cwd among the repository's worktrees.
  `config_dir` maps a linked worktree onto its main-worktree counterpart so the project's
  remotes apply; `target` picks the sync source and the `@<branch>` suffix of the remote
  directory, checking out a temporary detached worktree for `-b` when none has the branch.
  `live_suffixes` is what `--prune` keeps: every suffix a sync could currently produce.
- **`console.rs`** — the `log` backend: info lines on stdout, other levels prefixed on stderr;
  `-v`/`-q` set the level, and `sync_directory` derives rsync's own verbosity from it.
- **`config.rs`** — `RemoteEntry` (the serialized config record) plus the interactive
  prompt/select/list/remove helpers and `generate_unique_name` (host-derived, collision-suffixed).
- **`cache.rs`** — persistence + versioned migration. `RemoteMap` is
  `HashMap<dir_abs_path, Vec<RemoteEntry>>`, stored at `<config_dir>/sync-rs/cache.json`.

### Config-resolution flow (the core logic)

`main::determine_remote_config` is where most complexity lives — read it before
changing CLI behavior. The branch taken depends on what's cached for the *current
working directory*:

- **host+dir given on CLI** → create/update an entry (named via `-n` or `generate_unique_name`).
- **no host given, 0 cached entries** → prompt interactively.
- **exactly 1 entry** → reuse it, overlaying any flags that were passed.
- **multiple entries** → use `--preferred`, else `-n`, else the preferred-flagged entry,
  else `select_remote` prompt.

Setting `--preferred` (`-P`) clears the `preferred` flag on all sibling entries for that
directory before setting it on the chosen one — only one preferred per directory.

`main` resolves the cache key through `Location::config_dir` unless the cwd already has
entries of its own, so worktrees inherit their project's remotes. `perform_sync` streams the
excludes from `git_ignored_paths` plus `-i` patterns (as `- <pattern>`) to rsync. Only when
git is not installed does it fall back to a per-directory `:- .gitignore` filter, passed as an
argument because `--from0` changes how rsync reads merge files. The main sync always runs
with `--delete`; override paths (`-o`) only delete when `-d` is passed.

### Cache migration

`MigrationManager` (constructed with the running `CARGO_PKG_VERSION`) reads the cache and,
if it's not the current `VersionedCache` format, walks registered `CacheMigrator`s.
`LegacyMigrator` upgrades the original unversioned `HashMap<dir, entry>` format, backing the
old file up to `cache.json.bak`. New `RemoteEntry` fields must be `#[serde(default)]` so old
caches still deserialize; add a new `CacheMigrator` only for structural changes the
`serde(default)` approach can't cover.

## Notes

- Tests live in `tests/` and need git and rsync on `PATH`: `ignore.rs` and `worktree.rs` hold
  fixed scenarios, `adversarial.rs` hand-built hostile layouts, and `fuzz.rs` random trees
  (`SYNC_RS_FUZZ_CASES`, `SYNC_RS_FUZZ_SEED`); the latter two compare the synced tree with
  the `git check-ignore` oracle in `tests/common/oracle.rs`. `make release` runs
  `cargo test --all-features` as a gate.
- `contrib/` holds AUR and (currently disabled) Homebrew packaging templates updated by CI.
