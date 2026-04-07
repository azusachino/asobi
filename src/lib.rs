//! async rust in practice

// module declaration
pub mod observability;
pub mod queue;
pub mod shutdown;
mod tests;

// usage declaration
pub use anyhow::{Result, anyhow, bail};
pub use tokio::task::JoinHandle;
