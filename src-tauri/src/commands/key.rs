use serde::{Deserialize, Serialize};
use crate::error::AppResult;

/// API key creation payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApiKeyInput {
    pub name: String,
    pub allowed_models: Vec<String>,
    pub allowed_channels: Vec<String>,
    pub quota_limit: i64,
    pub expires_at: Option<String>,
}

/// List all gateway API keys
#[tauri::command]
pub async fn list_api_keys() -> AppResult<Vec<serde_json::Value>> {
    // TODO: Query from database
    Ok(vec![])
}

/// Create a new gateway API key
#[tauri::command]
pub async fn create_api_key(input: ApiKeyInput) -> AppResult<serde_json::Value> {
    // TODO: Generate sk-dong-<random>, hash it, store, return plaintext once
    let _ = input;
    let key = format!("sk-dong-{}", uuid::Uuid::new_v4().simple());
    Ok(serde_json::json!({
        "id": uuid::Uuid::new_v4().to_string(),
        "key": key,
        "status": "created"
    }))
}

/// Update an API key
#[tauri::command]
pub async fn update_api_key(id: String, input: ApiKeyInput) -> AppResult<()> {
    let _ = (id, input);
    Ok(())
}

/// Delete an API key
#[tauri::command]
pub async fn delete_api_key(id: String) -> AppResult<()> {
    let _ = id;
    Ok(())
}
