use anyhow::Result;
use rosemary::observability::init_tracing;
use sqlx::sqlite::SqlitePool;
use tracing::info;

#[derive(Debug, sqlx::FromRow)]
struct User {
    id: i64,
    name: String,
    email: String,
}

#[tokio::main]
async fn main() -> Result<()> {
    // Initialize tracing
    init_tracing();

    info!("Starting SQLx SQLite example...");

    // 1. Connect to an in-memory SQLite database
    let pool = SqlitePool::connect("sqlite::memory:").await?;
    info!("Connected to in-memory SQLite database");

    // 2. Create 'users' table
    sqlx::query(
        r#"
        CREATE TABLE IF NOT EXISTS users (
            id INTEGER PRIMARY KEY AUTOINCREMENT,
            name TEXT NOT NULL,
            email TEXT NOT NULL UNIQUE
        )
        "#,
    )
    .execute(&pool)
    .await?;
    info!("Table 'users' created");

    // 3. Insert a sample user
    let user_name = "Alice";
    let user_email = "alice@example.com";

    let res = sqlx::query("INSERT INTO users (name, email) VALUES (?, ?)")
        .bind(user_name)
        .bind(user_email)
        .execute(&pool)
        .await?;

    info!("Inserted user with id: {}", res.last_insert_rowid());

    // 4. Query the user
    let user: User = sqlx::query_as("SELECT id, name, email FROM users WHERE email = ?")
        .bind(user_email)
        .fetch_one(&pool)
        .await?;

    info!(
        "Queried user: id={}, name={}, email={}",
        user.id, user.name, user.email
    );

    Ok(())
}
