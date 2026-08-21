use crate::adapter::ProviderAdapter;
use crate::error::AppResult;
use serde_json::Value;

/// OpenAI-compatible adapter (passthrough, no conversion needed)
pub struct OpenAiAdapter;

impl ProviderAdapter for OpenAiAdapter {
    fn adapt_request(&self, request: &Value) -> AppResult<Value> {
        // OpenAI format is the canonical format, passthrough
        Ok(request.clone())
    }

    fn adapt_response(&self, response: &Value) -> AppResult<Value> {
        // Passthrough
        Ok(response.clone())
    }

    fn endpoint(&self) -> &str {
        "/v1/chat/completions"
    }
}
