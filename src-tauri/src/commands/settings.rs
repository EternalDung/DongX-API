use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::db::repository::{audit_events, settings as settings_repo, stats};
use crate::error::AppResult;
use crate::AppState;

/// 应用开机自启动设置。
///
/// 同步 API（tauri-plugin-autostart 的 enable/disable 本身不是 async）。
/// 失败仅告警，不阻断主流程（例如 dev 模式下注册表写入可能受限）。
pub fn apply_autostart(app: &AppHandle, enable: bool) {
    use tauri_plugin_autostart::ManagerExt;
    let mgr = app.autolaunch();
    let res = if enable { mgr.enable() } else { mgr.disable() };
    if let Err(e) = res {
        tracing::warn!(
            "设置开机自启动失败 (enable={}): {}",
            enable,
            e
        );
    }
}

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

/// Load settings: built-in defaults merged with stored values.
///
/// `pub` 以便 `setup` 在启动时读取自启动 / 关闭到托盘等设置并立即生效。
pub async fn load_all_settings(pool: &sqlx::SqlitePool) -> AppResult<serde_json::Value> {
    let mut obj: serde_json::Map<String, serde_json::Value> =
        serde_json::from_str(DEFAULTS).unwrap_or_default();

    let rows = settings_repo::get_all(pool).await?;
    for row in rows {
        // 存储值是 JSON 编码后的字符串；解析失败（脏数据）时保留默认值，
        // 而不是把整份 settings 打断。
        if let Ok(v) = serde_json::from_str::<serde_json::Value>(&row.value) {
            obj.insert(row.key, v);
        }
    }

    Ok(serde_json::Value::Object(obj))
}

/// Get all settings (stored values override built-in defaults).
#[tauri::command]
pub async fn get_settings(state: State<'_, Arc<AppState>>) -> AppResult<serde_json::Value> {
    load_all_settings(&state.db).await
}

/// Update settings (partial update — only provided fields are written).
#[tauri::command]
pub async fn update_settings(
    app: AppHandle,
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

        // 审计：配置变更（记录本次改动了哪些键，便于事后追溯）。
        let changed: Vec<&str> = entries.iter().map(|(k, _)| k.as_str()).collect();
        let meta = serde_json::to_string(&serde_json::json!({ "changed": changed })).ok();
        let _ = audit_events::insert(
            &state.db,
            "config_change",
            "info",
            Some("settings"),
            "配置已更新",
            meta.as_deref(),
        )
        .await;
    }

    // 自启动：保存即应用（开关与系统登录项保持同步）
    if let Some(v) = update.auto_start {
        apply_autostart(&app, v);
    }
    // 关闭到托盘：更新运行态标志，供 on_window_event 同步读取
    if let Some(v) = update.close_to_tray {
        *state.close_to_tray.lock().unwrap() = v;
    }

    // 回写后的完整 settings（不是 {status:"updated"}）：前端直接拿它做
    // setSettings，契约与 get_settings 一致，避免把状态对象覆盖成状态码。
    load_all_settings(&state.db).await
}

/// Get dashboard statistics (aggregated from request_logs + channels + gateway_keys).
#[tauri::command]
pub async fn get_dashboard_stats(state: State<'_, Arc<AppState>>) -> AppResult<serde_json::Value> {
    let s = stats::dashboard(&state.db).await?;
    Ok(serde_json::to_value(&s)?)
}
