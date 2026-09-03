//! MCP 服务：把 RAG 检索 / 问答能力以 MCP 协议暴露给外部 AI Agent。
//!
//! 通过 [`Service`] trait 注册进网关路由表，而不是另起 listener：
//!
//! - **端点与网关同源**（默认 `http://127.0.0.1:9842/mcp`），不再需要
//!   `mcp_host` / `mcp_port` 两个独立设置项；
//! - **改网关端口后自动跟随**：重启网关 → MCP 端点同步更新，无需额外联动代码
//!   （因为两者共用同一个 `TcpListener`）；
//! - 禁用走统一的服务开关（持久化到 `service.mcp.enabled`）。
//!
//! 协议与工具实现见 [`crate::mcp`]。

use super::{Service, ServiceStatus};
use crate::AppState;
use async_trait::async_trait;
use axum::Router;
use serde_json::json;
use tauri::AppHandle;

pub struct McpService;

#[async_trait]
impl Service for McpService {
    fn id(&self) -> &'static str {
        "mcp"
    }

    fn name(&self) -> &'static str {
        "MCP Server"
    }

    fn description(&self) -> &'static str {
        "Model Context Protocol 服务：把知识库检索与问答暴露为 MCP 工具，供 Claude Desktop / Cursor / 自定义 Agent 接入（端点与网关同源）"
    }

    fn enabled(&self) -> bool {
        true
    }

    async fn status(&self, state: &AppState) -> ServiceStatus {
        // 对外可见的 KB 数量：只有 mcp_exposed=1 且未删除的 KB 会出现在
        // `list_knowledge_bases` 里，与 tools::require_exposed_kb 的判定保持一致。
        let exposed: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_bases WHERE mcp_exposed = 1 AND status = 1")
                .fetch_one(&state.db)
                .await
                .unwrap_or(0);

        let tools = crate::mcp::tools::tool_specs()
            .iter()
            .map(|t| {
                json!({
                    "name": t.name,
                    "description": t.description,
                })
            })
            .collect::<Vec<_>>();

        ServiceStatus {
            id: self.id().to_string(),
            name: self.name().to_string(),
            description: self.description().to_string(),
            enabled: self.enabled(),
            running: true,
            stats: json!({
                "exposed_knowledge_bases": exposed,
                "tools_count": tools.len(),
                "tools": tools,
            }),
        }
    }

    fn routes(&self) -> Router<AppHandle> {
        crate::mcp::router::mcp_service_routes()
    }
}
