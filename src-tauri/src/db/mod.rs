use sqlx::SqlitePool;
use std::path::Path;

/// Initialize SQLite connection pool and run migrations
pub async fn init_pool(db_path: &Path) -> Result<SqlitePool, sqlx::Error> {
    let url = format!("sqlite://{}?mode=rwc", db_path.display());
    let pool = SqlitePool::connect(&url).await?;

    // Run embedded migrations
    sqlx::migrate!("./migrations").run(&pool).await?;

    tracing::info!("Database initialized at {}", db_path.display());
    Ok(pool)
}

/// Execute a query and return rows as serde_json::Value
pub async fn query_json(
    pool: &SqlitePool,
    sql: &str,
    args: &[serde_json::Value],
) -> Result<Vec<serde_json::Value>, sqlx::Error> {
    let mut query = sqlx::query(sql);
    for arg in args {
        query = match arg {
            serde_json::Value::Null => query.bind(None::<String>),
            serde_json::Value::Bool(b) => query.bind(b),
            serde_json::Value::Number(n) => {
                if let Some(i) = n.as_i64() {
                    query.bind(i)
                } else if let Some(f) = n.as_f64() {
                    query.bind(f)
                } else {
                    query.bind(n.to_string())
                }
            }
            serde_json::Value::String(s) => query.bind(s),
            other => query.bind(other.to_string()),
        };
    }
    let rows = query.fetch_all(pool).await?;
    Ok(rows
        .into_iter()
        .map(|row| {
            // Convert row to JSON value (simplified placeholder)
            serde_json::json!({})
        })
        .collect())
}
