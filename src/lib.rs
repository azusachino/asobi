//! async rust in practice

// module declaration
pub mod queue;
mod tests;

// usage declaration
pub use anyhow::{Result, anyhow, bail};
pub use tokio::task::{JoinHandle};
