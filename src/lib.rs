//! Asobi — Knowledge Graph & Document Memory

// Graph + optional Document/Vector (libSQL) tiers
pub mod api;
pub mod application;
#[cfg(feature = "documents")]
pub mod chunk;
#[cfg(feature = "documents")]
pub mod compact;
#[cfg(feature = "documents")]
pub mod embed;
pub mod storage;
// Shared Utilities
pub mod frontmatter;
#[cfg(feature = "documents")]
pub mod ingest;
pub mod init;
pub mod model;
pub mod normalize;
pub mod paths;
#[cfg(feature = "documents")]
pub mod recall;
pub mod response;
pub mod skills;
pub use anyhow::{Result, anyhow, bail};
pub use tokio::task::JoinHandle;
