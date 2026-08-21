use serde::{Deserialize, Serialize};

/// Channel entity (maps to channels table)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Channel {
    pub id: String,
    pub name: String,
    pub protocol: String,      // openai | anthropic | ollama
    pub r#type: String,         // openai | deepseek | claude | zhipu | ollama | custom
    pub base_url: String,
    pub api_key_encrypted: Option<String>,
    pub models: serde_json::Value, // Vec<String> stored as JSON
    pub priority: i32,
    pub weight: i32,
    pub status: String,         // active | disabled
    pub config: serde_json::Value,
    pub model_mapping: serde_json::Value,
    pub endpoints: serde_json::Value, // Vec<String> stored as JSON
    pub created_at: String,
    pub updated_at: String,
}

/// API Key entity (maps to api_keys table)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApiKey {
    pub id: String,
    pub key_prefix: String,         // First 12 chars for display
    pub key_hash: String,           // SHA-256 hash for lookup
    pub name: Option<String>,
    pub max_tokens: Option<i64>,    // Budget limit (null = unlimited)
    pub used_tokens: i64,
    pub status: String,             // active | disabled | exhausted
    pub expires_at: Option<String>,
    pub created_at: String,
    pub last_used_at: Option<String>,
}

/// Request log entity (maps to request_logs table)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct RequestLog {
    pub id: String,
    pub request_id: String,
    pub timestamp: String,
    pub method: String,
    pub path: String,
    pub status_code: i32,
    pub latency_ms: i64,
    pub client_ip: String,
    pub gateway_key_id: Option<String>,
    pub channel_id: Option<String>,
    pub channel_name: Option<String>,
    pub model: Option<String>,
    pub prompt_tokens: Option<i64>,
    pub completion_tokens: Option<i64>,
    pub total_tokens: Option<i64>,
    pub request_body: Option<String>,
    pub response_body: Option<String>,
    pub error: Option<String>,
}

/// Audit event entity (maps to audit_events table)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AuditEvent {
    pub id: String,
    pub timestamp: String,
    pub event_type: String,     // config_change | key_create | key_delete | key_use | security_alert
    pub severity: String,       // info | warning | critical
    pub actor: String,          // user | system | gateway
    pub target: Option<String>,
    pub detail: serde_json::Value,
    pub ip: Option<String>,
}

/// Settings entity (maps to settings table, key-value)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct Setting {
    pub key: String,
    pub value: String,
}
