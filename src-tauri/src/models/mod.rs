//! Database row entities — 1:1 mapping with migrations/001_init.sql.
//!
//! Java mental model: 这些 struct 等价于 JPA 的 @Entity，但 sqlx 没有运行时
//! 反射，FromRow 通过列名 -> 字段名的匹配在解码时完成（编译期无开销）。
//! DTO 转换（如 status INTEGER -> "active" 字符串）放在 commands 层做，
//! 相当于 Spring 里 Entity -> DTO 的 Mapper 层。

use serde::{Deserialize, Serialize};
use sqlx::FromRow;

// ============================================================
// channels
// ============================================================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct ChannelRow {
    pub id: String,
    pub name: String,
    pub protocol: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub channel_type: String, // openai | deepseek | claude | gemini | zhipu | ollama | custom
    pub base_url: String,
    pub cred_encrypted: String,      // AES-GCM ciphertext of upstream API key
    pub models: String,              // JSON array string
    pub status: i32,                 // 0=disabled 1=enabled 2=error
    pub priority: i32,
    pub weight: i32,
    pub config: String,              // JSON object string
    pub model_mapping: String,       // JSON object string
    pub endpoints: String,           // JSON array string
    pub created_at: String,
    pub updated_at: String,
    pub last_test_at: Option<String>,
    pub last_test_ok: Option<i32>,
}

// ============================================================
// gateway_keys
// ============================================================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct GatewayKeyRow {
    pub id: String,
    pub name: String,
    pub key: String,            // masked display: sk-dongapi-****a1b2
    pub key_hash: String,       // SHA-256 hex for lookup
    pub status: i32,            // 0=disabled 1=active 2=expired
    pub allowed_models: String,   // JSON array string
    pub allowed_channels: String, // JSON array string
    pub quota_limit: i64,       // 0 = unlimited
    pub quota_used: i64,
    pub expires_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

// ============================================================
// request_logs
// ============================================================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RequestLogRow {
    pub id: String,
    pub seq: Option<i64>,
    pub api_key_name: Option<String>,
    pub channel_name: Option<String>,
    pub model: String,
    pub upstream_model: Option<String>,
    pub mode: String,                    // chat | completion | embedding | other
    pub status_code: i32,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
    pub duration_ms: i64,
    pub error_message: Option<String>,
    pub is_stream: i32,
    pub is_retry: i32,
    pub created_at: String,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub risk_level: String,
    pub risk_score: i64,
    pub risk_summary: Option<String>,
    pub security_action: String,
    pub sanitized: i32,
    pub blocked_reason: Option<String>,
}

/// Slim variant for list queries (excludes heavy request/response bodies).
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct RequestLogListItem {
    pub id: String,
    pub seq: Option<i64>,
    pub api_key_name: Option<String>,
    pub channel_name: Option<String>,
    pub model: String,
    pub mode: String,
    pub status_code: i32,
    pub total_tokens: i64,
    pub duration_ms: i64,
    pub is_stream: i32,
    pub is_retry: i32,
    pub created_at: String,
    pub error_message: Option<String>,
    pub risk_level: String,
    pub security_action: String,
}

// ============================================================
// audit_events
// ============================================================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct AuditEventRow {
    pub id: String,
    pub timestamp: String,
    #[sqlx(rename = "type")]
    #[serde(rename = "type")]
    pub event_type: String,   // rate_limit | invalid_key | quota_exhaust | suspicious | config_change
    pub severity: String,     // info | warning | critical
    pub actor: Option<String>,
    pub message: String,
    pub meta: Option<String>, // JSON string
}

// ============================================================
// settings (key-value)
// ============================================================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct SettingRow {
    pub key: String,
    pub value: String, // JSON encoded value
}

// ============================================================
// Dashboard stats (aggregation projection)
// ============================================================
#[derive(Debug, Clone, Serialize, Deserialize, FromRow)]
pub struct DashboardStatsRow {
    pub today_requests: i64,
    pub today_total_tokens: i64,
    pub avg_latency_ms: i64,
    pub active_channels: i64,
    pub total_channels: i64,
    pub total_api_keys: i64,
    pub total_requests: i64,
    pub total_tokens: i64,
}
