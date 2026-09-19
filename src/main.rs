use anyhow::Result;
use clap::{ArgAction, Parser};
use log::{debug, info, warn};
use std::env;
use std::time::Instant;

// Import from our crate modules
use sync_rs::{
    cache::{get_cache_path, MigrationManager, RemoteMap},
    config::{
        confirm, generate_unique_name, list_all_remotes, list_remotes, prompt_remote_info,
        remove_remote, select_remote, RemoteEntry,
    },
    console,
    ignore::{exclude_pattern, git_ignored_among, git_ignored_paths},
    sync::{
        execute_ssh_command, get_remote_home, list_remote_siblings, open_remote_shell,
        override_path, pending_deletions, remove_remote_dirs, sync_directory, sync_relative, Rules,
    },
    worktree::{suffix_for, Location, Target},
};

// This application requires a Unix-like environment
#[cfg(windows)]
compile_error!("This application does not support Windows. Please use Linux or macOS.");

#[derive(Parser, Debug)]
#[command(author, version, about, long_about = None)]
struct Args {
    /// Remote host (e.g., user@host)
    remote_host: Option<String>,

    /// Remote directory (relative to remote home)
    remote_dir: Option<String>,

    /// Additional paths to sync even if ignored, relative to the synced directory
    #[arg(short, long)]
    override_path: Vec<String>,

    /// Post-sync command to execute
    #[arg(short, long)]
    post_command: Option<String>,

    /// Open an interactive shell in the remote directory after syncing
    #[arg(short, long)]
    shell: bool,

    /// Name for this remote configuration (used when managing multiple remotes)
    #[arg(short, long)]
    name: Option<String>,

    /// List all remote configurations for the current directory
    #[arg(short, long)]
    list: bool,

    /// List the remote configurations of every directory
    #[arg(short = 'L', long)]
    list_all: bool,

    /// Print the path of the configuration file
    #[arg(long)]
    config_path: bool,

    /// Remove a remote configuration by name
    #[arg(short = 'r', long)]
    remove: Option<String>,

    /// Enable delete mode for override paths (default: disabled)
    #[arg(short = 'd', long)]
    delete_override: bool,

    /// Set this remote as the preferred one for this directory
    #[arg(short = 'P', long)]
    preferred: bool,

    /// Patterns to ignore (can specify multiple)
    #[arg(short = 'i', long = "ignore")]
    ignore_patterns: Vec<String>,

    /// Sync this branch instead of the working tree, from its worktree or a temporary checkout
    #[arg(short = 'b', long)]
    branch: Option<String>,

    /// Remove remote branch mirrors: the one for --branch, or all whose branch is gone locally
    #[arg(long)]
    prune: bool,

    /// Show what runs underneath (git, rsync, ssh); twice to list every exclude rule
    #[arg(short = 'v', long, action = ArgAction::Count)]
    verbose: u8,

    /// Only report warnings and errors
    #[arg(short = 'q', long, conflicts_with = "verbose")]
    quiet: bool,
}

fn main() {
    let args = Args::parse();
    console::init(args.verbose, args.quiet);
    if let Err(err) = run(args) {
        log::error!("{err:#}");
        std::process::exit(1);
    }
}

