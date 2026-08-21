use crate::adapter::ProviderAdapter;
use crate::error::{AppError, AppResult};
use serde_json::{json, Value};

/// Anthropic Claude adapter: converts OpenAI chat format to Anthropic Messages API
pub struct AnthropicAdapter;

impl ProviderAdapter for AnthropicAdapter {
    fn adapt_request(&self, request: &Value) -> AppResult<Value> {
        // Convert OpenAI chat/completions request to Anthropic /v1/messages format
        // OpenAI: { model, messages: [{role, content}], max_tokens, temperature, stream }
        // Anthropic: { model, messages: [{role, content}], max_tokens, system, ... }

        let messages = request.get("messages")
            .ok_or_else(|| AppError::Validation("Missing messages field".into()))?
            .as_array()
            .ok_or_else(|| AppError::Validation("messages must be array".into()))?;

        // Extract system message if present
        let mut system_msg = String::new();
        let mut anthropic_messages = Vec::new();

        for msg in messages {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = msg.get("content").and_then(|v| v.as_str()).unwrap_or("");
            if role == "system" {
                system_msg = content.to_string();
            } else {
                anthropic_messages.push(json!({
                    "role": role,
                    "content": content
                }));
            }
        }

        let mut result = json!({
            "model": request.get("model").cloned().unwrap_or(json!("claude-sonnet-4-20250514")),
            "messages": anthropic_messages,
            "max_tokens": request.get("max_tokens").cloned().unwrap_or(json!(4096)),
        });

        if !system_msg.is_empty() {
            result["system"] = json!(system_msg);
        }

        if let Some(temp) = request.get("temperature") {
            result["temperature"] = temp.clone();
        }

        if request.get("stream").and_then(|v| v.as_bool()).unwrap_or(false) {
            result["stream"] = json!(true);
        }

        Ok(result)
    }

    fn adapt_response(&self, response: &Value) -> AppResult<Value> {
        // TODO: Convert Anthropic response back to OpenAI format
        // For now, passthrough (requires proper conversion)
        Ok(response.clone())
    }

    fn endpoint(&self) -> &str {
        "/v1/messages"
    }

    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![("anthropic-version".into(), "2023-06-01".into())]
    }
}
