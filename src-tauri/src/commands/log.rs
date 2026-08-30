use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::repository::{request_logs, security_findings, LogFilter};
use crate::error::{AppError, AppResult};
use crate::AppState;

/// Log query filters (mirrors frontend LogQuery).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct LogQuery {
    pub keyword: Option<String>,
    pub channel_name: Option<String>,
    pub model: Option<String>,
    pub status_code: Option<i32>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

/// List request logs (slim rows, no bodies) with optional filters + pagination.
#[tauri::command]
pub async fn list_logs(
    query: Option<LogQuery>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<Vec<serde_json::Value>> {
    let q = query.unwrap_or_default();
    let filter = LogFilter {
        keyword: q.keyword,
        channel_name: q.channel_name,
        model: q.model,
        status_code: q.status_code,
        start_time: q.start_time,
        end_time: q.end_time,
        page: q.page.unwrap_or(1),
        page_size: q.page_size.unwrap_or(20),
    };
    let rows = request_logs::list_filtered(&state.db, &filter).await?;
    Ok(rows
        .into_iter()
        .map(|r| serde_json::to_value(&r).unwrap_or_default())
        .collect())
}

/// Get a single log entry with full request/response body.
#[tauri::command]
pub async fn get_log_detail(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> AppResult<serde_json::Value> {
    let row = request_logs::get_detail(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("日志 {} 不存在", id)))?;
    Ok(serde_json::to_value(&row)?)
}

/// Get security audit findings for a log entry (severity-desc, all of them).
///
/// 与汇总字段（risk_level/score/action）的区别：汇总只看最高等级，
/// 这里返回**全部**命中明细——MAX 只决定动作，不决定展示条数。
/// 前端仅在 `risk_score > 0` 时调用，避免列表页 N+1 查询。
#[tauri::command]
pub async fn get_log_security_findings(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> AppResult<Vec<serde_json::Value>> {
    let rows = security_findings::list_by_log(&state.db, &id).await?;
    Ok(rows
        .into_iter()
        .map(|r| serde_json::to_value(&r).unwrap_or_default())
        .collect())
}

/// Clear all logs, or only those older than N days (None = clear all).
#[tauri::command]
pub async fn clear_logs(
    older_than_days: Option<i32>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    request_logs::clear(&state.db, older_than_days).await?;
    Ok(())
}

/// Delete a single log entry by id.
#[tauri::command]
pub async fn delete_log(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> AppResult<u64> {
    let n = request_logs::delete(&state.db, &id).await?;
    if n == 0 {
        return Err(AppError::NotFound(format!("日志 {} 不存在", id)));
    }
    Ok(n)
}