fn run(args: Args) -> Result<()> {
    // Get current directory and cache path
    let current_dir = env::current_dir()?;
    let cache_path = get_cache_path()?;
    debug!("cache at {}", cache_path.display());

    // Initialize migration manager with current program version
    let migration_manager = MigrationManager::new(env!("CARGO_PKG_VERSION").to_string());

    // Read or initialize cache with migration support
    let mut cache: RemoteMap = migration_manager.read_cache(&cache_path)?;

    if args.config_path {
        println!("{}", cache_path.display());
        return Ok(());
    }

    if args.list_all {
        return list_all_remotes(&cache);
    }

    // A worktree shares its main worktree's configuration unless it was configured on its own
    let own_config = current_dir
        .to_str()
        .and_then(|dir| cache.get(dir))
        .is_some_and(|entries| !entries.is_empty());
    let location = if own_config && args.branch.is_none() && !args.prune {
        Location::plain(&current_dir)
    } else {
        Location::discover(&current_dir)?
    };
    let current_dir_str = location
        .config_dir()
        .to_str()
        .unwrap_or_default()
        .to_string();
    debug!("remote configuration of {current_dir_str}");

    // Ensure the current directory exists in the cache
    if !cache.contains_key(&current_dir_str) {
        cache.insert(current_dir_str.clone(), Vec::new());
    }

    // Handle command-line options
    if args.list {
        list_remotes(&cache, &current_dir_str)?;
        return Ok(());
    }

    if let Some(name) = args.remove.clone() {
        remove_remote(&mut cache, &current_dir_str, &name)?;
        migration_manager.save_cache(&cache_path, &cache)?;
        return Ok(());
    }

    // Validate host/dir pairing if provided
    if (args.remote_host.is_some() || args.remote_dir.is_some())
        && !(args.remote_host.is_some() && args.remote_dir.is_some())
    {
        anyhow::bail!("Both remote_host and remote_dir must be provided together");
    }

    // Determine which remote to use or add new one
    let remote_entry = determine_remote_config(
        &args,
        &mut cache,
        &current_dir_str,
        &migration_manager,
        &cache_path,
    )?;

    if args.prune {
        return prune(&location, args.branch.as_deref(), &remote_entry);
    }

    // Perform the sync operation
    let target = location.target(args.branch.as_deref())?;
    perform_sync(&target, &remote_entry, args.shell, args.delete_override)?;

    Ok(())
}

// Determine which remote configuration to use based on args and cache
fn determine_remote_config(
    args: &Args,
    cache: &mut RemoteMap,
    current_dir: &str,
    migration_manager: &MigrationManager,
    cache_path: &std::path::Path,
) -> Result<RemoteEntry> {
    let remote_entry = if let (Some(h), Some(d)) =
        (args.remote_host.clone(), args.remote_dir.clone())
    {
        // Create new remote entry with name based on just the host
        let name = if let Some(name) = args.name.as_ref() {
            name.clone()
        } else if let Some(entry) = cache.get(current_dir).and_then(|entries| {
            entries
                .iter()
                .find(|e| e.remote_host == h && e.remote_dir == d)
        }) {
            entry.name.clone()
        } else {
            generate_unique_name(&h, cache, current_dir)
        };

        let entry = RemoteEntry {
            name: name.clone(),
            remote_host: h,
            remote_dir: d,
            override_paths: args.override_path.clone(),
            post_sync_command: args.post_command.clone(),
            preferred: args.preferred,
            ignore_patterns: args.ignore_patterns.clone(),
        };

        // If this is being set as preferred, unset preferred status for all other entries
        if args.preferred {
            if let Some(entries) = cache.get_mut(current_dir) {
                for e in entries.iter_mut() {
                    e.preferred = false;
                }
            }
        }

        // Check if name already exists and update or add
        let entries = cache.get_mut(current_dir).unwrap();
        if let Some(index) = entries.iter().position(|e| e.name == name) {
            entries[index] = entry.clone();
        } else {
            entries.push(entry.clone());
        }

        migration_manager.save_cache(cache_path, cache)?;
        entry
    } else {
        // Use existing entry
        let entries = cache.get(current_dir).unwrap();

        if entries.is_empty() {
            // Prompt for new remote info
            let (h, d) = prompt_remote_info()?;
            let default_name = generate_unique_name(&h, cache, current_dir);
            let name = args.name.clone().unwrap_or(default_name);

            let entry = RemoteEntry {
                name,
                remote_host: h,
                remote_dir: d,
                override_paths: args.override_path.clone(),
                post_sync_command: args.post_command.clone(),
                preferred: args.preferred,
                ignore_patterns: args.ignore_patterns.clone(),
            };

            cache.get_mut(current_dir).unwrap().push(entry.clone());
            migration_manager.save_cache(cache_path, cache)?;
            entry
        } else if entries.len() == 1 {
            // Use the only entry
            let mut entry = entries[0].clone();

            // Update with new parameters if provided
            if !args.override_path.is_empty() {
                entry.override_paths = args.override_path.clone();
                cache.get_mut(current_dir).unwrap()[0].override_paths = args.override_path.clone();
            }

            if args.post_command.is_some() {
                entry.post_sync_command = args.post_command.clone();
                cache.get_mut(current_dir).unwrap()[0].post_sync_command =
                    args.post_command.clone();
            }

            if args.preferred {
                entry.preferred = true;
                cache.get_mut(current_dir).unwrap()[0].preferred = true;
            }

            if !args.ignore_patterns.is_empty() {
                entry.ignore_patterns = args.ignore_patterns.clone();
                cache.get_mut(current_dir).unwrap()[0].ignore_patterns =
                    args.ignore_patterns.clone();
            }

            migration_manager.save_cache(cache_path, cache)?;
            entry
        } else {
            // Multiple entries, check for preferred or prompt for selection
            let name = if args.preferred {
                // If setting preferred, use the name from args
                args.name
                    .clone()
                    .ok_or_else(|| anyhow::anyhow!("Name required when setting preferred remote"))?
            } else {
                if let Some(name) = args.name.clone() {
                    name
                } else if let Some(preferred) = entries.iter().find(|e| e.preferred) {
                    preferred.name.clone()
                } else {
                    select_remote(entries)?
                }
            };

            let entry = entries
                .iter()
                .find(|e| e.name == name)
                .ok_or_else(|| anyhow::anyhow!("Remote with name '{}' not found", name))?
                .clone();

            // Update with new parameters if provided
            if !args.override_path.is_empty()
                || args.post_command.is_some()
                || args.preferred
                || !args.ignore_patterns.is_empty()
            {
                let mut updated_entry = entry.clone();

                if !args.override_path.is_empty() {
                    updated_entry.override_paths = args.override_path.clone();
                }

                if args.post_command.is_some() {
                    updated_entry.post_sync_command = args.post_command.clone();
                }

                if args.preferred {
                    // Unset preferred status for all other entries
                    for e in cache.get_mut(current_dir).unwrap().iter_mut() {
                        e.preferred = false;
                    }
                    updated_entry.preferred = true;
                }

                if !args.ignore_patterns.is_empty() {
                    updated_entry.ignore_patterns = args.ignore_patterns.clone();
                }

                // Update in cache
                if let Some(index) = cache
                    .get_mut(current_dir)
                    .unwrap()
                    .iter()
                    .position(|e| e.name == name)
                {
                    cache.get_mut(current_dir).unwrap()[index] = updated_entry.clone();
                    migration_manager.save_cache(cache_path, cache)?;
                    updated_entry
                } else {
                    entry
                }
            } else {
                entry
            }
        }
    };

    Ok(remote_entry)
}

