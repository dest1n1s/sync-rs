pub mod cache;
pub mod config;
pub mod console;
pub mod ignore;
pub mod sync;
pub mod worktree;

// Re-export key types for easier external use
pub use cache::{get_cache_path, MigrationManager, RemoteMap};
pub use config::RemoteEntry;
