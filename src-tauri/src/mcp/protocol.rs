//! JSON-RPC 2.0 wire types for MCP.
//!
//! 只覆盖 MCP 服务器侧接收/响应用到的子集；requests → response / notification。
//! Refs:
//!   - https://www.jsonrpc.org/specification
//!   - https://modelcontextprotocol.io/specification

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// JSON-RPC 2.0 请求（或 notification：`id = None`）。
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    pub jsonrpc: String,
    pub method: String,
    #[serde(default)]
    pub params: Value,
    /// `None` → notification（不需回包）；否则必须回 [`JsonRpcResponse`]。
    #[serde(default)]
    pub id: Option<Value>,
}

impl JsonRpcRequest {
    pub fn is_notification(&self) -> bool {
        self.id.is_none()
    }
}

/// JSON-RPC 2.0 响应。`result` 与 `error` 二选一。
#[derive(Debug, Serialize)]
pub struct JsonRpcResponse {
    pub jsonrpc: &'static str,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub id: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<JsonRpcError>,
}

impl JsonRpcResponse {
    pub fn ok(id: Option<Value>, result: Value) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: Some(result),
            error: None,
        }
    }

    pub fn err(id: Option<Value>, err: JsonRpcError) -> Self {
        Self {
            jsonrpc: "2.0",
            id,
            result: None,
            error: Some(err),
        }
    }

    /// 收到 notification 时返回 `None`：调用方吞掉、不发响应。
    pub fn for_request(req: &JsonRpcRequest, result: Value) -> Option<Self> {
        if req.is_notification() {
            None
        } else {
            Some(Self::ok(req.id.clone(), result))
        }
    }

    /// 收到 notification 时仍然报错：日志记一句后吞掉（与 spec 一致：不回包）。
    pub fn for_request_or_log(req: &JsonRpcRequest, err: JsonRpcError) -> Option<Self> {
        if req.is_notification() {
            tracing::debug!("notification '{}' 失败：{}", req.method, err.message);
            None
        } else {
            Some(Self::err(req.id.clone(), err))
        }
    }
}

#[derive(Debug, Serialize, Clone)]
pub struct JsonRpcError {
    pub code: i64,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub data: Option<Value>,
}

impl JsonRpcError {
    pub fn parse_error(msg: impl Into<String>) -> Self {
        Self {
            code: -32700,
            message: msg.into(),
            data: None,
        }
    }
    pub fn invalid_request(msg: impl Into<String>) -> Self {
        Self {
            code: -32600,
            message: msg.into(),
            data: None,
        }
    }
    pub fn method_not_found(method: &str) -> Self {
        Self {
            code: -32601,
            message: format!("Method not found: {}", method),
            data: None,
        }
    }
    pub fn invalid_params(msg: impl Into<String>) -> Self {
        Self {
            code: -32602,
            message: msg.into(),
            data: None,
        }
    }
    pub fn internal_error(msg: impl Into<String>) -> Self {
        Self {
            code: -32603,
            message: msg.into(),
            data: None,
        }
    }
}

/// 应用级 MCP 错误码（占用 JSON-RPC 保留区间 -32000 ~ -32099）。
/// 当前实现把 KB 类业务错误以 isError=true 文本回包，便于 MCP client 渲染成对话气泡，
/// 这些 code 仅在 `require_exposed_kb` 等内部拒识处使用，不直接透传给 client。
pub const ERR_MCP_KB_NOT_FOUND: i64 = -32003;
pub const ERR_MCP_KB_NOT_EXPOSED: i64 = -32004;
/// Wiki 项目不存在（MCP 工具专用）。
pub const ERR_MCP_WIKI_NOT_FOUND: i64 = -32005;
/// Wiki 项目存在但未开启 MCP 暴露（MCP 工具专用）。
pub const ERR_MCP_WIKI_NOT_EXPOSED: i64 = -32006;

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn req_with_id() -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "tools/list".into(),
            params: json!({}),
            id: Some(json!(1)),
        }
    }

    fn notification() -> JsonRpcRequest {
        JsonRpcRequest {
            jsonrpc: "2.0".into(),
            method: "notifications/initialized".into(),
            params: json!(null),
            id: None,
        }
    }

    /// `JsonRpcResponse::for_request` 对 notification 返回 `None`（不发包）。
    /// 否则构造带 id 的正常响应。
    #[test]
    fn for_request_skips_notifications() {
        assert!(JsonRpcResponse::for_request(&notification(), json!({})).is_none());
        let resp = JsonRpcResponse::for_request(&req_with_id(), json!({"ok": true}));
        let resp = resp.expect("request 应有响应");
        assert_eq!(resp.id.as_ref().and_then(|v| v.as_i64()), Some(1));
        assert!(resp.result.is_some());
        assert!(resp.error.is_none());
    }

    /// `for_request_or_log` 同理：notification 路径返回 None（仅记录）。
    #[test]
    fn for_request_or_log_skips_notifications() {
        assert!(JsonRpcResponse::for_request_or_log(
            &notification(),
            JsonRpcError::method_not_found("notifications/initialized")
        )
        .is_none());
    }

    /// 错误响应：保留 id，result 为空，error.code/message 正确填入。
    #[test]
    fn err_response_carries_id_and_code() {
        let err = JsonRpcError::invalid_params("kb_id required");
        let resp = JsonRpcResponse::err(Some(json!(42)), err);
        assert_eq!(resp.id.as_ref().and_then(|v| v.as_i64()), Some(42));
        let e = resp.error.as_ref().expect("error 应存在");
        assert_eq!(e.code, -32602);
        assert!(e.message.contains("kb_id"));
        assert!(resp.result.is_none());
    }

    /// 标准 JSON-RPC 错误码映射稳定。
    #[test]
    fn error_codes_follow_jsonrpc_2_0() {
        assert_eq!(JsonRpcError::parse_error("x").code, -32700);
        assert_eq!(JsonRpcError::invalid_request("x").code, -32600);
        assert_eq!(JsonRpcError::method_not_found("mcp/foo").code, -32601);
        assert_eq!(JsonRpcError::invalid_params("x").code, -32602);
        assert_eq!(JsonRpcError::internal_error("x").code, -32603);
    }

    /// 完整成功响应 → 期望 wire 格式（id + result，error 字段缺省被 skip）。
    #[test]
    fn ok_response_wire_shape() {
        let resp = JsonRpcResponse::ok(Some(json!("abc")), json!({"hello": "world"}));
        let s = serde_json::to_string(&resp).unwrap();
        // 必须包含 jsonrpc / id=abc / result.hello
        assert!(s.contains("\"jsonrpc\":\"2.0\""));
        assert!(s.contains("\"id\":\"abc\""));
        assert!(s.contains("\"hello\":\"world\""));
        // error 字段在 skip_serializing_if=Option::is_none 时不应出现
        assert!(!s.contains("\"error\""));
    }
}
