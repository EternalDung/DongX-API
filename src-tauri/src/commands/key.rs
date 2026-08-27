use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;

use crate::crypto;
use crate::db::repository::gateway_keys;
use crate::error::{AppError, AppResult};
use crate::models::GatewayKeyRow;
use crate::AppState;

/// API key creation/update payload (mirrors frontend ApiKeyInput).
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApiKeyInput {
    pub name: String,
    pub allowed_models: Vec<String>,
    pub allowed_channels: Vec<String>,
    pub quota_limit: i64,
    pub expires_at: Option<String>,
}

/// Serialize a `gateway_keys` row into the frontend wire shape.
/// `key` here is the plaintext gateway key stored locally.
fn row_to_value(row: GatewayKeyRow) -> serde_json::Value {
    let allowed_models: Vec<String> = serde_json::from_str(&row.allowed_models).unwrap_or_default();
    let allowed_channels: Vec<String> =
        serde_json::from_str(&row.allowed_channels).unwrap_or_default();
    serde_json::json!({
        "id": row.id,
        "name": row.name,
        "key": row.key,
        "status": row.status,
        "allowed_models": allowed_models,
        "allowed_channels": allowed_channels,
        "quota_limit": row.quota_limit,
        "quota_used": row.quota_used,
        "expires_at": row.expires_at,
        "created_at": row.created_at,
        "updated_at": row.updated_at,
    })
}

/// List all gateway API keys (masked).
#[tauri::command]
pub async fn list_api_keys(state: State<'_, Arc<AppState>>) -> AppResult<Vec<serde_json::Value>> {
    let rows = gateway_keys::list(&state.db).await?;
    Ok(rows.into_iter().map(row_to_value).collect())
}

/// Create a new gateway API key. Returns the plaintext key exactly once.
#[tauri::command]
pub async fn create_api_key(
    input: ApiKeyInput,
    state: State<'_, Arc<AppState>>,
) -> AppResult<serde_json::Value> {
    if input.name.trim().is_empty() {
        return Err(AppError::Validation("密钥名称不能为空".into()));
    }

    // Generate a plaintext gateway key. DongX is a local single-user gateway,
    // so keys are stored in plaintext (no hashing).
    let plaintext = crypto::generate_api_key();

    let allowed_models = serde_json::to_string(&input.allowed_models)?;
    let allowed_channels = serde_json::to_string(&input.allowed_channels)?;

    let row = gateway_keys::insert(
        &state.db,
        &input.name,
        &plaintext,
        &allowed_models,
        &allowed_channels,
        input.quota_limit,
        input.expires_at.as_deref(),
    )
    .await?;

    // Return plaintext once — frontend shows it once and never again.
    Ok(serde_json::json!({
        "id": row.id,
        "key": plaintext,
        "name": row.name,
        "status": "created",
    }))
}

/// Update an existing gateway API key (name / scopes / quota / expiry).
#[tauri::command]
pub async fn update_api_key(
    id: String,
    input: ApiKeyInput,
    state: State<'_, Arc<AppState>>,
) -> AppResult<serde_json::Value> {
    let existing = gateway_keys::get_by_id(&state.db, &id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("API key {} 不存在", id)))?;

    let allowed_models = serde_json::to_string(&input.allowed_models)?;
    let allowed_channels = serde_json::to_string(&input.allowed_channels)?;

    gateway_keys::update(
        &state.db,
        &id,
        &input.name,
        &allowed_models,
        &allowed_channels,
        input.quota_limit,
        input.expires_at.as_deref(),
        existing.status,
    )
    .await?;

    Ok(serde_json::json!({ "status": "updated" }))
}

/// Delete a gateway API key.
#[tauri::command]
pub async fn delete_api_key(id: String, state: State<'_, Arc<AppState>>) -> AppResult<()> {
    let n = gateway_keys::delete(&state.db, &id).await?;
    if n == 0 {
        return Err(AppError::NotFound(format!("API key {} 不存在", id)));
    }
    Ok(())
}

/// Enable / disable a gateway key by id.
#[tauri::command]
pub async fn set_api_key_status(
    id: String,
    status: i32,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    gateway_keys::set_status(&state.db, &id, status)
        .await
        .map_err(|e| AppError::Internal(e.to_string()))?;
    Ok(())
}
