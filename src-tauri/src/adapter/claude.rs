use crate::adapter::{
    build_client, ensure_scheme, extract_usage, map_model, parse_model_ids, Adaptor, ChannelConfig,
    ProxyRequest, SseRecord, StreamUsage, TestResult, TokenUsage,
};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Anthropic Claude adaptor — the only provider needing real protocol
/// conversion (OpenAI chat format <-> Anthropic Messages API).
pub struct ClaudeAdaptor;

impl ClaudeAdaptor {
    fn request_url(&self, config: &ChannelConfig) -> String {
        let base = config.base_url.trim_end_matches('/');
        format!("{}/v1/messages", base)
    }

    /// OpenAI request -> Anthropic /v1/messages request
    fn to_anthropic_request(&self, request: &ProxyRequest, config: &ChannelConfig) -> Value {
        let messages = request
            .body
            .get("messages")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut system = String::new();
        let mut converted: Vec<Value> = Vec::new();

        for msg in &messages {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = msg.get("content");
            match role {
                "system" | "developer" => {
                    // Anthropic takes system prompt as a top-level field.
                    // OpenAI's `developer` role is a more stable variant of
                    // `system` (emitted by Codex/o-series); fold it in here.
                    if let Some(text) = content.and_then(|c| c.as_str()) {
                        system = text.to_string();
                    }
                }
                "assistant" | "user" => {
                    converted.push(json!({ "role": role, "content": content }));
                }
                _ => {} // tool/function roles: TODO when tool-calling lands
            }
        }

        let mut result = json!({
            "model": map_model(request, config),
            "messages": converted,
            "max_tokens": request.body.get("max_tokens").and_then(|v| v.as_u64()).unwrap_or(4096),
        });

        if !system.is_empty() {
            result["system"] = json!(system);
        }
        if let Some(t) = request.body.get("temperature") {
            result["temperature"] = t.clone();
        }
        if request.stream {
            result["stream"] = json!(true);
        }
        result
    }

    /// Anthropic response -> OpenAI chat completion response
    fn to_openai_response(&self, model: &str, body: &Value) -> Value {
        let text = body
            .get("content")
            .and_then(|c| c.as_array())
            .map(|blocks| {
                blocks
                    .iter()
                    .filter_map(|b| {
                        if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                            b.get("text").and_then(|t| t.as_str())
                        } else {
                            None
                        }
                    })
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        let usage = body.get("usage").cloned().unwrap_or(json!({}));

        json!({
            "id": body.get("id").cloned().unwrap_or(json!("")),
            "object": "chat.completion",
            "created": std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0),
            "model": model,
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": text },
                "finish_reason": "stop",
            }],
            "usage": {
                "prompt_tokens": usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                "completion_tokens": usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
                "total_tokens":
                    usage.get("input_tokens").and_then(|v| v.as_u64()).unwrap_or(0)
                    + usage.get("output_tokens").and_then(|v| v.as_u64()).unwrap_or(0),
            },
        })
    }
}

#[async_trait]
impl Adaptor for ClaudeAdaptor {
    fn channel_type(&self) -> &'static str {
        "claude"
    }

    fn default_models(&self) -> Vec<&'static str> {
        vec!["claude-sonnet-4-20250514", "claude-opus-4-20250514"]
    }

    fn default_base_url(&self) -> &str {
        "https://api.anthropic.com"
    }

    /// Anthropic lists models at `GET /v1/models` using the `x-api-key` header
    /// (not Bearer), so override the default OpenAI-compatible implementation.
    async fn list_models(&self, config: &ChannelConfig) -> Result<Vec<String>, anyhow::Error> {
        let client = build_client(config)?;
        let url = ensure_scheme(&format!(
            "{}/v1/models",
            config.base_url.trim_end_matches('/')
        ));
        let resp = client
            .get(&url)
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .send()
            .await?;
        if !resp.status().is_success() {
            anyhow::bail!("list models failed: upstream status {}", resp.status());
        }
        let json: serde_json::Value = resp.json().await?;
        Ok(parse_model_ids(&json, "data", "id"))
    }

    async fn test(&self, config: &ChannelConfig) -> Result<TestResult, anyhow::Error> {
        let client = build_client(config)?;
        let body = json!({
            "model": config.models.first().map(String::as_str)
                .unwrap_or_else(|| self.default_models().first().copied().unwrap_or("claude-sonnet-4-20250514")),
            "messages": [{ "role": "user", "content": "ping" }],
            "max_tokens": 5,
        });

        let start = std::time::Instant::now();
        let resp = client
            .post(self.request_url(config))
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await;
        let latency = start.elapsed().as_millis() as u64;

        match resp {
            Ok(r) => {
                let status = r.status();
                if status.is_success() {
                    Ok(TestResult { success: true, message: "OK".into(), latency_ms: latency })
                } else {
                    let text = r.text().await.unwrap_or_default();
                    Ok(TestResult {
                        success: false,
                        message: format!("HTTP {}: {}", status.as_u16(),
                            crate::adapter::openai::truncate(&text, 200)),
                        latency_ms: latency,
                    })
                }
            }
            Err(e) => Ok(TestResult { success: false, message: e.to_string(), latency_ms: latency }),
        }
    }

    async fn forward(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<(u16, Value, Option<TokenUsage>), anyhow::Error> {
        let client = build_client(config)?;
        let resp = client
            .post(self.request_url(config))
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&self.to_anthropic_request(request, config))
            .send()
            .await?;

        let status = resp.status().as_u16();
        let body: Value = resp.json().await?;

        // Non-streaming: convert back to OpenAI format; usage moves from
        // input/output_tokens into prompt/completion_tokens.
        let usage = extract_usage(&body);
        let converted = self.to_openai_response(&request.model, &body);
        Ok((status, converted, usage))
    }

    async fn forward_stream(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<reqwest::Response, anyhow::Error> {
        // Streaming SSE stays in Anthropic event format; converting the event
        // stream chunk-by-chunk happens in the proxy layer (TODO there).
        let client = build_client(config)?;
        let mut body = self.to_anthropic_request(request, config);
        body["stream"] = json!(true);

        let resp = client
            .post(self.request_url(config))
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&body)
            .send()
            .await?;
        Ok(resp)
    }
}

