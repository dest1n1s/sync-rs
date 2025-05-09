use anyhow::{Context, Result};
use clap::Parser;
use serde::{Deserialize, Serialize};
use std::io::{self, Write};
use std::process::Command;

mod cache;
use cache::{get_cache_path, MigrationManager, RemoteMap};

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
}

#[derive(Debug, Serialize, Deserialize, Clone)]
struct RemoteEntry {
    name: String,
    remote_host: String,
    remote_dir: String,
    #[serde(default)]
    override_paths: Vec<String>,
    #[serde(default)]
    post_sync_command: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Get current directory and cache path
    let current_dir = std::env::current_dir()?;
    let current_dir_str = current_dir
        .to_str()
        .context("Current directory is not valid UTF-8")?
        .to_string();
    let cache_path = get_cache_path()?;

    // Initialize migration manager with current program version
    let migration_manager = MigrationManager::new(env!("CARGO_PKG_VERSION").to_string());

    // Read or initialize cache with migration support
    let mut cache: RemoteMap = migration_manager.read_cache(&cache_path)?;

    // Ensure the current directory exists in the cache
    if !cache.contains_key(&current_dir_str) {
        cache.insert(current_dir_str.clone(), Vec::new());
    }

    // List all remotes if requested
    if args.list {
        list_remotes(&cache, &current_dir_str)?;
        return Ok(());
    }

