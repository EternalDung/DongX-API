use crate::adapter::{
    build_client, extract_usage, map_model, normalize_developer_role, Adaptor, ChannelConfig,
    ProxyRequest, TestResult, TokenUsage,
};
use async_trait::async_trait;
use serde_json::json;

/// OpenAI adaptor — also the canonical OpenAI-compatible implementation
/// reused by zhipu / ollama / moonshot / qwen via `get_adaptor()`.
pub struct OpenAIAdaptor;

impl OpenAIAdaptor {
    fn request_url(&self, config: &ChannelConfig) -> String {
        let base = config.base_url.trim_end_matches('/');
        // Preset base_url already contains the version segment (e.g. ".../v1")
        format!("{}/chat/completions", base)
    }

    /// Shared request body builder (model mapping + role normalization).
    fn build_body(&self, request: &ProxyRequest, config: &ChannelConfig) -> serde_json::Value {
        let mut body = request.body.clone();
        body["model"] = json!(map_model(request, config));
        // Upstreams that don't accept OpenAI's `developer` role would reject the
        // request (e.g. Codex sends developer-role messages). Map to `system`.
        normalize_developer_role(&mut body);
        body
    }
}

#[async_trait]
impl Adaptor for OpenAIAdaptor {
    fn channel_type(&self) -> &'static str {
        "openai"
    }

    fn default_models(&self) -> Vec<&'static str> {
        vec!["gpt-4o", "gpt-4o-mini", "gpt-4.1", "o3-mini"]
    }

    fn default_base_url(&self) -> &str {
        "https://api.openai.com/v1"
    }

    async fn test(&self, config: &ChannelConfig) -> Result<TestResult, anyhow::Error> {
        let client = build_client(config)?;
        let body = json!({
            "model": config.models.first().map(String::as_str)
                .unwrap_or_else(|| self.default_models().first().copied().unwrap_or("gpt-4o-mini")),
            "messages": [{ "role": "user", "content": "ping" }],
            "max_tokens": 5,
            "stream": false,
        });

        let start = std::time::Instant::now();
        let resp = client
            .post(self.request_url(config))
            .bearer_auth(&config.api_key)
            .json(&body)
            .send()
            .await;
        let latency = start.elapsed().as_millis() as u64;

        match resp {
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    Ok(TestResult {
                        success: true,
                        message: "OK".into(),
                        latency_ms: latency,
                    })
                } else {
                    let text = r.text().await.unwrap_or_default();
                    Ok(TestResult {
                        success: false,
                        message: format!("HTTP {}: {}", status.as_u16(), truncate(&text, 200)),
                        latency_ms: latency,
                    })
                }
            }
            Err(e) => Ok(TestResult {
                success: false,
                message: e.to_string(),
                latency_ms: latency,
            }),
        }
    }

    async fn forward(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<(u16, serde_json::Value, Option<TokenUsage>), anyhow::Error> {
        let client = build_client(config)?;
        let resp = client
            .post(self.request_url(config))
            .bearer_auth(&config.api_key)
            .json(&self.build_body(request, config))
            .send()
            .await?;

        let status = resp.status().as_u16();
        // Read the raw body once, then parse, so a non-JSON upstream response
        // (proxy/HTML error page, or an undecoded compressed body) surfaces the
        // real HTTP status + a snippet instead of an opaque
        // "error decoding response body".
        let text = resp.text().await?;
        let body: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
            let snippet: String = text.chars().take(300).collect();
            anyhow::anyhow!("上游返回非 JSON（HTTP {}）：{}", status, snippet)
        })?;
        let usage = extract_usage(&body);
        Ok((status, body, usage))
    }

    async fn forward_stream(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<reqwest::Response, anyhow::Error> {
        let client = build_client(config)?;
        let mut body = self.build_body(request, config);
        body["stream"] = json!(true);
        body["stream_options"] = json!({ "include_usage": true });

        let resp = client
            .post(self.request_url(config))
            .bearer_auth(&config.api_key)
            .json(&body)
            .send()
            .await?;
        Ok(resp)
    }
}

pub(crate) fn truncate(s: &str, max: usize) -> String {
    if s.len() <= max {
        s.to_string()
    } else {
        format!("{}...", &s[..max])
    }
}
