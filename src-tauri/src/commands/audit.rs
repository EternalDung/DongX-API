use serde::{Deserialize, Serialize};
use crate::error::AppResult;

/// Audit event query filters
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AuditQuery {
    pub severity: Option<String>,
    pub event_type: Option<String>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
    pub page: Option<u32>,
    pub page_size: Option<u32>,
}

/// List audit events with optional filters
#[tauri::command]
pub async fn list_audit_events(query: Option<AuditQuery>) -> AppResult<Vec<serde_json::Value>> {
    let _ = query;
    // TODO: Query from database
    Ok(vec![])
}
