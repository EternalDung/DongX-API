use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::adapter;
use crate::crypto;
use crate::db::repository::channels;
use crate::error::{AppError, AppResult};
use crate::models::ChannelRow;
use crate::AppState;

/// One upstream API key + its load-balancing weight.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KeyEntry {
    pub key: String,
    pub weight: i32,
}

/// Channel creation/update payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChannelInput {
    pub name: String,
    pub protocol: String,
    pub r#type: String,
    pub base_url: String,
    /// Multiple upstream keys for load balancing (each with a weight).
    pub keys: Vec<KeyEntry>,
    pub models: Vec<String>,
    pub priority: i32,
    pub weight: i32,
    pub config: serde_json::Value,
    pub model_mapping: serde_json::Value,
    pub endpoints: Vec<String>,
    /// Request timeout in seconds (folded into `config` for storage).
    pub timeout_secs: i64,
}

/// Serialize a `channels` row into the frontend `Channel` wire shape.
///
/// `api_key` is intentionally empty (the upstream key string is shown via
/// `keys`). The upstream keys are stored encrypted at rest, but this is a
/// local single-user gateway so they are decrypted and returned for display
/// (so the edit dialog can echo them back).
fn row_to_value(row: ChannelRow) -> serde_json::Value {
    let models: Vec<String> = serde_json::from_str(&row.models).unwrap_or_default();
    let config: serde_json::Value =
        serde_json::from_str(&row.config).unwrap_or_else(|_| serde_json::json!({}));
    let model_mapping: serde_json::Value =
        serde_json::from_str(&row.model_mapping).unwrap_or_else(|_| serde_json::json!({}));
    let endpoints: Vec<String> = serde_json::from_str(&row.endpoints).unwrap_or_default();
    // Decrypt the upstream keys (local gateway: not treated as secrets) so the
    // UI can echo them when editing. Any failure falls back to an empty list.
    let keys: Vec<KeyEntry> = if row.cred_encrypted.is_empty() {
        Vec::new()
    } else {
        crypto::decrypt(&row.cred_encrypted)
            .ok()
            .and_then(|raw| serde_json::from_str::<Vec<KeyEntry>>(&raw).ok())
            .unwrap_or_default()
    };

    serde_json::json!({
        "id": row.id,
        "name": row.name,
        "protocol": row.protocol,
        "type": row.channel_type,
        "base_url": row.base_url,
        "api_key": "",
        "keys": keys,
        "models": models,
        "status": row.status,
        "priority": row.priority,
        "weight": row.weight,
        "config": config,
        "model_mapping": model_mapping,
        "endpoints": endpoints,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
        "last_test_at": row.last_test_at,
        "last_test_ok": row.last_test_ok,
    })
}

/// Encrypt the upstream key set for storage. Empty set -> empty ciphertext
/// (used by providers like Ollama that need no credential).
fn encrypt_keys(keys: &[KeyEntry]) -> AppResult<String> {
    if keys.is_empty() {
        Ok(String::new())
    } else {
        let json = serde_json::to_string(keys).unwrap_or_else(|_| "[]".into());
        crypto::encrypt(&json).map_err(AppError::Crypto)
    }
}

/// List all channels
#[tauri::command]
pub async fn list_channels(
    state: State<'_, Arc<AppState>>,
) -> AppResult<Vec<serde_json::Value>> {
    let rows = channels::list(&state.db).await?;
    Ok(rows.into_iter().map(row_to_value).collect())
}

/// Create a new channel
#[tauri::command]
pub async fn create_channel(
    state: State<'_, Arc<AppState>>,
    input: ChannelInput,
) -> AppResult<serde_json::Value> {
    if input.name.trim().is_empty() || input.base_url.trim().is_empty() {
        return Err(AppError::Validation(
            "渠道名称和 Base URL 不能为空".into(),
        ));
    }

    let cred_encrypted = encrypt_keys(&input.keys)?;
    let models = serde_json::to_string(&input.models).unwrap_or_else(|_| "[]".into());

    // Fold timeout into the config JSON object for storage.
    let mut config = input.config.clone();
    config["timeout_secs"] = serde_json::json!(input.timeout_secs);
    let config = serde_json::to_string(&config).unwrap_or_else(|_| "{}".into());

    let model_mapping =
        serde_json::to_string(&input.model_mapping).unwrap_or_else(|_| "{}".into());
    let endpoints = serde_json::to_string(&input.endpoints).unwrap_or_else(|_| "[]".into());

    let row = channels::insert(
        &state.db,
        &input.name,
        &input.protocol,
        &input.r#type,
        &input.base_url,
        &cred_encrypted,
        &models,
        input.priority,
        input.weight,
        &config,
        &model_mapping,
        &endpoints,
    )
    .await?;

    Ok(row_to_value(row))
}

