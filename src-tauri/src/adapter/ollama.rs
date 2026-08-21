use crate::adapter::ProviderAdapter;
use crate::error::AppResult;
use serde_json::Value;

/// Ollama adapter: converts OpenAI chat format to Ollama API format
pub struct OllamaAdapter;

impl ProviderAdapter for OllamaAdapter {
    fn adapt_request(&self, request: &Value) -> AppResult<Value> {
        // Ollama's /api/chat endpoint accepts a similar format but with model-specific fields
        // For OpenAI compatibility, Ollama also supports /v1/chat/completions directly
        // We'll use the OpenAI-compatible endpoint for simplicity
        Ok(request.clone())
    }

    fn adapt_response(&self, response: &Value) -> AppResult<Value> {
        Ok(response.clone())
    }

    fn endpoint(&self) -> &str {
        // Ollama supports OpenAI-compatible endpoint
        "/v1/chat/completions"
    }
}