// Remove branch mirrors next to the remote directory that nothing local would sync to anymore
fn prune(location: &Location, branch: Option<&str>, remote_entry: &RemoteEntry) -> Result<()> {
    let live = location
        .live_suffixes()?
        .ok_or_else(|| anyhow::anyhow!("--prune requires a git repository"))?;
    let remote_home = get_remote_home(&remote_entry.remote_host)?;
    let base = remote_full_dir(&remote_entry.remote_dir, &remote_home);
    let prefix = format!("{}@", base);
    let mirrors = list_remote_siblings(&remote_entry.remote_host, &base)?;

    let (doomed, kept): (Vec<String>, Vec<String>) = mirrors.into_iter().partition(|dir| {
        let suffix = dir.strip_prefix(&prefix).unwrap_or_default();
        match branch {
            Some(branch) => suffix == suffix_for(branch),
            None => !live.contains(suffix),
        }
    });
    for dir in &kept {
        info!("keep   {}", tilde(dir, &remote_home));
    }
    if doomed.is_empty() {
        info!("Nothing to prune");
        return Ok(());
    }
    for dir in &doomed {
        info!("remove {}", tilde(dir, &remote_home));
    }
    if !confirm(&format!(
        "Remove {} director{} on {}?",
        doomed.len(),
        if doomed.len() == 1 { "y" } else { "ies" },
        remote_entry.remote_host
    ))? {
        info!("Aborted");
        return Ok(());
    }
    remove_remote_dirs(&remote_entry.remote_host, &doomed)?;
    info!(
        "Removed {} director{}",
        doomed.len(),
        if doomed.len() == 1 { "y" } else { "ies" }
    );
    Ok(())
}

/// `path` with the remote home shortened to `~`.
fn tilde(path: &str, remote_home: &str) -> String {
    match path.strip_prefix(remote_home) {
        Some(rest) if rest.is_empty() || rest.starts_with('/') => format!("~{rest}"),
        _ => path.to_string(),
    }
}

