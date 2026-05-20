//! Rosemary Personal Knowledge Base v0.2.0

// Knowledge Base Modules
pub mod chunk;
pub mod compact;
pub mod db;
pub mod digest;
pub mod embed;
pub mod ingest;
pub mod kb;
pub mod recall;
pub mod vector;

// Async Masterclass Modules
pub mod observability;
pub mod queue;
pub mod shutdown;

#[cfg(test)]
mod tests;

// Shared Utilities
pub use anyhow::{Result, anyhow, bail};
pub use tokio::task::JoinHandle;
