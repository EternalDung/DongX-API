//! MCP HTTP 路由：单端点 POST /mcp（MCP Streamable HTTP transport），
//! 按 JSON-RPC 2.0 分发。
//!
//! 传输层遵循 2025-03-26 引入的 Streamable HTTP 规范（取代已废弃的
//! legacy HTTP+SSE 双端点）：客户端向同一 `POST /mcp` 发 JSON-RPC，并可在
//! `Accept` 头声明 `text/event-stream` 以接收 SSE 事件流形式的响应；服务端
//! 也可直接回 `application/json`。本实现为无状态模式（不持有跨请求连接）。
//!
//! 方法集（v1 最小实现）：
//!   - initialize           返回 serverInfo + capabilities
//!   - notifications/initialized  吞掉（notification 不回包）
//!   - ping                 保持连接 / 心跳
//!   - tools/list           暴露 5 个 tools
//!   - tools/call           调度工具执行；KB 未暴露等业务错误以 isError=true 文本回包
//!
//! 两种挂载方式共用同一套分发逻辑（[`handle_http`] → [`handle_request`]）：
//!   - [`mcp_service_routes`]：状态 `tauri::AppHandle`，由 `services::mcp::McpService`
//!     合并进网关路由（**生产路径**）。端点与网关同源
//!     （默认 `http://127.0.0.1:9842/mcp`），改网关端口后自动跟随。
//!   - [`create_mcp_router`]：`#[cfg(test)]` 门控，状态 `Arc<SqlitePool>`，
//!     让 HTTP 层测试无需 mock Tauri 运行时。

use std::sync::Arc;

use axum::body::Body;
use axum::extract::State;
use axum::http::{header, HeaderMap, StatusCode};
use axum::response::{IntoResponse, Json, Response};
use axum::routing::post;
use axum::Router;
use serde_json::json;
use sqlx::SqlitePool;
use tauri::{AppHandle, Manager};

use crate::mcp::protocol::{JsonRpcError, JsonRpcRequest, JsonRpcResponse};
use crate::mcp::tools::{self, ToolCallArgs};
use crate::AppState;

/// 网关内嵌路由（由 `services::mcp::McpService` 注册，状态类型为 `AppHandle`）。
pub fn mcp_service_routes() -> Router<AppHandle> {
    Router::new()
        // 主端点：MCP Streamable HTTP（POST = JSON-RPC）
        .route("/mcp", post(mcp_endpoint_gateway))
        // 尾斜杠变体：部分客户端会发 /mcp/
        .route("/mcp/", post(mcp_endpoint_gateway))
        // 调试用（不强制 JSON-RPC 包装）：列出可用 tool 元数据
        .route("/mcp/tools", axum::routing::get(list_tools_debug))
}

/// 单元测试用路由：状态类型为 `Arc<SqlitePool>`，构造 axum router 时不依赖
/// Tauri 运行时（无需 mock `AppHandle`），因此 HTTP 层测试走这个入口。
///
/// 生产构建不编译此函数（并入网关后已无独立 listener）。
#[cfg(test)]
pub fn create_mcp_router(pool: SqlitePool) -> Router {
    Router::new()
        .route("/mcp", post(mcp_endpoint))
        // 调试用（不强制 JSON-RPC 包装）：列出可用 tool 元数据
        .route("/mcp/tools", axum::routing::get(list_tools_debug))
        .with_state(Arc::new(pool))
}

/// 网关版入口：从 `AppHandle` 托管的 `AppState` 取 pool。
async fn mcp_endpoint_gateway(
    State(app): State<AppHandle>,
    headers: HeaderMap,
    body: Result<Json<JsonRpcRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    let state: Arc<AppState> = app.state::<Arc<AppState>>().inner().clone();
    // SqlitePool 内部已是 Arc，clone 极廉价；外层再包 Arc 仅为满足
    // `tools::dispatch` 的签名（`handle_request` 需要 Arc<SqlitePool>）。
    handle_http(Arc::new(state.db.clone()), headers, body).await
}

/// 测试入口：State 本身就是 pool（对应 [`create_mcp_router`]）。
#[cfg(test)]
async fn mcp_endpoint(
    State(pool): State<Arc<SqlitePool>>,
    headers: HeaderMap,
    body: Result<Json<JsonRpcRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    handle_http(pool.clone(), headers, body).await
}

/// GET /mcp/tools —— 调试用，不走 JSON-RPC 包装（无状态依赖，两种挂载方式共用）。
async fn list_tools_debug() -> impl IntoResponse {
    Json(json!({
        "tools": tools::tool_specs(),
    }))
}

