pub mod claude;
pub mod custom;
pub mod deepseek;
pub mod gemini;
pub mod openai;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::time::Duration;

/// Channel runtime config passed to adaptors (built from the `channels` row).
///
/// Java mental model: like a Spring `@ConfigurationProperties` POJO that the
/// service layer assembles before handing work to a strategy bean.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelConfig {
    pub base_url: String,
    pub api_key: String,
    pub models: Vec<String>,
    pub model_mapping: serde_json::Value,
    pub extra: serde_json::Value,
    pub timeout_secs: u64,
}

/// A normalized (OpenAI-format) request to be forwarded upstream.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyRequest {
    pub model: String,
    pub body: serde_json::Value,
    pub stream: bool,
}

/// Connectivity test result.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TestResult {
    pub success: bool,
    pub message: String,
    pub latency_ms: u64,
}

/// Token usage extracted from an upstream response (both shapes supported).
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct TokenUsage {
    pub prompt_tokens: u64,
    pub completion_tokens: u64,
    pub total_tokens: u64,
}

/// Strategy interface: one implementation per provider protocol.
///
/// Java mental model: `Adaptor` ≈ a Strategy interface; `get_adaptor()` ≈
/// a factory returning the strategy bean for the channel type.
///
/// Note: `#[async_trait]` is required because we return `Box<dyn Adaptor>`;
/// Rust 1.75's native async-fn-in-trait does not allow trait objects (dyn).
#[async_trait]
pub trait Adaptor: Send + Sync {
    fn channel_type(&self) -> &'static str;
    fn default_models(&self) -> Vec<&'static str>;
    fn default_base_url(&self) -> &str;

    /// Test channel connectivity with a minimal request; measure latency.
    async fn test(&self, config: &ChannelConfig) -> Result<TestResult, anyhow::Error>;

    /// Forward a non-streaming request. Returns (status, body, usage).
    async fn forward(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<(u16, serde_json::Value, Option<TokenUsage>), anyhow::Error>;

    /// Forward a streaming (SSE) request. Returns the raw upstream response;
    /// the caller streams `bytes_stream()` through to the client.
    async fn forward_stream(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<reqwest::Response, anyhow::Error>;
}

/// Factory: resolve the adaptor for a channel type.
///
/// Providers that are OpenAI-compatible (zhipu, ollama, moonshot, ...) are
/// routed to the OpenAI adaptor instead of getting their own module.
pub fn get_adaptor(channel_type: &str) -> Box<dyn Adaptor> {
    match channel_type {
        "openai" => Box::new(openai::OpenAIAdaptor),
        "deepseek" => Box::new(deepseek::DeepSeekAdaptor::new()),
        "claude" => Box::new(claude::ClaudeAdaptor),
        "gemini" => Box::new(gemini::GeminiAdaptor),
        // OpenAI-compatible providers reuse the OpenAI adaptor
        "zhipu" | "ollama" | "moonshot" | "qwen" => Box::new(openai::OpenAIAdaptor),
        _ => Box::new(custom::CustomAdaptor),
    }
}

// ---------------------------------------------------------------------------
// Shared helpers used by all adaptor implementations
// ---------------------------------------------------------------------------

/// Build an HTTP client honouring the channel's timeout config.
pub(crate) fn build_client(config: &ChannelConfig) -> Result<reqwest::Client, anyhow::Error> {
    Ok(reqwest::Client::builder()
        .timeout(Duration::from_secs(config.timeout_secs.max(1)))
        .build()?)
}

/// Extract token usage from an OpenAI-shaped or Anthropic-shaped body.
pub(crate) fn extract_usage(body: &serde_json::Value) -> Option<TokenUsage> {
    let usage = body.get("usage")?;
    // OpenAI shape: prompt_tokens / completion_tokens / total_tokens
    if let (Some(p), Some(c)) = (
        usage.get("prompt_tokens").and_then(|v| v.as_u64()),
        usage.get("completion_tokens").and_then(|v| v.as_u64()),
    ) {
        return Some(TokenUsage {
            prompt_tokens: p,
            completion_tokens: c,
            total_tokens: usage
                .get("total_tokens")
                .and_then(|v| v.as_u64())
                .unwrap_or(p + c),
        });
    }
    // Anthropic shape: input_tokens / output_tokens
    if let (Some(p), Some(c)) = (
        usage.get("input_tokens").and_then(|v| v.as_u64()),
        usage.get("output_tokens").and_then(|v| v.as_u64()),
    ) {
        return Some(TokenUsage {
            prompt_tokens: p,
            completion_tokens: c,
            total_tokens: p + c,
        });
    }
    None
}

/// Apply model mapping if configured: gateway model -> upstream model.
pub(crate) fn map_model(request: &ProxyRequest, config: &ChannelConfig) -> String {
    config
        .model_mapping
        .get(&request.model)
        .and_then(|v| v.as_str())
        .map(str::to_string)
        .unwrap_or_else(|| request.model.clone())
}
