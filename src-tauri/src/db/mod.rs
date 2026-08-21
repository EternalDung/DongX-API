use std::path::Path;
use std::str::FromStr;
use std::time::Duration;

use sqlx::sqlite::{
    SqliteConnectOptions, SqliteJournalMode, SqlitePoolOptions, SqliteSynchronous,
};
use sqlx::SqlitePool;

/// Initialize the SQLite connection pool and run migrations.
///
/// Java mental model:
/// - SqliteConnectOptions ≈ JDBC url + connection props (pragmas)
/// - SqlitePoolOptions    ≈ HikariCP config (max/min connections, acquire timeout)
/// - SqlitePool           ≈ HikariDataSource: internally Arc'd, clone() shares
///   the same pool, no extra Mutex/Arc wrapping needed.
pub async fn init_pool(db_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    // Ensure the parent directory exists (e.g. %APPDATA%/com.dongx.app/)
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| {
            tracing::error!("Failed to create db directory: {}", e);
            sqlx::Error::Io(e)
        })?;
    }

    // --- Connection-level options (per connection, like JDBC url params) ---
    let conn_opts = SqliteConnectOptions::from_str(&format!(
        "sqlite://{}",
        db_path.display()
    ))?
    .create_if_missing(true)          // equivalent to ?mode=rwc
    .journal_mode(SqliteJournalMode::Wal) // WAL: concurrent reads while writing
    .synchronous(SqliteSynchronous::Normal) // good perf/safety balance for WAL
    .foreign_keys(true)               // enforce FK constraints per connection
    .busy_timeout(Duration::from_secs(5));  // wait instead of failing on lock contention

    // --- Pool-level options (like HikariCP maximumPoolSize etc.) ---
    // SQLite is single-writer: a small pool is correct. Extra connections
    // only serve concurrent readers (WAL mode).
    let pool = SqlitePoolOptions::new()
        .max_connections(5)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(conn_opts)
        .await?;

    // Run embedded migrations (compile-time checked, from src-tauri/migrations/)
    sqlx::migrate!("./migrations").run(&pool).await?;

    tracing::info!(
        "SQLite pool initialized at {} (WAL mode, max 5 connections)",
        db_path.display()
    );
    Ok(pool)
}

/// Gracefully close the pool on app shutdown.
pub async fn close_pool(pool: SqlitePool) {
    pool.close().await;
}