/// HTTP 层：JSON 解析 → 协议版本校验 → 分发 → 回包。
///
/// 与状态来源（AppHandle / 裸 pool）解耦，两种挂载方式共用。
/// 遵循 MCP Streamable HTTP：若客户端在 `Accept` 头声明 `text/event-stream`，
/// 响应以 SSE 事件流（`event: message`）下发；否则直接回 `application/json`。
async fn handle_http(
    pool: Arc<SqlitePool>,
    headers: HeaderMap,
    body: Result<Json<JsonRpcRequest>, axum::extract::rejection::JsonRejection>,
) -> Response {
    // Streamable HTTP：客户端请求 SSE 形式的响应（单个 JSON-RPC 消息包成
    // 一条 `event: message` 事件，随后结束流）。无状态，无需 session 注册表。
    let wants_sse = headers
        .get(header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|s| s.contains("text/event-stream"))
        .unwrap_or(false);

    let req = match body {
        Ok(Json(r)) => r,
        Err(e) => {
            // 顶层解析失败（无 id 可填）：按惯例回 null id
            let resp = JsonRpcResponse::err(
                None,
                JsonRpcError::parse_error(format!("invalid JSON: {}", e)),
            );
            return build_response(resp, wants_sse);
        }
    };

    if req.jsonrpc != "2.0" {
        match JsonRpcResponse::for_request_or_log(
            &req,
            JsonRpcError::invalid_request("jsonrpc 字段必须为 \"2.0\""),
        ) {
            Some(resp) => return build_response(resp, wants_sse),
            // 无 id 的 notification 类请求：无需回包
            None => return build_empty(wants_sse),
        }
    }

    match handle_request(&pool, &req).await {
        Ok(Some(resp)) => build_response(resp, wants_sse),
        Ok(None) => build_empty(wants_sse),
        Err(e) => build_response(JsonRpcResponse::err(req.id.clone(), e), wants_sse),
    }
}

/// 收尾（有响应体）：SSE 模式下把 JSON-RPC 响应包成单条 `event: message`
/// 事件流；否则按原 Streamable HTTP 行为直接返回 JSON。
fn build_response(resp: JsonRpcResponse, wants_sse: bool) -> Response {
    if wants_sse {
        let payload = serde_json::to_string(&resp).unwrap_or_else(|_| "{}".to_string());
        let body = format!("event: message\ndata: {}\n\n", payload);
        return Response::builder()
            .status(StatusCode::OK)
            .header(header::CONTENT_TYPE, "text/event-stream")
            .header(header::CACHE_CONTROL, "no-cache")
            .body(Body::from(body))
            .unwrap();
    }
    (StatusCode::OK, Json(resp)).into_response()
}

/// 收尾（notification 无回包）：SSE 与非 SSE 统一回 204 No Content，
/// 符合 Streamable HTTP 对 notification 的处理。
fn build_empty(_wants_sse: bool) -> Response {
    StatusCode::NO_CONTENT.into_response()
}

/// Streamable HTTP 协议版本协商：在支持的版本集合中，优先回显客户端请求的版本，
/// 不匹配则回退到最新支持版本。覆盖 `2024-11-05`（初版）与 `2025-03-26`
/// （Streamable HTTP 引入版）。
fn negotiate_protocol_version(requested: Option<&str>) -> String {
    const SUPPORTED: &[&str] = &["2025-03-26", "2024-11-05"];
    match requested {
        Some(v) if SUPPORTED.contains(&v) => v.to_string(),
        _ => SUPPORTED[0].to_string(),
    }
}

