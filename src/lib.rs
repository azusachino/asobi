//! Miku — Knowledge Graph & Document Memory

// Graph + optional Document/Vector (libSQL) tiers
pub mod backup;
#[cfg(feature = "documents")]
pub mod chunk;
#[cfg(feature = "documents")]
pub mod compact;
pub mod constant;
pub mod db;
#[cfg(feature = "documents")]
pub mod embed;
#[cfg(feature = "documents")]
pub mod ingest;
pub mod init;
#[cfg(feature = "documents")]
pub mod recall;
#[cfg(feature = "documents")]
pub mod vector;

// Shared Utilities
pub mod mcp;
pub mod normalize;
pub mod paths;
pub mod skills;
pub use anyhow::{Result, anyhow, bail};
pub use tokio::task::JoinHandle;
