use crate::adapter::{
    build_client, extract_usage, map_model, normalize_developer_role, Adaptor, ChannelConfig,
    ProxyRequest, TestResult, TokenUsage,
};
use async_trait::async_trait;
use serde_json::json;

/// Custom adaptor — OpenAI-compatible passthrough for any user-defined
/// endpoint (self-hosted vLLM, LiteLLM, one-api, ...).
///
/// Differs from the OpenAI adaptor only in defaults; the endpoint path can
/// be overridden via `config.extra["chat_path"]`.
pub struct CustomAdaptor;

impl CustomAdaptor {
    fn chat_path(&self, config: &ChannelConfig) -> String {
        config
            .extra
            .get("chat_path")
            .and_then(|v| v.as_str())
            .unwrap_or("/chat/completions")
            .to_string()
    }

    fn request_url(&self, config: &ChannelConfig) -> String {
        let base = config.base_url.trim_end_matches('/');
        format!("{}{}", base, self.chat_path(config))
    }
}

#[async_trait]
impl Adaptor for CustomAdaptor {
    fn channel_type(&self) -> &'static str {
        "custom"
    }

    fn default_models(&self) -> Vec<&'static str> {
        vec![] // user must supply models explicitly
    }

    fn default_base_url(&self) -> &str {
        ""
    }

    async fn test(&self, config: &ChannelConfig) -> Result<TestResult, anyhow::Error> {
        if config.base_url.is_empty() {
            return Ok(TestResult {
                success: false,
                message: "base_url is empty".into(),
                latency_ms: 0,
            });
        }
        if config.models.is_empty() {
            return Ok(TestResult {
                success: false,
                message: "no model configured".into(),
                latency_ms: 0,
            });
        }

        let client = build_client(config)?;
        let body = json!({
            "model": config.models[0],
            "messages": [{ "role": "user", "content": "ping" }],
            "max_tokens": 5,
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
                        message: format!(
                            "HTTP {}: {}",
                            status.as_u16(),
                            crate::adapter::openai::truncate(&text, 200)
                        ),
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
        let mut body = request.body.clone();
        body["model"] = json!(map_model(request, config));
        // Upstreams that don't accept OpenAI's `developer` role would reject the
        // request (e.g. Codex sends developer-role messages). Map to `system`.
        normalize_developer_role(&mut body);

        let resp = client
            .post(self.request_url(config))
            .bearer_auth(&config.api_key)
            .json(&body)
            .send()
            .await?;

        let status = resp.status().as_u16();
        let body: serde_json::Value = resp.json().await?;
        let usage = extract_usage(&body);
        Ok((status, body, usage))
    }

    async fn forward_stream(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<reqwest::Response, anyhow::Error> {
        let client = build_client(config)?;
        let mut body = request.body.clone();
        body["model"] = json!(map_model(request, config));
        // Upstreams that don't accept OpenAI's `developer` role would reject the
        // request (e.g. Codex sends developer-role messages). Map to `system`.
        normalize_developer_role(&mut body);
        body["stream"] = json!(true);

        let resp = client
            .post(self.request_url(config))
            .bearer_auth(&config.api_key)
            .json(&body)
            .send()
            .await?;
        Ok(resp)
    }
}
