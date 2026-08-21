use serde::{Deserialize, Serialize};
use crate::error::AppResult;

/// Channel creation/update payload
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ChannelInput {
    pub name: String,
    pub protocol: String,
    pub r#type: String,
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub priority: i32,
    pub weight: i32,
    pub config: serde_json::Value,
    pub model_mapping: serde_json::Value,
    pub endpoints: Vec<String>,
}

/// Provider preset for channel creation UI
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "snake_case")]
pub struct ProviderPresetDto {
    pub r#type: String,
    pub label: String,
    pub default_base_url: String,
    pub requires_api_key: bool,
}

/// List all channels
#[tauri::command]
pub async fn list_channels() -> AppResult<Vec<serde_json::Value>> {
    // TODO: Query from database
    Ok(vec![])
}

/// Create a new channel
#[tauri::command]
pub async fn create_channel(input: ChannelInput) -> AppResult<serde_json::Value> {
    // TODO: Encrypt API key, insert into database
    let _ = input;
    Ok(serde_json::json!({ "id": uuid::Uuid::new_v4().to_string(), "status": "created" }))
}

/// Update an existing channel
#[tauri::command]
pub async fn update_channel(id: String, input: ChannelInput) -> AppResult<()> {
    // TODO: Update database record
    let _ = (id, input);
    Ok(())
}

/// Delete a channel
#[tauri::command]
pub async fn delete_channel(id: String) -> AppResult<()> {
    // TODO: Delete from database
    let _ = id;
    Ok(())
}

/// Test channel connectivity
#[tauri::command]
pub async fn test_channel(id: String) -> AppResult<bool> {
    // TODO: Send test request to upstream
    let _ = id;
    Ok(false)
}

/// List provider presets (grouped by protocol)
#[tauri::command]
pub async fn list_provider_presets() -> AppResult<Vec<ProviderPresetDto>> {
    Ok(vec![
        ProviderPresetDto {
            r#type: "openai".into(),
            label: "OpenAI".into(),
            default_base_url: "https://api.openai.com/v1".into(),
            requires_api_key: true,
        },
        ProviderPresetDto {
            r#type: "deepseek".into(),
            label: "DeepSeek".into(),
            default_base_url: "https://api.deepseek.com".into(),
            requires_api_key: true,
        },
        ProviderPresetDto {
            r#type: "claude".into(),
            label: "Anthropic Claude".into(),
            default_base_url: "https://api.anthropic.com".into(),
            requires_api_key: true,
        },
        ProviderPresetDto {
            r#type: "gemini".into(),
            label: "Google Gemini".into(),
            default_base_url: "https://generativelanguage.googleapis.com/v1beta/openai".into(),
            requires_api_key: true,
        },
        ProviderPresetDto {
            r#type: "zhipu".into(),
            label: "智谱 GLM".into(),
            default_base_url: "https://open.bigmodel.cn/api/paas/v4".into(),
            requires_api_key: true,
        },
        ProviderPresetDto {
            r#type: "ollama".into(),
            label: "Ollama (本地)".into(),
            default_base_url: "http://localhost:11434".into(),
            requires_api_key: false,
        },
        ProviderPresetDto {
            r#type: "custom".into(),
            label: "自定义".into(),
            default_base_url: "".into(),
            requires_api_key: true,
        },
    ])
}
