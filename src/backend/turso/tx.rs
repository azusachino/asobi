use std::future::Future;
use std::path::Path;
use std::pin::Pin;
use std::time::Duration;

use anyhow::{Context, Result};
use turso::{Builder, Connection, Database, Error};

const MAX_TRANSACTION_RETRIES: usize = 8;
const MAX_OPEN_RETRIES: usize = 32;

pub async fn open_local(path: &Path) -> Result<(Database, Connection)> {
    let path = path
        .to_str()
        .context("Turso database path must be valid UTF-8")?;
    let mut retries = 0;
    let db = loop {
        match Builder::new_local(path)
            .experimental_multiprocess_wal(true)
            .experimental_index_method(true)
            .build()
            .await
        {
            Ok(db) => break db,
            Err(error)
                if (is_retryable(&error) || error.to_string().contains("Locking error"))
                    && retries < MAX_OPEN_RETRIES =>
            {
                retries += 1;
                tokio::time::sleep(Duration::from_millis(50 * (retries as u64).min(5))).await;
            }
            Err(error) => {
                return Err(error)
                    .with_context(|| format!("failed to open Turso database at '{}'", path));
            }
        }
    };
    let conn = db
        .connect()
        .context("failed to connect to Turso database")?;
    Ok((db, conn))
}

pub fn is_retryable(error: &Error) -> bool {
    matches!(error, Error::Busy(_) | Error::BusySnapshot(_))
}

pub async fn immediate_transaction<F>(
    conn: &Connection,
    mut operation: F,
) -> std::result::Result<(), Error>
where
    F: for<'a> FnMut(
        &'a Connection,
    ) -> Pin<Box<dyn Future<Output = std::result::Result<(), Error>> + 'a>>,
{
    for attempt in 0..=MAX_TRANSACTION_RETRIES {
        let result = async {
            conn.execute("BEGIN IMMEDIATE", ()).await?;
            operation(conn).await?;
            conn.execute("COMMIT", ()).await.map(|_| ())
        }
        .await;

        match result {
            Ok(()) => return Ok(()),
            Err(error) if is_retryable(&error) && attempt < MAX_TRANSACTION_RETRIES => {
                let _ = conn.execute("ROLLBACK", ()).await;
                tokio::time::sleep(Duration::from_millis(1 << attempt.min(6))).await;
            }
            Err(error) => {
                let _ = conn.execute("ROLLBACK", ()).await;
                return Err(error);
            }
        }
    }

    unreachable!("transaction retry loop always returns")
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[tokio::test]
    async fn immediate_transaction_commits_and_rolls_back() {
        let dir = tempdir().unwrap();
        let (_db, conn) = open_local(&dir.path().join("transaction.db"))
            .await
            .unwrap();
        conn.execute("CREATE TABLE values_table (value TEXT NOT NULL)", ())
            .await
            .unwrap();

        immediate_transaction(&conn, |conn| {
            Box::pin(async move {
                conn.execute("INSERT INTO values_table (value) VALUES (?1)", ("kept",))
                    .await
                    .map(|_| ())
            })
        })
        .await
        .unwrap();

        let error = immediate_transaction(&conn, |conn| {
            Box::pin(async move {
                conn.execute("INSERT INTO missing_table VALUES (?1)", ("discarded",))
                    .await
                    .map(|_| ())
            })
        })
        .await
        .unwrap_err();
        assert!(!is_retryable(&error));

        let mut rows = conn
            .query("SELECT value FROM values_table", ())
            .await
            .unwrap();
        assert_eq!(
            rows.next()
                .await
                .unwrap()
                .unwrap()
                .get::<String>(0)
                .unwrap(),
            "kept"
        );
        assert!(rows.next().await.unwrap().is_none());
    }
}
