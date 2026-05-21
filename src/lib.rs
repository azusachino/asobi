//! Rosemary — Knowledge Graph & Document Memory

// Graph (libSQL) + optional Document/Vector (LanceDB) tiers
#[cfg(feature = "documents")]
pub mod chunk;
#[cfg(feature = "documents")]
pub mod compact;
pub mod db;
pub mod digest;
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
pub mod paths;
pub use anyhow::{Result, anyhow, bail};
pub use tokio::task::JoinHandle;
