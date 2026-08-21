use serde::{Deserialize, Serialize};
use crate::error::AppResult;

/// Settings update payload (partial update)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SettingsUpdate {
    pub server_port: Option<u16>,
    pub server_host: Option<String>,
    pub ui_theme: Option<String>,
    pub ui_language: Option<String>,
    pub minimize_to_tray: Option<bool>,
    pub close_to_tray: Option<bool>,
    pub auto_start: Option<bool>,
    pub retry_enabled: Option<bool>,
    pub retry_times: Option<i32>,
    pub log_retention_days: Option<i32>,
    pub log_raw_body: Option<bool>,
    pub security_enabled: Option<bool>,
    pub security_mode: Option<String>,
}

/// Get all settings
#[tauri::command]
pub async fn get_settings() -> AppResult<serde_json::Value> {
    // TODO: Query from database
    Ok(serde_json::json!({
        "server_port": 9842,
        "server_host": "127.0.0.1",
        "ui_theme": "system",
        "ui_language": "zh-CN",
        "minimize_to_tray": true,
        "close_to_tray": true,
        "auto_start": false,
        "retry_enabled": true,
        "retry_times": 3,
        "log_retention_days": 30,
        "log_raw_body": false,
        "security_enabled": true,
        "security_mode": "balanced"
    }))
}

/// Update settings (partial update)
#[tauri::command]
pub async fn update_settings(update: SettingsUpdate) -> AppResult<serde_json::Value> {
    let _ = update;
    // TODO: Update database
    Ok(serde_json::json!({ "status": "updated" }))
}

/// Get dashboard statistics
#[tauri::command]
pub async fn get_dashboard_stats() -> AppResult<serde_json::Value> {
    // TODO: Aggregate from request_logs and channels
    Ok(serde_json::json!({
        "today_requests": 0,
        "today_total_tokens": 0,
        "active_channels": 0,
        "avg_latency_ms": 0,
        "total_channels": 0,
        "total_api_keys": 0,
        "total_requests": 0,
        "total_tokens": 0
    }))
}
