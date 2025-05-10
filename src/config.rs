use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::io::{self, Write};

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct RemoteEntry {
    pub name: String,
    pub remote_host: String,
    pub remote_dir: String,
    #[serde(default)]
    pub override_paths: Vec<String>,
    #[serde(default)]
    pub post_sync_command: Option<String>,
    #[serde(default)]
    pub preferred: bool,
}

pub fn prompt_remote_info() -> Result<(String, String)> {
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

pub fn select_remote(entries: &[RemoteEntry]) -> Result<String> {
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

pub fn list_remotes(cache: &crate::cache::RemoteMap, current_dir: &str) -> Result<()> {
    let empty_vec: Vec<RemoteEntry> = Vec::new();
    let entries = cache.get(current_dir).unwrap_or(&empty_vec);

    if entries.is_empty() {
        println!("No remote configurations found for this directory.");
        return Ok(());
    }

    println!("Remote configurations for this directory:");
    for (i, entry) in entries.iter().enumerate() {
        let preferred = if entry.preferred { " (preferred)" } else { "" };
        println!(
            "{}: {}{} ({}:{})",
            i + 1,
            entry.name,
            preferred,
            entry.remote_host,
            entry.remote_dir
        );
    }

    Ok(())
}

pub fn remove_remote(
    cache: &mut crate::cache::RemoteMap,
    current_dir: &str,
    name: &str,
) -> Result<()> {
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

// Generate a unique name based on the host name
pub fn generate_unique_name(
    host: &str,
    cache: &crate::cache::RemoteMap,
    current_dir: &str,
) -> String {
    // Extract username@hostname or just hostname
    let base_name = host.split(':').next().unwrap_or(host);

    // If there are no entries for this directory or no entries with this base name, use it as is
    if !cache.contains_key(current_dir) || !cache[current_dir].iter().any(|e| e.name == base_name) {
        return base_name.to_string();
    }

    // Find the highest index used for this base name
    let mut highest_index = 0;
    for entry in &cache[current_dir] {
        if entry.name == base_name {
            highest_index = 1;
        } else if entry.name.starts_with(&format!("{}_", base_name)) {
            if let Ok(index) = entry.name[base_name.len() + 1..].parse::<usize>() {
                highest_index = highest_index.max(index + 1);
            }
        }
    }

    // Return the base name with the next available index
    format!("{}_{}", base_name, highest_index)
}
