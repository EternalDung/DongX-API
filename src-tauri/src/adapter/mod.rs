pub mod openai;
pub mod anthropic;
pub mod ollama;

use crate::error::AppResult;
use serde_json::Value;

/// Adapter trait: convert OpenAI-compatible request to upstream format
/// and convert upstream response back to OpenAI-compatible format
pub trait ProviderAdapter {
    /// Convert an OpenAI chat completion request to the upstream format
    fn adapt_request(&self, request: &Value) -> AppResult<Value>;

    /// Convert the upstream response back to OpenAI format
    fn adapt_response(&self, response: &Value) -> AppResult<Value>;

    /// Get the upstream endpoint path for this provider
    fn endpoint(&self) -> &str;

    /// Get additional headers to inject (e.g., anthropic-version)
    fn extra_headers(&self) -> Vec<(String, String)> {
        vec![]
    }
}

/// Get the appropriate adapter for a channel protocol
pub fn get_adapter(protocol: &str) -> Box<dyn ProviderAdapter> {
    match protocol {
        "openai" => Box::new(openai::OpenAiAdapter),
        "anthropic" => Box::new(anthropic::AnthropicAdapter),
        "ollama" => Box::new(ollama::OllamaAdapter),
        _ => Box::new(openai::OpenAiAdapter), // default to OpenAI compatible
    }
}
