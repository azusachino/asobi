//! Rosemary — Knowledge Graph & Document Memory

// Graph (libSQL) + Document/Vector (LanceDB) tiers
pub mod chunk;
pub mod compact;
pub mod db;
pub mod digest;
pub mod embed;
pub mod ingest;
pub mod init;
pub mod recall;
pub mod vector;

// Async Masterclass Modules
pub mod observability;
pub mod queue;
pub mod shutdown;

#[cfg(test)]
mod tests;

// Shared Utilities
pub mod mcp;
pub mod paths;
pub use anyhow::{Result, anyhow, bail};
pub use tokio::task::JoinHandle;
