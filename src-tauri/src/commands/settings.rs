use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::repository::{settings as settings_repo, stats};
use crate::error::AppResult;
use crate::AppState;

/// Settings partial update payload (mirrors frontend SettingsUpdate).
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

/// Built-in defaults merged under stored values.
const DEFAULTS: &str = r#"{
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
}"#;

/// Get all settings (stored values override built-in defaults).
#[tauri::command]
pub async fn get_settings(state: State<'_, Arc<AppState>>) -> AppResult<serde_json::Value> {
    let mut obj: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(DEFAULTS).unwrap_or_default();

    let rows = settings_repo::get_all(&state.db).await?;
    for row in rows {
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&row.value) {
            obj.insert(row.key, v);
        }
    }

    Ok(serde_json::Value::Object(obj))
}

/// Update settings (partial update — only provided fields are written).
#[tauri::command]
pub async fn update_settings(
    update: SettingsUpdate,
    state: State<'_, Arc<AppState>>,
) -> AppResult<serde_json::Value> {
    let mut entries: Vec<(String, String)> = Vec::new();

    macro_rules! push {
        ($field:ident, $key:literal) => {
            if let Some(v) = update.$field {
                entries.push(($key.to_string(), serde_json::to_string(&v)?));
            }
        };
    }
    push!(server_port, "server_port");
    push!(server_host, "server_host");
    push!(ui_theme, "ui_theme");
    push!(ui_language, "ui_language");
    push!(minimize_to_tray, "minimize_to_tray");
    push!(close_to_tray, "close_to_tray");
    push!(auto_start, "auto_start");
    push!(retry_enabled, "retry_enabled");
    push!(retry_times, "retry_times");
    push!(log_retention_days, "log_retention_days");
    push!(log_raw_body, "log_raw_body");
    push!(security_enabled, "security_enabled");
    push!(security_mode, "security_mode");

    if !entries.is_empty() {
        settings_repo::upsert_many(&state.db, &entries).await?;
    }

    Ok(serde_json::json!({ "status": "updated" }))
}

/// Get dashboard statistics (aggregated from request_logs + channels + gateway_keys).
#[tauri::command]
pub async fn get_dashboard_stats(state: State<'_, Arc<AppState>>) -> AppResult<serde_json::Value> {
    let s = stats::dashboard(&state.db).await?;
    Ok(serde_json::to_value(&s)?)
}
