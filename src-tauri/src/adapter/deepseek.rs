use crate::adapter::openai::OpenAIAdaptor;
use crate::adapter::{Adaptor, ChannelConfig, ProxyRequest, TestResult};
use async_trait::async_trait;

/// DeepSeek adaptor — the API is fully OpenAI-compatible, so delegate
/// everything to the OpenAI adaptor and only override metadata.
///
/// Java mental model: like subclassing a default strategy because the
/// vendor happens to implement the same contract.
pub struct DeepSeekAdaptor(OpenAIAdaptor);

impl DeepSeekAdaptor {
    /// Public constructor used by the adaptor factory in `adapter/mod.rs`.
    pub fn new() -> Self {
        Self(OpenAIAdaptor)
    }
}

#[async_trait]
impl Adaptor for DeepSeekAdaptor {
    fn channel_type(&self) -> &'static str {
        "deepseek"
    }

    fn default_models(&self) -> Vec<&'static str> {
        vec!["deepseek-chat", "deepseek-reasoner"]
    }

    fn default_base_url(&self) -> &str {
        "https://api.deepseek.com"
    }

    async fn test(&self, config: &ChannelConfig) -> Result<TestResult, anyhow::Error> {
        self.0.test(config).await
    }

    async fn forward(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<
        (
            u16,
            serde_json::Value,
            Option<crate::adapter::TokenUsage>,
            Option<String>,
        ),
        anyhow::Error,
    > {
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
