use anyhow::{Context, Result};
use clap::Parser;
use dirs::config_dir;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::process::Command;

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
}

#[derive(Debug, Serialize, Deserialize)]
struct CacheEntry {
    remote_host: String,
    remote_dir: String,
    #[serde(default)]
    override_paths: Vec<String>,
    #[serde(default)]
    post_sync_command: Option<String>,
}

fn main() -> Result<()> {
    let args = Args::parse();

    // Validate host/dir pairing
    if (args.remote_host.is_some() || args.remote_dir.is_some())
        && !(args.remote_host.is_some() && args.remote_dir.is_some())
    {
        anyhow::bail!("Both remote_host and remote_dir must be provided together");
    }

    // Get current directory and cache path
    let current_dir = std::env::current_dir()?;
    let current_dir_str = current_dir
        .to_str()
        .context("Current directory is not valid UTF-8")?
        .to_string();
    let cache_path = get_cache_path()?;

    // Read or initialize cache
    let mut cache = read_cache(&cache_path)?;

    // Determine remote configuration
    let (remote_host, remote_dir, override_paths, post_sync_command) =
        if let (Some(h), Some(d)) = (args.remote_host, args.remote_dir) {
            // Create new cache entry
            let entry = CacheEntry {
                remote_host: h.clone(),
                remote_dir: d.clone(),
                override_paths: args.override_path.clone(),
                post_sync_command: args.post_command.clone(),
            };
            cache.insert(current_dir_str.clone(), entry);
            (h, d, args.override_path, args.post_command)
        } else {
            // Use existing entry or prompt
            match cache.get_mut(&current_dir_str) {
                Some(entry) => {
                    // Update existing entry with new parameters
                    if !args.override_path.is_empty() {
                        entry.override_paths = args.override_path.clone();
                    }
                    if args.post_command.is_some() {
                        entry.post_sync_command = args.post_command.clone();
                    }
                    let result = (
                        entry.remote_host.clone(),
                        entry.remote_dir.clone(),
                        entry.override_paths.clone(),
                        entry.post_sync_command.clone(),
                    );
                    save_cache(&cache_path, &cache)?;
                    result
                }
                None => {
                    // Prompt for missing information
                    let (h, d) = prompt_remote_info()?;
                    let entry = CacheEntry {
                        remote_host: h.clone(),
                        remote_dir: d.clone(),
                        override_paths: args.override_path.clone(),
                        post_sync_command: args.post_command.clone(),
                    };
                    cache.insert(current_dir_str.clone(), entry);
                    save_cache(&cache_path, &cache)?;
                    (h, d, args.override_path, args.post_command)
                }
            }
        };

    // Get remote home directory
    let remote_home = get_remote_home(&remote_host)?;
    let remote_full_dir = format!("{}/{}", remote_home, remote_dir);
    println!("Syncing to {}:{}", remote_host, remote_full_dir);

    // Sync main directory with .gitignore filtering
    let destination = format!("{}:{}", remote_host, remote_full_dir);
    sync_directory(".", &destination, Some(":- .gitignore"))?;

    // Sync additional paths
    for path in &override_paths {
        sync_directory(path, &destination, None)?;
    }

    // Execute post-sync command if specified
    if let Some(cmd) = post_sync_command {
        println!("Executing post-sync command: {}", cmd);
        let full_command = format!(
            "cd {} && . {}/.local/bin/env && {}",
            remote_full_dir, remote_home, cmd
        );
        execute_ssh_command(&remote_host, &full_command)?;
    }

    Ok(())
}

fn get_cache_path() -> Result<PathBuf> {
    let config_dir = config_dir().context("Failed to find config directory")?;
    let cache_dir = config_dir.join("sync-tool");
    if !cache_dir.exists() {
        fs::create_dir_all(&cache_dir).context("Failed to create cache directory")?;
    }
    Ok(cache_dir.join("cache.json"))
}

fn read_cache(cache_path: &Path) -> Result<HashMap<String, CacheEntry>> {
    if cache_path.exists() {
        let file = File::open(cache_path).context("Failed to open cache file")?;
        serde_json::from_reader(file).context("Failed to parse cache file")
    } else {
        Ok(HashMap::new())
    }
}

fn save_cache(cache_path: &Path, cache: &HashMap<String, CacheEntry>) -> Result<()> {
    let file = File::create(cache_path).context("Failed to create cache file")?;
    serde_json::to_writer_pretty(file, cache).context("Failed to write cache file")
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

fn sync_directory(source: &str, destination: &str, filter: Option<&str>) -> Result<()> {
    let mut cmd = Command::new("rsync");
    cmd.args(["-azP", "--delete"]);

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
