use crate::adapter::{
    build_client, ensure_scheme, extract_usage, map_model, parse_model_ids, Adaptor, ChannelConfig,
    ProxyRequest, SseRecord, StreamUsage, TestResult, TokenUsage,
};
use async_trait::async_trait;
use serde_json::{json, Value};

/// Google Gemini adaptor — uses the NATIVE Gemini API
/// (`/v1beta/models/{model}:generateContent` and `:streamGenerateContent`),
/// so it performs full OpenAI <-> Gemini protocol conversion here.
///
/// This is deliberately NOT the OpenAI-compatible `/v1beta/openai` endpoint:
/// going native is the whole point of having a separate `gemini` channel type
/// — it lets us map the wire format exactly and use Gemini-specific features.
pub struct GeminiAdaptor;

impl GeminiAdaptor {
    /// Build the native Gemini endpoint URL. Auth is carried via the `?key=`
    /// query param (see `AuthScheme::QueryKey`), not an HTTP header.
    fn request_url(&self, config: &ChannelConfig, model: &str, stream: bool) -> String {
        let base = config.base_url.trim_end_matches('/');
        if stream {
            format!(
                "{}/v1beta/models/{}:streamGenerateContent?key={}&alt=sse",
                base, model, config.api_key
            )
        } else {
            format!(
                "{}/v1beta/models/{}:generateContent?key={}",
                base, model, config.api_key
            )
        }
    }

    /// OpenAI chat request -> Gemini `generateContent` body.
    fn to_gemini_request(&self, request: &ProxyRequest) -> Value {
        let messages = request
            .body
            .get("messages")
            .and_then(|v| v.as_array())
            .cloned()
            .unwrap_or_default();

        let mut system_instruction = None;
        let mut contents: Vec<Value> = Vec::new();

        for msg in &messages {
            let role = msg.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = msg.get("content");
            match role {
                "system" | "developer" => {
                    // OpenAI's `developer` role is a more stable variant of
                    // `system` (emitted by Codex/o-series); fold it into the
                    // Gemini system instruction.
                    if let Some(text) = content.and_then(|c| c.as_str()) {
                        system_instruction = Some(json!({ "parts": [{ "text": text }] }));
                    }
                }
                "assistant" | "user" => {
                    // Skip empty assistant messages unless they carry tool calls.
                    if role == "assistant" {
                        let empty = content
                            .and_then(|c| c.as_str())
                            .map(|s| s.is_empty())
                            .unwrap_or(true);
                        let has_tools = msg
                            .get("tool_calls")
                            .and_then(|t| t.as_array())
                            .map(|a| !a.is_empty())
                            .unwrap_or(false);
                        if empty && !has_tools {
                            continue;
                        }
                    }
                    let text = content.and_then(|c| c.as_str()).unwrap_or("");
                    contents.push(json!({
                        "role": if role == "assistant" { "model" } else { "user" },
                        "parts": [{ "text": text }],
                    }));
                }
                _ => {}
            }
        }

        let mut gemini_body = json!({ "contents": contents });
        if let Some(si) = system_instruction {
            gemini_body["systemInstruction"] = si;
        }
        if let Some(t) = request.body.get("temperature") {
            gemini_body["generationConfig"]["temperature"] = t.clone();
        }
        if let Some(m) = request.body.get("max_tokens") {
            gemini_body["generationConfig"]["maxOutputTokens"] = m.clone();
        }
        if let Some(p) = request.body.get("top_p") {
            gemini_body["generationConfig"]["topP"] = p.clone();
        }
        gemini_body
    }

    /// Gemini `generateContent` response -> OpenAI chat completion response.
    fn to_openai_response(&self, model: &str, gemini: &Value) -> Value {
        let text = gemini
            .get("candidates")
            .and_then(|c| c.as_array())
            .and_then(|arr| arr.first())
            .and_then(|cand| cand.get("content"))
            .and_then(|c| c.get("parts"))
            .and_then(|p| p.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        let usage_meta = gemini.get("usageMetadata");
        let prompt_tokens = usage_meta
            .and_then(|u| u.get("promptTokenCount"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0);
        let completion_tokens = usage_meta
            .and_then(|u| u.get("candidatesTokenCount"))
            .and_then(|t| t.as_u64())
            .unwrap_or(0);
        let total_tokens = usage_meta
            .and_then(|u| u.get("totalTokenCount"))
            .and_then(|t| t.as_u64())
            .unwrap_or(prompt_tokens + completion_tokens);

        json!({
            "id": format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()),
            "object": "chat.completion",
            "created": chrono::Utc::now().timestamp(),
            "model": model,
            "choices": [{
                "index": 0,
                "message": { "role": "assistant", "content": text },
                "finish_reason": "stop",
            }],
            "usage": {
                "prompt_tokens": prompt_tokens,
                "completion_tokens": completion_tokens,
                "total_tokens": total_tokens,
            },
        })
    }
}

#[async_trait]
impl Adaptor for GeminiAdaptor {
    fn channel_type(&self) -> &'static str {
        "gemini"
    }