// ---------------------------------------------------------------------------
// Streaming: Anthropic SSE -> OpenAI SSE (chunk-by-chunk)
// ---------------------------------------------------------------------------

/// Stateful converter: turns Anthropic `/v1/messages` SSE events into OpenAI
/// `chat.completion.chunk` `data:` frames. One instance lives for the whole
/// upstream stream, because it needs to emit the `role` exactly once and to
/// know when the stream is finished.
pub struct AnthropicSseConverter {
    model: String,
    id: String,
    role_emitted: bool,
    finished: bool,
}

impl AnthropicSseConverter {
    pub fn new(model: String) -> Self {
        Self {
            model,
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()),
            role_emitted: false,
            finished: false,
        }
    }

    /// Convert one Anthropic SSE record. Returns zero or more OpenAI `data:`
    /// frames (each terminated with `\n\n`). `acc` accumulates token usage.
    pub fn convert(&mut self, record: &SseRecord, acc: &mut StreamUsage) -> Vec<String> {
        if self.finished {
            return Vec::new();
        }
        let data = &record.data;
        if data.is_empty() || data == "[DONE]" {
            return Vec::new();
        }
        let Ok(json) = serde_json::from_str::<Value>(data) else {
            return Vec::new();
        };
        let typ = json.get("type").and_then(|t| t.as_str()).unwrap_or("");
        let mut frames: Vec<String> = Vec::new();

        match typ {
            "message_start" => {
                // Capture input tokens early; output tokens come at message_delta.
                if let Some(u) = json.pointer("/message/usage") {
                    acc.prompt_tokens = u
                        .get("input_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                }
            }
            "content_block_delta" => {
                let delta = json.get("delta");
                let kind = delta
                    .and_then(|d| d.get("type"))
                    .and_then(|t| t.as_str())
                    .unwrap_or("");
                // Map the two text-bearing Anthropic deltas to OpenAI deltas.
                // `thinking_delta` -> `reasoning_content` (OpenAI-compatible
                // extended field; DongX's log viewer already renders it).
                let field_and_text = match kind {
                    "text_delta" => delta
                        .and_then(|d| d.get("text"))
                        .and_then(|t| t.as_str())
                        .map(|t| ("content", t)),
                    "thinking_delta" => delta
                        .and_then(|d| d.get("thinking"))
                        .and_then(|t| t.as_str())
                        .map(|t| ("reasoning_content", t)),
                    _ => None, // input_json_delta (tools) — unsupported yet, skip
                };
                if let Some((field, text)) = field_and_text {
                    if !self.role_emitted {
                        frames.push(self.role_frame());
                    }
                    frames.push(self.delta_frame(serde_json::json!({ field: text })));
                }
            }
            "message_delta" => {
                if let Some(u) = json.get("usage") {
                    acc.completion_tokens = u
                        .get("output_tokens")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(0);
                }
                let stop = json
                    .pointer("/delta/stop_reason")
                    .and_then(|s| s.as_str())
                    .unwrap_or("end_turn");
                frames.push(self.finish_frame(map_anthropic_stop(stop)));
                self.finished = true;
            }
            "message_stop" => {
                if !self.finished {
                    frames.push(self.finish_frame("stop"));
                    self.finished = true;
                }
            }
            _ => {} // ping, errors, etc. ignored
        }
        frames
    }

    fn role_frame(&mut self) -> String {
        self.role_emitted = true;
        self.chunk(json!({ "role": "assistant" }), None)
    }

    fn delta_frame(&self, delta: Value) -> String {
        self.chunk(delta, None)
    }

    fn finish_frame(&self, finish_reason: &str) -> String {
        self.chunk(json!({}), Some(finish_reason))
    }

    fn chunk(&self, delta: Value, finish_reason: Option<&str>) -> String {
        let mut choice = json!({ "index": 0, "delta": delta });
        if let Some(fr) = finish_reason {
            choice["finish_reason"] = json!(fr);
        }
        format!(
            "data: {}\n\n",
            json!({
                "id": self.id,
                "object": "chat.completion.chunk",
                "created": chrono::Utc::now().timestamp(),
                "model": self.model,
                "choices": [choice]
            })
        )
    }
}

fn map_anthropic_stop(reason: &str) -> &'static str {
    match reason {
        "end_turn" | "stop_sequence" => "stop",
        "max_tokens" => "length",
        "tool_use" => "tool_calls",
        "refusal" => "content_filter",
        _ => "stop",
    }
}