/// Update an existing channel
#[tauri::command]
pub async fn update_channel(
    state: State<'_, Arc<AppState>>,
    id: String,
    input: ChannelInput,
) -> AppResult<()> {
    if input.name.trim().is_empty() || input.base_url.trim().is_empty() {
        return Err(AppError::Validation(
            "渠道名称和 Base URL 不能为空".into(),
        ));
    }

    // Preserve existing encrypted keys when the form leaves them blank.
    let existing = channels::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("渠道不存在: {id}")))?;

    let cred_encrypted = if input.keys.is_empty() {
        existing.cred_encrypted
    } else {
        encrypt_keys(&input.keys)?
    };

    let models = serde_json::to_string(&input.models).unwrap_or_else(|_| "[]".into());

    let mut config = input.config.clone();
    config["timeout_secs"] = serde_json::json!(input.timeout_secs);
    let config = serde_json::to_string(&config).unwrap_or_else(|_| "{}".into());

    let model_mapping =
        serde_json::to_string(&input.model_mapping).unwrap_or_else(|_| "{}".into());
    let endpoints = serde_json::to_string(&input.endpoints).unwrap_or_else(|_| "[]".into());

    channels::update(
        &state.db,
        &id,
        &input.name,
        &input.protocol,
        &input.r#type,
        &input.base_url,
        &cred_encrypted,
        &models,
        input.priority,
        input.weight,
        &config,
        &model_mapping,
        &endpoints,
        existing.status, // keep current enable/disable state
    )
    .await?;

    Ok(())
}

/// Delete a channel
#[tauri::command]
pub async fn delete_channel(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> AppResult<()> {
    let affected = channels::delete(&state.db, &id).await?;
    if affected == 0 {
        return Err(AppError::NotFound(format!("渠道不存在: {id}")));
    }
    Ok(())
}

/// Test channel connectivity by issuing a minimal upstream request.
///
/// Returns `true` if the adaptor reports success; the result is also
/// persisted via `set_test_result` for the UI to display later.
#[tauri::command]
pub async fn test_channel(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> AppResult<bool> {
    let row = channels::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("渠道不存在: {id}")))?;

    // Decrypt the key set; fall back to legacy single-key format if needed.
    let keys: Vec<KeyEntry> = if row.cred_encrypted.is_empty() {
        vec![]
    } else {
        let raw = crypto::decrypt(&row.cred_encrypted).map_err(AppError::Crypto)?;
        serde_json::from_str(&raw).unwrap_or_else(|_| {
            vec![KeyEntry {
                key: raw,
                weight: 1,
            }]
        })
    };
    let api_key = keys.first().map(|k| k.key.clone()).unwrap_or_default();

    let models: Vec<String> = serde_json::from_str(&row.models).unwrap_or_default();
    let config: serde_json::Value =
        serde_json::from_str(&row.config).unwrap_or_else(|_| serde_json::json!({}));
    let model_mapping: serde_json::Value =
        serde_json::from_str(&row.model_mapping).unwrap_or_else(|_| serde_json::json!({}));

    let timeout_secs = config
        .get("timeout_secs")
        .and_then(|v| v.as_i64())
        .unwrap_or(30);

    let config = adapter::ChannelConfig {
        base_url: row.base_url.clone(),
        api_key,
        models,
        model_mapping,
        extra: config,
        timeout_secs: timeout_secs.max(1) as u64,
        stream: false,
    };

    let adaptor = adapter::get_adaptor(&row.channel_type);
    let result = adaptor.test(&config).await?;

    channels::set_test_result(&state.db, &id, result.success).await?;

    Ok(result.success)
}

/// Fetch the live model list from a provider.
///
/// The UI "拉取模型" button passes the channel's `base_url` and the first
/// upstream `api_key`; we hit the provider's model-list endpoint (per-adaptor
/// auth/path) and return the upstream model ids. A brand-new, unsaved channel
/// with no base URL yet falls back to the adaptor's local preset list (nothing
/// to fetch). Any network failure or empty upstream result is surfaced as an
/// error rather than silently returning presets, so the UI can show why the
/// pull failed instead of looking like it succeeded with stale data.
#[tauri::command]
pub async fn list_provider_models(
    r#type: String,
    base_url: String,
    api_key: String,
) -> AppResult<Vec<String>> {
    let adaptor = adapter::get_adaptor(&r#type);

    if base_url.trim().is_empty() {
        return Ok(adaptor
            .default_models()
            .into_iter()
            .map(|s| s.to_string())
            .collect());
    }

    let config = adapter::ChannelConfig {
        base_url,
        api_key,
        models: vec![],
        model_mapping: serde_json::json!({}),
        extra: serde_json::json!({}),
        timeout_secs: 15,
        stream: false,
    };

    match adaptor.list_models(&config).await {
        Ok(models) if !models.is_empty() => Ok(models),
        Ok(_) => Err(AppError::Proxy("上游返回的模型列表为空".into())),
        Err(e) => Err(AppError::Proxy(format!("拉取模型失败：{e}"))),
    }
}

/// List provider presets grouped by protocol — the single source of truth for
/// the channel-type picker. The full registry is returned so the frontend
/// never hard-codes base URLs, model suggestions, or native endpoints.
#[tauri::command]
pub async fn list_provider_presets(
) -> AppResult<Vec<crate::channel_presets::ProtocolPresetGroup>> {
    Ok(crate::channel_presets::groups_for_protocols())
}
