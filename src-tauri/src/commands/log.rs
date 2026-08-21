use serde::{Deserialize, Serialize};
use crate::error::AppResult;

/// Log query filters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct LogQuery {
    pub channel_name: Option<String>,
    pub model: Option<String>,
    pub status_code: Option<i32>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

/// List request logs with optional filters
#[tauri::command]
pub async fn list_logs(query: Option<LogQuery>) -> AppResult<Vec<serde_json::Value>> {
    let _ = query;
    // TODO: Query from database with filters
    Ok(vec![])
}

/// Get a single log entry with full request/response body
#[tauri::command]
pub async fn get_log_detail(id: String) -> AppResult<serde_json::Value> {
    let _ = id;
    // TODO: Query from database
    Ok(serde_json::json!({}))
}

/// Clear all logs (or logs older than N days)
#[tauri::command]
pub async fn clear_logs(older_than_days: Option<i32>) -> AppResult<()> {
    let _ = older_than_days;
    // TODO: Delete from database
    Ok(())
}
