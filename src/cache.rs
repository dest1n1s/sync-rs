use anyhow::{Context, Result};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs::{self, File};
use std::path::{Path, PathBuf};

use crate::config::RemoteEntry;

pub type RemoteMap = HashMap<String, Vec<RemoteEntry>>;

#[derive(Debug, Serialize, Deserialize)]
pub struct VersionedCache {
    pub version: String,
    pub entries: RemoteMap,
}

// Legacy cache format for migration (v0)
#[derive(Debug, Deserialize)]
struct LegacyCacheEntry {
    remote_host: String,
    remote_dir: String,
    #[serde(default)]
    override_paths: Vec<String>,
    #[serde(default)]
    post_sync_command: Option<String>,
}

type LegacyCache = HashMap<String, LegacyCacheEntry>;

// Migration trait - all migrators must implement this
trait CacheMigrator {
    fn version(&self) -> &str;
    fn can_migrate(&self, data: &[u8]) -> bool;
    fn migrate(&self, data: &[u8], cache_path: &Path) -> Result<RemoteMap>;
}

// Migrator for legacy cache format (no version field)
struct LegacyMigrator;

impl CacheMigrator for LegacyMigrator {
    fn version(&self) -> &str {
        "0.1.0"
    }

    fn can_migrate(&self, data: &[u8]) -> bool {
        // Try parsing as legacy format
        serde_json::from_slice::<LegacyCache>(data).is_ok()
    }

    fn migrate(&self, data: &[u8], cache_path: &Path) -> Result<RemoteMap> {
        println!("Migrating from legacy cache format...");

        let legacy_cache: LegacyCache =
            serde_json::from_slice(data).context("Failed to parse legacy cache")?;

        let migrated = self.convert_legacy_cache(legacy_cache);

        // Backup the old cache file
        let backup_path = cache_path.with_extension("json.bak");
        fs::copy(cache_path, &backup_path).context("Failed to backup legacy cache file")?;

        println!(
            "Cache migration complete. Backup saved at {:?}",
            backup_path
        );

        Ok(migrated)
    }
}

impl LegacyMigrator {
    fn convert_legacy_cache(&self, legacy_cache: LegacyCache) -> RemoteMap {
        let mut new_cache = RemoteMap::new();

        for (dir, entry) in legacy_cache {
            let name = format!(
                "{}_{}",
                entry.remote_host,
                entry.remote_dir.replace('/', "_")
            );
            let remote_entry = RemoteEntry {
                name,
                remote_host: entry.remote_host,
                remote_dir: entry.remote_dir,
                override_paths: entry.override_paths,
                post_sync_command: entry.post_sync_command,
                preferred: false,
                ignore_patterns: Vec::new(),
            };

            new_cache.insert(dir, vec![remote_entry]);
        }

        new_cache
    }
}

// Migration registry
pub struct MigrationManager {
    migrators: Vec<Box<dyn CacheMigrator>>,
    current_version: String,
}

impl MigrationManager {
    pub fn new(current_version: String) -> Self {
        let mut manager = Self {
            migrators: Vec::new(),
            current_version,
        };

        // Register all migrators in chronological order
        manager.register_migrator(Box::new(LegacyMigrator));

        manager
    }

    fn register_migrator(&mut self, migrator: Box<dyn CacheMigrator>) {
        self.migrators.push(migrator);
    }

    pub fn read_cache(&self, cache_path: &Path) -> Result<RemoteMap> {
        if !cache_path.exists() {
            return Ok(RemoteMap::new());
        }

        // Read the cache file
        let data = fs::read(cache_path).context("Failed to read cache file")?;

        // Try parsing as versioned cache first
        if let Ok(versioned_cache) = serde_json::from_slice::<VersionedCache>(&data) {
            println!("Using cache version {}", versioned_cache.version);

            // If already at current version, use as is
            if versioned_cache.version == self.current_version {
                return Ok(versioned_cache.entries);
            }

            // Future: Add specific version-to-version migrations here
            println!(
                "Cache version {} migrated to {}",
                versioned_cache.version, self.current_version
            );
            return Ok(versioned_cache.entries);
        }

        // Try each migrator in sequence
        for migrator in &self.migrators {
            if migrator.can_migrate(&data) {
                println!("Found compatible migrator: {}", migrator.version());
                return migrator.migrate(&data, cache_path);
            }
        }

        // If no migrator works, log and return empty cache
        eprintln!("Warning: Could not migrate cache, creating new one");
        Ok(RemoteMap::new())
    }

    pub fn save_cache(&self, cache_path: &Path, entries: &RemoteMap) -> Result<()> {
        let cache = VersionedCache {
            version: self.current_version.clone(),
            entries: entries.clone(),
        };

        let file = File::create(cache_path).context("Failed to create cache file")?;
        serde_json::to_writer_pretty(file, &cache).context("Failed to write cache file")
    }
}

pub fn get_cache_path() -> Result<PathBuf> {
    let config_dir = dirs::config_dir().context("Failed to find config directory")?;
    let cache_dir = config_dir.join("sync-rs");
    if !cache_dir.exists() {
        fs::create_dir_all(&cache_dir).context("Failed to create cache directory")?;
    }
    Ok(cache_dir.join("cache.json"))
}
