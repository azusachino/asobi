//! The supported 0.6 storage provider: one local, bundled-SQLite database.

mod sqlite;

pub use sqlite::SqliteStore;
pub type Storage = SqliteStore;
