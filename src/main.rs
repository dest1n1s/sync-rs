use anyhow::Result;
use clap::Parser;
use std::env;

// Import from our crate modules
use sync_rs::{
    cache::{get_cache_path, MigrationManager, RemoteMap},
    config::{
        generate_unique_name, list_remotes, prompt_remote_info, remove_remote, select_remote,
        RemoteEntry,
    },
    sync::{execute_ssh_command, get_remote_home, open_remote_shell, sync_directory},
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

    /// Additional paths to sync (can specify multiple)
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
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Get current directory and cache path
    let current_dir = env::current_dir()?;
    let current_dir_str = current_dir.to_str().unwrap_or_default().to_string();
    let cache_path = get_cache_path()?;

    // Initialize migration manager with current program version
    let migration_manager = MigrationManager::new(env!("CARGO_PKG_VERSION").to_string());

    // Read or initialize cache with migration support
    let mut cache: RemoteMap = migration_manager.read_cache(&cache_path)?;

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

    // Perform the sync operation
    perform_sync(&remote_entry, args.shell, args.delete_override)?;

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
        let default_name = generate_unique_name(&h, cache, current_dir);
        let name = args.name.clone().unwrap_or(default_name);

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
                // First check for preferred remote
                if let Some(preferred) = entries.iter().find(|e| e.preferred) {
                    preferred.name.clone()
                } else {
                    // No preferred remote, use name from args or prompt
                    match args.name.clone() {
                        Some(name) => name,
                        None => select_remote(entries)?,
                    }
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

// Perform the actual sync operation
fn perform_sync(remote_entry: &RemoteEntry, open_shell: bool, delete_override: bool) -> Result<()> {
    // Get remote home directory
    let remote_home = get_remote_home(&remote_entry.remote_host)?;
    let remote_full_dir = if remote_entry.remote_dir.starts_with('/') {
        remote_entry.remote_dir.clone()
    } else {
        format!("{}/{}", remote_home, remote_entry.remote_dir)
    };
    println!(
        "Syncing to {} ({}:{})",
        remote_entry.name, remote_entry.remote_host, remote_full_dir
    );

    // Sync main directory with .gitignore filtering and any additional ignore patterns
    let destination = format!("{}:{}", remote_entry.remote_host, remote_full_dir);

    // Start with .gitignore filter
    let mut filter_strings = vec![String::from(":- .gitignore")];

    // Add additional ignore patterns
    for pattern in &remote_entry.ignore_patterns {
        // Format as rsync exclude pattern
        filter_strings.push(format!("- {}", pattern));
    }

    // Join filters with commas for rsync
    let filter_string = filter_strings.join(",");

    sync_directory(".", &destination, Some(&filter_string), true)?;

    // Sync additional paths
    for path in &remote_entry.override_paths {
        sync_directory(path, &destination, None, delete_override)?;
    }

    // Execute post-sync command if specified
    if let Some(cmd) = &remote_entry.post_sync_command {
        println!("Executing post-sync command: {}", cmd);
        let full_command = format!("cd {} && {}", remote_full_dir, cmd);
        execute_ssh_command(&remote_entry.remote_host, &full_command)?;
    }

    // Open interactive shell if requested
    if open_shell {
        println!(
            "Opening interactive shell in {}:{}",
            remote_entry.remote_host, remote_full_dir
        );
        open_remote_shell(&remote_entry.remote_host, &remote_full_dir)?;
    }

    Ok(())
}
