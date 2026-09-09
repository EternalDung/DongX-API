//! MCP（Model Context Protocol）服务：协议实现 + 工具 + 状态查询。
//!
//! **挂载方式**：本模块不自己监听端口。路由由 [`crate::services::mcp::McpService`]
//! 注册进网关（`server::router::create_router` → `ServiceRegistry::merge_into`），
//! 因此 MCP 端点与网关同源，默认 `http://127.0.0.1:9842/mcp`。
//!
//! 这样做的好处是端点自动跟随网关：改 `server_host` / `server_port` 后重启网关，
//! MCP 端点同步变化，不需要为 MCP 单独维护端口设置与生命周期（早期版本曾为 MCP
//! 单开 8777 listener，两者独立，改端口不会联动，且端口冲突会静默失败）。
//!
//! 授权范围：KB 是否对外可见完全由 KB 上 `mcp_exposed` 字段决定，
//! `tools::dispatch` 内部统一把关（见 [`crate::mcp::tools::require_exposed_kb`]）。
//! 注意这是**授权范围**而非**身份认证**——当前端点无入站鉴权，仅靠绑定
//! 127.0.0.1 限制为本机可访问。
//!
//! Wire：
//! - JSON-RPC 2.0 over HTTP，POST /mcp；
//! - 方法集：initialize / notifications/initialized / ping / tools/list / tools/call。
//! - 协议细节见 [`crate::mcp::protocol`]；工具实现见 [`crate::mcp::tools`]。

pub mod protocol;
pub mod router;
pub mod tools;
pub mod wiki_tools;

use serde::Serialize;

use crate::AppState;

/// MCP 端点路径（挂在网关根下，与 `router::mcp_service_routes` 的路由一致）。
pub const ENDPOINT_PATH: &str = "/mcp";

/// MCP 服务监听地址（与网关同源，取网关运行态）。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpListenAddr {
    pub host: String,
    pub port: u16,
}

/// 由网关监听地址拼出 MCP 端点 URL。
pub fn mcp_url(host: &str, port: u16) -> String {
    format!("http://{}:{}{}", host, port, ENDPOINT_PATH)
}

/// MCP 服务运行态快照（供 UI / 命令消费）。
///
/// `running` / `bind_addr` 反映的是**网关**的运行态——MCP 与网关同源，
/// 网关没起来则 MCP 端点同样不可达。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpStatus {
    pub running: bool,
    /// 当前实际监听的地址；网关未运行时为 None。
    pub bind_addr: Option<McpListenAddr>,
    /// 给 MCP client 用的 URL 字符串（= `mcp_url(bind_addr)`）。
    pub endpoint: Option<String>,
    /// 当前暴露的工具数量（与 `tools::tool_specs().len()` 一致）。
    pub tools_count: usize,
}

pub fn get_mcp_status_for(state: &AppState) -> McpStatus {
    let addr = state.server.current();
    McpStatus {
        running: state.server.is_running(),
        endpoint: addr.as_ref().map(|a| mcp_url(&a.host, a.port)),
        bind_addr: addr.map(|a| McpListenAddr {
            host: a.host,
            port: a.port,
        }),
        tools_count: tools::tool_specs().len(),
    }
}
