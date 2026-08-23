use crate::adapter::openai::OpenAIAdaptor;
use crate::adapter::{Adaptor, ChannelConfig, ProxyRequest, TestResult};
use async_trait::async_trait;
/// Google Gemini adaptor — uses Google's official OpenAI-compatible endpoint
/// (`/v1beta/openai/chat/completions`), so it delegates to the OpenAI adaptor.
///
/// If native Gemini API conversion is ever needed (non-OpenAI-compatible
/// endpoint), this is the place to implement `to_gemini_request`.
pub struct GeminiAdaptor(OpenAIAdaptor);

#[async_trait]
impl Adaptor for GeminiAdaptor {
    fn channel_type(&self) -> &'static str {
        "gemini"
    }

    fn default_models(&self) -> Vec<&'static str> {
        vec!["gemini-2.0-flash", "gemini-2.5-pro", "gemini-2.5-flash"]
    }

    fn default_base_url(&self) -> &str {
        "https://generativelanguage.googleapis.com/v1beta/openai"
    }

    async fn test(&self, config: &ChannelConfig) -> Result<TestResult, anyhow::Error> {
        self.0.test(config).await
    }

    async fn forward(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<(u16, serde_json::Value, Option<crate::adapter::TokenUsage>), anyhow::Error> {
        self.0.forward(request, config).await
    }

    async fn forward_stream(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<reqwest::Response, anyhow::Error> {
        self.0.forward_stream(request, config).await
    }
}
