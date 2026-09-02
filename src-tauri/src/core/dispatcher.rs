use rand::Rng;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use sqlx::SqlitePool;

use crate::crypto;
use crate::db::repository::{channel_health, channels};
use crate::error::{AppError, AppResult};
use crate::models::ChannelRow;

/// Request dispatch context — what the data plane knows about an incoming call.
#[derive(Debug, Clone)]
pub struct DispatchContext {
    pub model: String,
    pub api_key_id: String,
    pub is_stream: bool,
    pub request_body: serde_json::Value,
}

/// A selected upstream channel ready for forwarding.
///
/// Carries everything `adapter::ChannelConfig` needs, plus the decrypted
/// upstream API key (weighted-picked from the channel's key set).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SelectedChannel {
    pub id: String,
    pub name: String,
    pub channel_type: String,
    pub base_url: String,
    pub upstream_api_key: String,
    pub models: Vec<String>,
    pub model_mapping: serde_json::Value,
    pub extra: serde_json::Value,
    pub timeout_secs: u64,
}

/// Select a single channel for a request (priority + weight + one upstream key).
///
/// This is the single-attempt convenience form. The data-plane
/// `run_chat_pipeline` instead drives a `Failover` (core/failover.rs) which
/// loops over `candidate_channels` + `pick_one` to provide automatic channel
/// Return the candidate channels for `ctx.model`: those that serve the model,
/// are not currently in circuit-breaker cooldown, and are not in `exclude`.
///
/// Any empty step returns `Err` — the caller (`Failover`) distinguishes
/// "no channel from the start" (exclude empty) from "all tried / cooling down"
/// (exclude non-empty) to decide between a 503 and a retry-exhausted error.
pub async fn candidate_channels(
    pool: &SqlitePool,
    ctx: &DispatchContext,
    exclude: &HashSet<String>,
) -> AppResult<Vec<ChannelRow>> {
    let enabled = channels::list_enabled(pool).await?;
    if enabled.is_empty() {
        return Err(AppError::NotFound("没有已启用的渠道".into()));
    }
    // Channels that serve this model (listed or mapped).
    let serving: Vec<&ChannelRow> = enabled
        .iter()
        .filter(|c| channel_serves_model(c, &ctx.model))
        .collect();
    if serving.is_empty() {
        return Err(AppError::NotFound(format!(
            "没有渠道支持模型 '{}'",
            ctx.model
        )));
    }
    // Drop channels excluded this request (already tried) and those whose
    // circuit breaker is currently open (cooling down).
    let mut candidates: Vec<ChannelRow> = Vec::with_capacity(serving.len());
    for c in &serving {
        if exclude.contains(&c.id) {
            continue;
        }
        if channel_health::is_open(pool, &c.id).await {
            continue; // 熔断冷却中
        }
        candidates.push((*c).clone());
    }
    if candidates.is_empty() {
        return Err(AppError::NotFound(
            "所有候选渠道均处于熔断冷却中或已尝试，请稍后重试".into(),
        ));
    }
    Ok(candidates)
}

/// Pick one channel from candidates: take the top-priority group, weighted-random
/// select within it by `weight`, then decrypt + weighted-random pick one upstream key.
pub fn pick_one(candidates: &[ChannelRow]) -> AppResult<SelectedChannel> {
    // Top-priority group (higher priority wins).
    let top_priority = candidates.iter().map(|c| c.priority).max().unwrap_or(0);
    let top: Vec<&ChannelRow> = candidates
        .iter()
        .filter(|c| c.priority == top_priority)
        .collect();

    // Weighted-random pick a channel by its `weight`.
    let pairs: Vec<(String, i32)> = top.iter().map(|c| (c.id.clone(), c.weight)).collect();
    let chosen_id = weighted_pick(&pairs)
        .ok_or_else(|| AppError::Internal("渠道加权选择失败".into()))?;
    let row = top
        .iter()
        .find(|c| c.id == chosen_id)
        .expect("加权选中的渠道必然存在");

    // Decrypt upstream keys; weighted-pick one.
    let upstream_api_key = pick_upstream_key(&row.cred_encrypted)?;

    let models: Vec<String> = serde_json::from_str(&row.models).unwrap_or_default();
    let model_mapping: serde_json::Value =
        serde_json::from_str(&row.model_mapping).unwrap_or_else(|_| serde_json::json!({}));
    let config: serde_json::Value =
        serde_json::from_str(&row.config).unwrap_or_else(|_| serde_json::json!({}));
    let timeout_secs = config
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(60);

    Ok(SelectedChannel {
        id: row.id.clone(),
        name: row.name.clone(),
        channel_type: row.channel_type.clone(),
        base_url: row.base_url.clone(),
        upstream_api_key,
        models,
        model_mapping,
        extra: config,
        timeout_secs,
    })
}

/// Whether a channel serves the given model — listed in `models` or mapped
/// in `model_mapping`.
fn channel_serves_model(c: &ChannelRow, model: &str) -> bool {
    let models: Vec<String> = serde_json::from_str(&c.models).unwrap_or_default();
    if models.iter().any(|m| m == model) {
        return true;
    }
    let mapping: serde_json::Value =
        serde_json::from_str(&c.model_mapping).unwrap_or_else(|_| serde_json::json!({}));
    mapping.get(model).is_some()
}

/// Weighted-random selection. `weight` clamped to >=1 so every entry has a chance.
fn weighted_pick(pairs: &[(String, i32)]) -> Option<String> {
    if pairs.is_empty() {
        return None;
    }
    let total: i32 = pairs.iter().map(|(_, w)| (*w).max(1)).sum();
    if total <= 0 {
        return Some(pairs[0].0.clone());
    }
    let mut rng = rand::thread_rng();
    let mut r = rng.gen_range(0..total);
    for (id, w) in pairs {
        r -= (*w).max(1);
        if r < 0 {
            return Some(id.clone());
        }
    }
    Some(pairs[0].0.clone())
}

/// Decrypt the channel credential and weighted-pick one upstream key.
///
/// Supports two storage shapes:
/// - Multi-key JSON array: `[{"key":"sk-...","weight":7}, ...]`
/// - Legacy single key: the plaintext is the key itself.
fn pick_upstream_key(cred_encrypted: &str) -> AppResult<String> {
    let plaintext = crypto::decrypt(cred_encrypted).map_err(AppError::Crypto)?;

    // Multi-key JSON array.
    if let Ok(keys) = serde_json::from_str::<Vec<serde_json::Value>>(&plaintext) {
        if !keys.is_empty() {
            let pairs: Vec<(String, i32)> = keys
                .iter()
                .filter_map(|k| {
                    let key = k.get("key")?.as_str()?.to_string();
                    if key.is_empty() {
                        return None;
                    }
                    let weight = k.get("weight").and_then(|v| v.as_i64()).unwrap_or(1) as i32;
                    Some((key, weight))
                })
                .collect();
            if !pairs.is_empty() {
                return Ok(weighted_pick(&pairs)
                    .ok_or_else(|| AppError::Crypto("无可用上游密钥".into()))?);
            }
        }
    }

    // Legacy single key.
    if plaintext.is_empty() {
        return Err(AppError::Crypto("上游密钥为空".into()));
    }
    Ok(plaintext)
}