/// JSON-RPC method 路由表（与传输层无关，直接被 [`handle_http`] 调用）。
async fn handle_request(
    pool: &Arc<SqlitePool>,
    req: &JsonRpcRequest,
) -> Result<Option<JsonRpcResponse>, JsonRpcError> {
    match req.method.as_str() {
        "initialize" => Ok(JsonRpcResponse::for_request(
            req,
            json!({
                "protocolVersion": negotiate_protocol_version(
                    req.params.get("protocolVersion").and_then(|v| v.as_str())
                ),
                "serverInfo": {
                    "name": "dongx-rag-mcp",
                    "version": env!("CARGO_PKG_VERSION"),
                },
                "capabilities": {
                    "tools": { "listChanged": false }
                }
            }),
        )),
        "notifications/initialized" => {
            // 客户端握手完成，按 MCP 规范不需回包
            if req.is_notification() {
                Ok(None)
            } else {
                Ok(JsonRpcResponse::for_request(req, json!({})))
            }
        }
        "ping" => Ok(JsonRpcResponse::for_request(req, json!({}))),
        "tools/list" => Ok(JsonRpcResponse::for_request(
            req,
            json!({ "tools": tools::tool_specs() }),
        )),
        "tools/call" => {
            let args: ToolCallArgs = serde_json::from_value(req.params.clone())
                .map_err(|e| JsonRpcError::invalid_params(format!("params 非法: {}", e)))?;
            let result = tools::dispatch(pool.clone(), &args.name, args.arguments).await;
            // 业务错误（KB 未暴露、参数非法、工具执行失败…）一律以 isError=true 文本回包，
            // 不返 RPC error code——便于 client 渲染成对话气泡而非协议错误。
            let result_json = serde_json::to_value(result).unwrap_or_else(|e| {
                json!({
                    "content": [{ "type": "text", "text": format!("序列化失败: {}", e) }],
                    "isError": true,
                })
            });
            Ok(JsonRpcResponse::for_request(req, result_json))
        }
        other => {
            let err = JsonRpcError::method_not_found(other);
            Ok(JsonRpcResponse::for_request_or_log(req, err))
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use axum::body::Body;
    use axum::http::{Method, Request, StatusCode};
    use serde_json::Value;
    use tower::ServiceExt;

    /// 跑全量迁移的 in-mem SQLite（与 `tools::tests` 同样套路）。
    async fn test_pool() -> SqlitePool {
        use sqlx::sqlite::SqlitePoolOptions;
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-mem pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    /// 往 router POST 一个 JSON-RPC 请求，返回响应字节。
    async fn post_json(pool: SqlitePool, body: &str) -> (StatusCode, Value) {
        let app = create_mcp_router(pool);
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(body.to_string()))
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).unwrap_or_else(|e| {
            panic!(
                "响应不是合法 JSON: {} / bytes: {:?}",
                e,
                String::from_utf8_lossy(&bytes)
            )
        });
        (status, json)
    }

    async fn get_json(pool: SqlitePool, path: &str) -> (StatusCode, Value) {
        let app = create_mcp_router(pool);
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::GET)
                    .uri(path)
                    .body(Body::empty())
                    .unwrap(),
            )
            .await
            .unwrap();
        let status = response.status();
        let bytes = axum::body::to_bytes(response.into_body(), 64 * 1024)
            .await
            .unwrap();
        let json: Value = serde_json::from_slice(&bytes).expect("响应不是合法 JSON");
        (status, json)
    }

    /// 一条已暴露的 KB（id=kb-x，name=测试 KB）。
    async fn insert_kb(pool: &SqlitePool, id: &str, name: &str, exposed: bool) {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO knowledge_bases (id, name, description, embedding_model,
                embedding_channel_id, status, mcp_exposed, created_at, updated_at)
             VALUES (?, ?, '', 'text-embedding-3-small', 'ch-1', 1, ?, ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(if exposed { 1i64 } else { 0i64 })
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("insert kb");
    }

    // ---------- initialize ----------

    #[tokio::test]
    async fn initialize_returns_server_info_and_capabilities() {
        let pool = test_pool().await;
        let (status, body) = post_json(
            pool,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{}}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["jsonrpc"], "2.0");
        assert_eq!(body["id"], 1);
        assert_eq!(body["result"]["serverInfo"]["name"], "dongx-rag-mcp");
        assert!(body["result"]["capabilities"]["tools"].is_object());
    }

    /// initialize 应回显客户端请求的协议版本（Streamable HTTP 协商）。
    #[tokio::test]
    async fn initialize_echoes_requested_protocol_version() {
        let pool = test_pool().await;
        let (status, body) = post_json(
            pool,
            r#"{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-03-26"}}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["result"]["protocolVersion"], "2025-03-26");
    }

    // ---------- notifications/initialized ----------

    /// notification（id 缺省）按 MCP 规范不回包 → HTTP 204 NO CONTENT。
    #[tokio::test]
    async fn notification_initialized_returns_no_content() {
        let pool = test_pool().await;
        let app = create_mcp_router(pool);
        let response = app
            .oneshot(
                Request::builder()
                    .method(Method::POST)
                    .uri("/mcp")
                    .header("content-type", "application/json")
                    .body(Body::from(
                        r#"{"jsonrpc":"2.0","method":"notifications/initialized"}"#,
                    ))
                    .unwrap(),
            )
            .await
            .unwrap();
        assert_eq!(response.status(), StatusCode::NO_CONTENT);
    }

    // ---------- tools/list ----------

    #[tokio::test]
    async fn tools_list_returns_five_tools() {
        let pool = test_pool().await;
        let (status, body) = post_json(
            pool,
            r#"{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let tools = body["result"]["tools"].as_array().expect("tools 应为数组");
        assert_eq!(tools.len(), 5);
        let names: Vec<&str> = tools.iter().map(|t| t["name"].as_str().unwrap()).collect();
        for expected in [
            "search_knowledge_base",
            "list_knowledge_bases",
            "ask_knowledge_base",
            "read_document",
            "get_knowledge_base_stats",
        ] {
            assert!(
                names.contains(&expected),
                "tools/list 应包含 {expected}，实际: {names:?}"
            );
        }
        // MCP 协议要求每个 tool 暴露 `inputSchema`（camelCase），缺则客户端不解析。
        // 同时确保 snake_case 字段名 `input_schema` 不出现在响应里。
        for tool in tools {
            assert!(
                tool.get("inputSchema").is_some(),
                "tool {} 缺 inputSchema 字段，wire 格式：{:?}",
                tool["name"],
                tool
            );
            assert!(
                tool.get("input_schema").is_none(),
                "tool {} 含错误的 snake_case 字段 input_schema，应为 inputSchema",
                tool["name"]
            );
        }
    }

    // ---------- tools/call: list_knowledge_bases ----------

    #[tokio::test]
    async fn tools_call_list_knowledge_bases_returns_filtered_table() {
        let pool = test_pool().await;
        // 一已暴露一未暴露
        insert_kb(&pool, "kb-public", "Public KB", true).await;
        insert_kb(&pool, "kb-private", "Private KB", false).await;

        let (status, body) = post_json(
            pool,
            r#"{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"list_knowledge_bases","arguments":{}}}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        let result = &body["result"];
        assert!(result["isError"].is_null() || result["isError"] == false);
        let text = result["content"][0]["text"]
            .as_str()
            .expect("text 应为字符串");
        assert!(text.contains("kb-public"), "应列出已暴露 KB");
        assert!(!text.contains("kb-private"), "不应列出未暴露 KB");
    }

    /// 没有 KB 暴露时不应报错，应回「当前没有...」文本。
    #[tokio::test]
    async fn tools_call_list_knowledge_bases_empty_when_none_exposed() {
        let pool = test_pool().await;
        let (_, body) = post_json(
            pool,
            r#"{"jsonrpc":"2.0","id":4,"method":"tools/call","params":{"name":"list_knowledge_bases","arguments":{}}}"#,
        )
        .await;
        let text = body["result"]["content"][0]["text"].as_str().unwrap();
        assert!(
            text.contains("当前没有") || text.contains("没有已开启"),
            "空态提示文案: {text}"
        );
    }

    /// 未知 tool 名以 `isError=true` 文本回包（不返 RPC error code）。
    #[tokio::test]
    async fn tools_call_unknown_tool_returns_iserror_text() {
        let pool = test_pool().await;
        let (_, body) = post_json(
            pool,
            r#"{"jsonrpc":"2.0","id":5,"method":"tools/call","params":{"name":"nope","arguments":{}}}"#,
        )
        .await;
        let result = &body["result"];
        assert_eq!(result["isError"], true);
        assert!(result["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("未知工具"));
    }

    // ---------- 协议级错误（无 DB 依赖） ----------

    /// 未知 JSON-RPC method → RPC error code `-32601`，HTTP 状态仍是 200。
    #[tokio::test]
    async fn unknown_method_returns_method_not_found_error() {
        let pool = test_pool().await;
        let (status, body) = post_json(
            pool,
            r#"{"jsonrpc":"2.0","id":6,"method":"prompts/list","params":{}}"#,
        )
        .await;
        assert_eq!(status, StatusCode::OK);
        assert_eq!(body["error"]["code"], -32601);
        assert!(body["result"].is_null());
    }

    /// 非 2.0 协议 → RPC error code `-32600 invalid_request`。
    #[tokio::test]
    async fn wrong_protocol_version_returns_invalid_request() {
        let pool = test_pool().await;
        let (_, body) = post_json(
            pool,
            r#"{"jsonrpc":"1.0","id":7,"method":"ping","params":{}}"#,
        )
        .await;
        assert_eq!(body["error"]["code"], -32600);
    }

    /// 顶层 JSON 解析失败 → RPC error code `-32700 parse_error`。
    #[tokio::test]
    async fn malformed_json_returns_parse_error() {
        let pool = test_pool().await;
        // 故意截断的 JSON
        let (_, body) = post_json(pool, r#"{ "jsonrpc": "#).await;
        assert_eq!(body["error"]["code"], -32700);
    }

    // ---------- 调试端点 ----------

    #[tokio::test]
    async fn get_mcp_tools_debug_endpoint() {
        let pool = test_pool().await;
        let (status, body) = get_json(pool, "/mcp/tools").await;
        assert_eq!(status, StatusCode::OK);
        let tools = body["tools"].as_array().expect("tools 数组");
        assert_eq!(tools.len(), 5);
    }
}
