use crate::error::AppResult;
use serde::{Deserialize, Serialize};

/// Request dispatch context
#[derive(Debug, Clone)]
pub struct DispatchContext {
    pub model: String,
    pub api_key_id: String,
    pub is_stream: bool,
    pub request_body: serde_json::Value,
}

/// Selected channel for forwarding
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SelectedChannel {
    pub id: String,
    pub name: String,
    pub base_url: String,
    pub endpoint: String,
    pub upstream_model: String,
    pub protocol: String,
}

/// Select the best channel for a given request
/// Algorithm: filter enabled + model match → sort by priority → weighted random in top priority group
pub async fn select_channel(ctx: &DispatchContext) -> AppResult<SelectedChannel> {
    // TODO: Implement full selection logic
    // 1. Query enabled channels (status=1) that support the requested model
    // 2. Apply model_mapping to find matching channels
    // 3. Sort by priority (ascending)
    // 4. Take top priority group
    // 5. Weighted random selection within group
    // 6. Check circuit breaker status
    let _ = ctx;
    Err(crate::error::AppError::NotFound("No available channel".into()))
}
