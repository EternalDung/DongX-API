use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::{AppHandle, State};

use crate::db::repository::{settings as settings_repo, stats};
use crate::error::AppResult;
use crate::security::rate_limit::RateLimiterState;
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
    pub enable_rate_limit: Option<bool>,
    pub rate_limit_rpm: Option<i32>,
    pub log_retention_days: Option<i32>,
    pub log_raw_body: Option<bool>,
    pub security_enabled: Option<bool>,
    pub security_mode: Option<String>,
    pub security_scan_unicode: Option<bool>,
    pub security_scan_tools: Option<bool>,
    pub security_scan_network: Option<bool>,
    pub security_scan_response: Option<bool>,
    pub security_redact_secrets: Option<bool>,
    pub security_block_on_critical: Option<bool>,
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
    "enable_rate_limit": false,
    "rate_limit_rpm": 60,
    "log_retention_days": 30,
    "log_raw_body": false,
    "security_enabled": true,
    "security_mode": "audit",
    "security_scan_unicode": true,
    "security_scan_tools": true,
    "security_scan_network": true,
    "security_scan_response": false,
    "security_redact_secrets": false,
    "security_block_on_critical": false
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

    // 变更是否触及数据面缓存覆盖的设置（日志体 / 重试 / 安全）。
    // 必须在下面的 push! 之前算完：push! 用 `if let Some(v) = update.$field`
    // 会把 Option<String> 字段（如 security_mode）移出 update，之后再读取就是「移动后借用」。
    let touches_cache = update.log_raw_body.is_some()
        || update.retry_enabled.is_some()
        || update.retry_times.is_some()
        || update.security_enabled.is_some()
        || update.security_mode.is_some()
        || update.security_scan_unicode.is_some()
        || update.security_scan_tools.is_some()
        || update.security_scan_network.is_some()
        || update.security_scan_response.is_some()
        || update.security_redact_secrets.is_some()
        || update.security_block_on_critical.is_some();

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
    push!(enable_rate_limit, "enable_rate_limit");
    push!(rate_limit_rpm, "rate_limit_rpm");
    push!(log_retention_days, "log_retention_days");
    push!(log_raw_body, "log_raw_body");
    push!(security_enabled, "security_enabled");
    push!(security_mode, "security_mode");
    push!(security_scan_unicode, "security_scan_unicode");
    push!(security_scan_tools, "security_scan_tools");
    push!(security_scan_network, "security_scan_network");
    push!(security_scan_response, "security_scan_response");
    push!(security_redact_secrets, "security_redact_secrets");
    push!(security_block_on_critical, "security_block_on_critical");

    if !entries.is_empty() {
        settings_repo::upsert_many(&state.db, &entries).await?;
    }

    // 自启动：保存即应用（开关与系统登录项保持同步）
    if let Some(v) = update.auto_start {
        apply_autostart(&app, v);
    }
    // 关闭到托盘：更新运行态标志，供 on_window_event 同步读取
    if let Some(v) = update.close_to_tray {
        *state.close_to_tray.lock().unwrap() = v;
    }

    // 限流：开关或 RPM 变更时整体重建运行态限速器（RateLimiter 不支持运行时改限额，
    // 故以整体替换方式生效；未提供的字段回退读取已存储值）。
    if update.enable_rate_limit.is_some() || update.rate_limit_rpm.is_some() {
        let enabled = if let Some(v) = update.enable_rate_limit {
            v
        } else {
            settings_repo::get(&state.db, "enable_rate_limit")
                .await
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str::<bool>(&s).ok())
                .unwrap_or(false)
        };
        let rpm = if let Some(v) = update.rate_limit_rpm {
            v.max(1) as u32
        } else {
            settings_repo::get(&state.db, "rate_limit_rpm")
                .await
                .ok()
                .flatten()
                .and_then(|s| serde_json::from_str::<i32>(&s).ok())
                .unwrap_or(60)
                .max(1) as u32
        };
        *state.rate_limiter.lock().unwrap() = RateLimiterState::new(enabled, rpm);
    }

    // 仅当本次变更涉及缓存覆盖的设置/规则时重建（见上方 touches_cache 的取值时机说明）。
    if touches_cache {
        state.reload_settings_cache().await;
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