    // Remove a remote if requested
    if let Some(name) = args.remove {
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
    let remote_entry =
        if let (Some(h), Some(d)) = (args.remote_host.clone(), args.remote_dir.clone()) {
            // Create new remote entry
            let name = args
                .name
                .clone()
                .unwrap_or_else(|| format!("{}_{}", h, d.replace('/', "_")));

            let entry = RemoteEntry {
                name: name.clone(),
                remote_host: h,
                remote_dir: d,
                override_paths: args.override_path.clone(),
                post_sync_command: args.post_command.clone(),
            };

            // Check if name already exists and update or add
            let entries = cache.get_mut(&current_dir_str).unwrap();
            if let Some(index) = entries.iter().position(|e| e.name == name) {
                entries[index] = entry.clone();
            } else {
                entries.push(entry.clone());
            }

            migration_manager.save_cache(&cache_path, &cache)?;
            entry
        } else {
            // Use existing entry
            let entries = cache.get(&current_dir_str).unwrap();

            if entries.is_empty() {
                // Prompt for new remote info
                let (h, d) = prompt_remote_info()?;
                let name = args
                    .name
                    .clone()
                    .unwrap_or_else(|| format!("{}_{}", h, d.replace('/', "_")));

                let entry = RemoteEntry {
                    name,
                    remote_host: h,
                    remote_dir: d,
                    override_paths: args.override_path.clone(),
                    post_sync_command: args.post_command.clone(),
                };

                cache.get_mut(&current_dir_str).unwrap().push(entry.clone());
                migration_manager.save_cache(&cache_path, &cache)?;
                entry
            } else if entries.len() == 1 {
                // Use the only entry
                let mut entry = entries[0].clone();

                // Update with new parameters if provided
                if !args.override_path.is_empty() {
                    entry.override_paths = args.override_path.clone();
                    cache.get_mut(&current_dir_str).unwrap()[0].override_paths =
                        args.override_path.clone();
                }

                if args.post_command.is_some() {
                    entry.post_sync_command = args.post_command.clone();
                    cache.get_mut(&current_dir_str).unwrap()[0].post_sync_command =
                        args.post_command.clone();
                }

                migration_manager.save_cache(&cache_path, &cache)?;
                entry
            } else {
                // Multiple entries, prompt for selection
                let name = match args.name.clone() {
                    Some(name) => name,
                    None => select_remote(entries)?,
                };
                let entry = entries
                    .iter()
                    .find(|e| e.name == name)
                    .with_context(|| format!("Remote with name '{}' not found", name))?
                    .clone();

                // Update with new parameters if provided
                if !args.override_path.is_empty() || args.post_command.is_some() {
                    let mut updated_entry = entry.clone();

                    if !args.override_path.is_empty() {
                        updated_entry.override_paths = args.override_path.clone();
                    }

                    if args.post_command.is_some() {
                        updated_entry.post_sync_command = args.post_command.clone();
                    }

                    // Update in cache
                    if let Some(index) = cache
                        .get_mut(&current_dir_str)
                        .unwrap()
                        .iter()
                        .position(|e| e.name == name)
                    {
                        cache.get_mut(&current_dir_str).unwrap()[index] = updated_entry.clone();
                        migration_manager.save_cache(&cache_path, &cache)?;
                        updated_entry
                    } else {
                        entry
                    }
                } else {
                    entry
                }
            }
        };

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

    // Sync main directory with .gitignore filtering
    let destination = format!("{}:{}", remote_entry.remote_host, remote_full_dir);
    sync_directory(".", &destination, Some(":- .gitignore"), true)?;

    // Sync additional paths
    for path in &remote_entry.override_paths {
        sync_directory(path, &destination, None, false)?;
    }

    // Execute post-sync command if specified
    if let Some(cmd) = remote_entry.post_sync_command {
        println!("Executing post-sync command: {}", cmd);
        let full_command = format!("cd {} && {}", remote_full_dir, cmd);
        execute_ssh_command(&remote_entry.remote_host, &full_command)?;
    }

    // Open interactive shell if requested
    if args.shell {
        println!(
            "Opening interactive shell in {}:{}",
            remote_entry.remote_host, remote_full_dir
        );
        open_remote_shell(&remote_entry.remote_host, &remote_full_dir)?;
    }

    Ok(())
}

fn prompt_remote_info() -> Result<(String, String)> {
    let mut remote_host = String::new();
    let mut remote_dir = String::new();

    print!("Enter remote host (e.g., user@host): ");
    io::stdout().flush()?;
    io::stdin().read_line(&mut remote_host)?;

    print!("Enter remote directory (relative to remote home): ");
    io::stdout().flush()?;
    io::stdin().read_line(&mut remote_dir)?;

    Ok((
        remote_host.trim().to_string(),
        remote_dir.trim().to_string(),
    ))
}

fn list_remotes(cache: &RemoteMap, current_dir: &str) -> Result<()> {
    let empty_vec: Vec<RemoteEntry> = Vec::new();
    let entries = cache.get(current_dir).unwrap_or(&empty_vec);

    if entries.is_empty() {
        println!("No remote configurations found for this directory.");
        return Ok(());
    }

    println!("Remote configurations for this directory:");
    for (i, entry) in entries.iter().enumerate() {
        println!(
            "{}: {} ({}:{})",
            i + 1,
            entry.name,
            entry.remote_host,
            entry.remote_dir
        );
    }

    Ok(())
}

fn select_remote(entries: &[RemoteEntry]) -> Result<String> {
    println!("Multiple remote configurations found. Please select one:");

    for (i, entry) in entries.iter().enumerate() {
        println!(
            "{}: {} ({}:{})",
            i + 1,
            entry.name,
            entry.remote_host,
            entry.remote_dir
        );
    }

    let mut selection = String::new();
    print!("Enter selection (1-{}): ", entries.len());
    io::stdout().flush()?;
    io::stdin().read_line(&mut selection)?;

    let index = selection
        .trim()
        .parse::<usize>()
        .context("Invalid selection")?
        - 1;

    if index >= entries.len() {
        anyhow::bail!("Selection out of range");
    }

    Ok(entries[index].name.clone())
}

fn remove_remote(cache: &mut RemoteMap, current_dir: &str, name: &str) -> Result<()> {
    let entries = cache
        .get_mut(current_dir)
        .context("No remotes found for this directory")?;

    let initial_len = entries.len();
    entries.retain(|e| e.name != name);

    if entries.len() == initial_len {
        anyhow::bail!("Remote with name '{}' not found", name);
    }

    println!("Removed remote configuration '{}'", name);
    Ok(())
}

fn get_remote_home(remote_host: &str) -> Result<String> {
    let output = Command::new("ssh")
        .arg(remote_host)
        .arg("echo $HOME")
        .output()
        .context("Failed to get remote home directory")?;

    if !output.status.success() {
        anyhow::bail!(
            "SSH command failed: {}",
            String::from_utf8_lossy(&output.stderr)
        );
    }

    let home = String::from_utf8(output.stdout)?.trim().to_string();

    if home.is_empty() {
        anyhow::bail!("Remote home directory is empty");
    }

    Ok(home)
}

fn sync_directory(
    source: &str,
    destination: &str,
    filter: Option<&str>,
    delete: bool,
) -> Result<()> {
    let mut cmd = Command::new("rsync");
    cmd.args(["-azP"]);

    if delete {
        cmd.args(["--delete"]);
    }

    if let Some(f) = filter {
        cmd.args(["--filter", f]);
    }

    cmd.args([source, destination]);

    let status = cmd.status().context("Failed to execute rsync command")?;

    if !status.success() {
        anyhow::bail!("rsync failed with exit code: {:?}", status.code());
    }

    Ok(())
}

fn execute_ssh_command(host: &str, command: &str) -> Result<()> {
    let status = Command::new("ssh")
        .arg(host)
        .arg(command)
        .status()
        .context("Failed to execute SSH command")?;

    if !status.success() {
        anyhow::bail!("SSH command failed with exit code: {:?}", status.code());
    }

    Ok(())
}

fn open_remote_shell(host: &str, directory: &str) -> Result<()> {
    let status = Command::new("ssh")
        .arg("-t") // Force pseudo-terminal allocation for interactive shell
        .arg(host)
        .arg(format!("cd {} && exec $SHELL -l", directory))
        .status()
        .context("Failed to open remote shell")?;

    if !status.success() {
        anyhow::bail!("Remote shell exited with code: {:?}", status.code());
    }

    Ok(())
}