    fn default_models(&self) -> Vec<&'static str> {
        vec!["gemini-2.5-flash", "gemini-2.5-pro", "gemini-2.0-flash"]
    }

    fn default_base_url(&self) -> &str {
        "https://generativelanguage.googleapis.com"
    }

    /// Gemini 走原生 generateContent，无 OpenAI 兼容的 `/v1/embeddings` 端点。
    async fn forward_embeddings(
        &self,
        _request: &ProxyRequest,
        _config: &ChannelConfig,
    ) -> Result<(u16, serde_json::Value), anyhow::Error> {
        Err(anyhow::anyhow!(
            "Embeddings API 不支持 Google (Gemini) 渠道"
        ))
    }

    /// Gemini lists models at `GET /v1beta/models?key=<api_key>` (native API,
    /// query-string auth, ids under `models[].name` as `models/<id>`). Override
    /// the default OpenAI-compatible implementation.
    async fn list_models(&self, config: &ChannelConfig) -> Result<Vec<String>, anyhow::Error> {
        let client = build_client(config)?;
        let base = config.base_url.trim_end_matches('/');
        let url = ensure_scheme(&format!("{}/v1beta/models?key={}", base, config.api_key));
        let resp = client.get(&url).send().await?;
        if !resp.status().is_success() {
            anyhow::bail!("list models failed: upstream status {}", resp.status());
        }
        let json: serde_json::Value = resp.json().await?;
        Ok(parse_model_ids(&json, "models", "name"))
    }

    async fn test(&self, config: &ChannelConfig) -> Result<TestResult, anyhow::Error> {
        let model = config
            .models
            .first()
            .map(String::as_str)
            .unwrap_or_else(|| {
                self.default_models()
                    .first()
                    .copied()
                    .unwrap_or("gemini-2.0-flash")
            });
        let url = format!(
            "{}/v1beta/models/{}?key={}",
            config.base_url.trim_end_matches('/'),
            model,
            config.api_key
        );

        let client = build_client(config)?;
        let start = std::time::Instant::now();
        let resp = client.get(&url).send().await;
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
    ) -> Result<(u16, Value, Option<TokenUsage>), anyhow::Error> {
        let model = map_model(request, config);
        let url = self.request_url(config, &model, false);
        let gemini_body = self.to_gemini_request(request);

        let client = build_client(config)?;
        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&gemini_body)
            .send()
            .await?;

        let status = resp.status();
        let status_code = status.as_u16();
        let gemini_json: Value = resp.json().await?;

        // Surface upstream errors in OpenAI error shape instead of 502-ing.
        if !status.is_success() {
            let msg = gemini_json
                .pointer("/error/message")
                .and_then(|m| m.as_str())
                .unwrap_or("Gemini 上游返回错误");
            let err_body = json!({ "error": { "message": msg, "type": "upstream_error" } });
            return Ok((status_code, err_body, None));
        }

        let openai_response = self.to_openai_response(&model, &gemini_json);
        let usage = extract_usage(&openai_response);
        Ok((status_code, openai_response, usage))
    }

    async fn forward_stream(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<reqwest::Response, anyhow::Error> {
        let model = map_model(request, config);
        let url = self.request_url(config, &model, true);
        let gemini_body = self.to_gemini_request(request);

        let client = build_client(config)?;
        let resp = client
            .post(&url)
            .header("Content-Type", "application/json")
            .json(&gemini_body)
            .send()
            .await?;
        // NOTE: the returned SSE is Gemini-native (`alt=sse`), NOT OpenAI SSE.
        // Per-chunk conversion to OpenAI SSE is handled by `GeminiSseConverter`
        // in the proxy layer (handler::build_stream_response).
        Ok(resp)
    }
}

// ---------------------------------------------------------------------------
// Streaming: Gemini SSE (alt=sse) -> OpenAI SSE
// ---------------------------------------------------------------------------

/// Stateful converter: turns Gemini `streamGenerateContent?alt=sse` chunks
/// into OpenAI `chat.completion.chunk` `data:` frames.
pub struct GeminiSseConverter {
    model: String,
    id: String,
    role_emitted: bool,
    finished: bool,
}

impl GeminiSseConverter {
    pub fn new(model: String) -> Self {
        Self {
            model,
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()),
            role_emitted: false,
            finished: false,
        }
    }

    /// Convert one Gemini SSE record. Returns zero or more OpenAI `data:`
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
        let mut frames: Vec<String> = Vec::new();

        if let Some(u) = json.get("usageMetadata") {
            acc.prompt_tokens = u
                .get("promptTokenCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(acc.prompt_tokens);
            acc.completion_tokens = u
                .get("candidatesTokenCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(acc.completion_tokens);
            acc.total_tokens = u
                .get("totalTokenCount")
                .and_then(|v| v.as_i64())
                .unwrap_or(acc.total_tokens);
        }

        let text = json
            .pointer("/candidates/0/content/parts")
            .and_then(|p| p.as_array())
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|p| p.get("text").and_then(|t| t.as_str()))
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();

        if !text.is_empty() {
            if !self.role_emitted {
                frames.push(self.role_frame());
            }
            frames.push(self.delta_frame(serde_json::json!({ "content": text })));
        }

        if let Some(fr) = json
            .pointer("/candidates/0/finishReason")
            .and_then(|v| v.as_str())
            .map(map_gemini_finish)
        {
            if !fr.is_empty() {
                frames.push(self.finish_frame(fr));
                self.finished = true;
            }
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

fn map_gemini_finish(reason: &str) -> &'static str {
    match reason {
        "STOP" => "stop",
        "MAX_TOKENS" => "length",
        "SAFETY" | "RECITATION" | "OTHER" => "content_filter",
        _ => "stop",
    }
}
