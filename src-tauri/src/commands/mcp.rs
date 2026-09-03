//! MCP 服务 Tauri 命令：当前只暴露 `get_mcp_status`，
//! 供前端 / 调试用。详细路由见 `crate::mcp::router`。

use std::sync::Arc;

use tauri::State;

use crate::mcp::{self, McpStatus};
use crate::AppState;

/// 获取 MCP server 运行态（监听地址 + 端点 URL + 工具数量）。
#[tauri::command]
pub fn get_mcp_status(state: State<'_, Arc<AppState>>) -> McpStatus {
    mcp::get_mcp_status_for(state.inner())
}