fn remote_full_dir(remote_dir: &str, remote_home: &str) -> String {
    if remote_dir.starts_with('/') {
        remote_dir.to_string()
    } else {
        format!("{}/{}", remote_home, remote_dir)
    }
}

// Perform the actual sync operation
fn perform_sync(
    target: &Target,
    remote_entry: &RemoteEntry,
    open_shell: bool,
    delete_override: bool,
) -> Result<()> {
    // Get remote home directory
    let remote_home = get_remote_home(&remote_entry.remote_host)?;
    let remote_dir = match &target.suffix {
        Some(suffix) => format!("{}@{}", remote_entry.remote_dir, suffix),
        None => remote_entry.remote_dir.clone(),
    };
    let remote_full_dir = remote_full_dir(&remote_dir, &remote_home);
    info!(
        "Syncing {} to {} ({}:{})",
        target.label,
        remote_entry.name,
        remote_entry.remote_host,
        tilde(&remote_full_dir, &remote_home)
    );
    debug!("source {}", target.source.display());
    let started = Instant::now();

    // Sync main directory, excluding what git ignores plus any additional ignore patterns.
    // Override paths belong to their own transfer below, so the main one must neither send
    // nor delete them; a linked worktree's `.git` only points at a local path.
    let destination = format!("{}:{}", remote_entry.remote_host, remote_full_dir);
    let source = format!("{}/", target.source.display());
    let overrides: Vec<String> = remote_entry
        .override_paths
        .iter()
        .map(|path| override_path(path))
        .collect::<Result<_>>()?;
    let ignore_rules: Vec<String> = remote_entry
        .ignore_patterns
        .iter()
        .map(|pattern| format!("- {}", pattern))
        .collect();
    let user_rules = target
        .suffix
        .iter()
        .map(|_| String::from("- /.git"))
        .chain(overrides.iter().map(|path| {
            format!(
                "- {}",
                String::from_utf8_lossy(&exclude_pattern(b"", path.as_bytes()))
            )
        }))
        .chain(ignore_rules.iter().cloned());

    match git_ignored_paths(&target.source)? {
        Some(paths) => {
            info!("Excluding {} paths ignored by git", paths.len());
            let mut rules: Vec<Vec<u8>> = paths
                .iter()
                .map(|path| [b"- ", path.as_slice()].concat())
                .chain(user_rules.map(String::into_bytes))
                .collect();
            let doomed = pending_deletions(&source, &destination, Rules::Stream(&rules))?;
            let kept = git_ignored_among(&target.source, &doomed)?;
            if !kept.is_empty() {
                info!("Keeping {} remote paths ignored by git", kept.len());
            }
            rules.extend(kept.iter().map(|path| [b"- ", path.as_slice()].concat()));
            sync_directory(&source, &destination, Rules::Stream(&rules), true)?;
        }
        None => {
            warn!("git not found, relying on rsync's own .gitignore reading");
            let rules: Vec<String> = std::iter::once(String::from(":- .gitignore"))
                .chain(user_rules)
                .collect();
            sync_directory(&source, &destination, Rules::Args(&rules), true)?;
        }
    }

    // Sync override paths to the same relative location
    for path in &overrides {
        if !target.source.join(path).exists() {
            warn!("Skipping override path {path}: not found locally");
            continue;
        }
        info!("Syncing override path {path}");
        sync_relative(
            &target.source,
            path,
            &destination,
            Rules::Args(&ignore_rules),
            delete_override,
        )?;
    }

    info!("Synced in {:.1}s", started.elapsed().as_secs_f64());

    // Execute post-sync command if specified
    if let Some(cmd) = &remote_entry.post_sync_command {
        info!("Running post-sync command: {}", cmd);
        let full_command = format!("cd {} && {}", remote_full_dir, cmd);
        execute_ssh_command(&remote_entry.remote_host, &full_command)?;
    }

    // Open interactive shell if requested
    if open_shell {
        info!(
            "Opening shell in {}:{}",
            remote_entry.remote_host,
            tilde(&remote_full_dir, &remote_home)
        );
        open_remote_shell(&remote_entry.remote_host, &remote_full_dir)?;
    }

    Ok(())
}
