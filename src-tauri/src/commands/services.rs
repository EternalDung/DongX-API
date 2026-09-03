//! 服务管理命令：暴露各业务服务的运行状态给前端（服务页 / 知识库页），
//! 并支持启用 / 禁用 / 移除操作。

use crate::services::ServiceRegistry;
use crate::AppState;
use std::sync::Arc;
use tauri::State;

/// 获取所有未移除服务的状态（UI 服务页用）。
#[tauri::command]
pub async fn list_services(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<serde_json::Value>, String> {
    let registry = ServiceRegistry::global();
    let statuses = registry.list_status(&state).await;
    Ok(statuses
        .into_iter()
        .map(|s| serde_json::to_value(s).unwrap_or_default())
        .collect())
}

/// 启用 / 禁用某个服务（持久化，重启后仍生效）。
#[tauri::command]
pub async fn set_service_enabled(
    state: State<'_, Arc<AppState>>,
    id: String,
    enabled: bool,
) -> Result<(), String> {
    ServiceRegistry::global()
        .set_enabled(&id, enabled, &state.db)
        .await
}

/// 移除（软删除）某个服务：从列表隐藏、不再挂载路由（持久化）。
#[tauri::command]
pub async fn delete_service(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    ServiceRegistry::global().remove(&id, &state.db).await
}
