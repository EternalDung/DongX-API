use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::db::repository::{audit_events, AuditFilter};
use crate::error::AppResult;
use crate::AppState;

/// Audit event query filters (mirrors frontend AuditQuery).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub struct AuditQuery {
    pub severity: Option<String>,
    pub event_type: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

/// List audit events with optional filters + pagination.
#[tauri::command]
pub async fn list_audit_events(
    query: Option<AuditQuery>,
    state: State<'_, Arc<AppState>>,
) -> AppResult<Vec<serde_json::Value>> {
    let q = query.unwrap_or_default();
    let filter = AuditFilter {
        severity: q.severity,
        event_type: q.event_type,
        start_time: q.start_time,
        end_time: q.end_time,
        page: q.page.unwrap_or(1),
        page_size: q.page_size.unwrap_or(20),
    };
    let rows = audit_events::list_filtered(&state.db, &filter).await?;
    Ok(rows
        .into_iter()
        .map(|r| serde_json::to_value(&r).unwrap_or_default())
        .collect())
}
