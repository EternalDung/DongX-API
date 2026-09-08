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
                "assistant" => {
                    // Assistant turns may carry text and/or tool calls; Anthropic
                    // wants a content-block array, OpenAI allows a bare string.
                    converted.push(json!({
                        "role": "assistant",
                        "content": assistant_content(msg),
                    }));
                }
                "tool" => {
                    // OpenAI tool result -> Anthropic `tool_result` block, which
                    // must be delivered as a *user* turn.
                    let tool_use_id = msg
                        .get("tool_call_id")
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    converted.push(json!({
                        "role": "user",
                        "content": [{
                            "type": "tool_result",
                            "tool_use_id": tool_use_id,
                            "content": content_to_string(content),
                        }],
                    }));
                }
                "user" => {
                    converted.push(json!({ "role": "user", "content": content }));
                }
                _ => {} // unknown/legacy `function` role: drop
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
        // `tools`: OpenAI `{type:"function",function:{name,description,parameters}}`
        // -> Anthropic `{name,description,input_schema}`.
        if let Some(tools) = request.body.get("tools").and_then(|v| v.as_array()) {
            let mapped: Vec<Value> = tools
                .iter()
                .filter_map(|t| {
                    let f = t.get("function")?;
                    Some(json!({
                        "name": f.get("name").cloned().unwrap_or(json!("")),
                        "description": f.get("description").cloned().unwrap_or(json!("")),
                        "input_schema": f.get("parameters").cloned()
                            .unwrap_or_else(|| json!({ "type": "object", "properties": {} })),
                    }))
                })
                .collect();
            if !mapped.is_empty() {
                result["tools"] = Value::Array(mapped);
            }
        }
        // `tool_choice`: "auto"/"required"/{function:{name}} -> Anthropic
        // {type:"auto"|"any"|"tool"}. "none" has no Anthropic equivalent (omit).
        if let Some(tc) = request.body.get("tool_choice") {
            let mapped = match tc {
                Value::String(s) => match s.as_str() {
                    "auto" => Some(json!({ "type": "auto" })),
                    "required" => Some(json!({ "type": "any" })),
                    _ => None,
                },
                Value::Object(_) => tc
                    .get("function")
                    .and_then(|f| f.get("name"))
                    .map(|n| json!({ "type": "tool", "name": n })),
                _ => None,
            };
            if let Some(m) = mapped {
                result["tool_choice"] = m;
            }
        }
        result
    }

    /// Anthropic response -> OpenAI chat completion response
    ///
    /// Anthropic returns a `content` block array; `text` blocks are joined into
    /// the message body and `tool_use` blocks become OpenAI `tool_calls`
    /// (with `finish_reason: "tool_calls"`).
    fn to_openai_response(&self, model: &str, body: &Value) -> Value {
        let mut text = String::new();
        let mut tool_calls: Vec<Value> = Vec::new();

        if let Some(blocks) = body.get("content").and_then(|c| c.as_array()) {
            for b in blocks {
                match b.get("type").and_then(|t| t.as_str()).unwrap_or("") {
                    "text" => {
                        if let Some(s) = b.get("text").and_then(|t| t.as_str()) {
                            text.push_str(s);
                        }
                    }
                    "tool_use" => {
                        let id = b.get("id").and_then(|v| v.as_str()).unwrap_or("");
                        let name = b.get("name").and_then(|v| v.as_str()).unwrap_or("");
                        let input = b.get("input").cloned().unwrap_or_else(|| json!({}));
                        tool_calls.push(json!({
                            "id": id,
                            "type": "function",
                            "function": { "name": name, "arguments": input.to_string() },
                        }));
                    }
                    _ => {}
                }
            }
        }

        let usage = body.get("usage").cloned().unwrap_or(json!({}));
        // A tool_use turn must report "tool_calls"; otherwise map the stop reason.
        let stop_reason = body
            .get("stop_reason")
            .and_then(|v| v.as_str())
            .unwrap_or("end_turn");
        let finish_reason = if !tool_calls.is_empty() {
            "tool_calls"
        } else {
            map_anthropic_stop(stop_reason)
        };
        // OpenAI sends `content: null` on pure tool-call turns.
        let content: Value = if text.is_empty() && !tool_calls.is_empty() {
            Value::Null
        } else {
            json!(text)
        };
        let mut message = json!({ "role": "assistant", "content": content });
        if !tool_calls.is_empty() {
            message["tool_calls"] = Value::Array(tool_calls);
        }

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
                "message": message,
                "finish_reason": finish_reason,
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

/// OpenAI assistant message -> Anthropic content-block array.
///
/// Keeps any leading text as a `text` block and turns each `tool_calls` entry
/// into a `tool_use` block (arguments are JSON-decoded into `input`).
/// Anthropic rejects an empty content array, so a bare empty text block is
/// emitted when there is nothing else.
fn assistant_content(msg: &Value) -> Value {
    let mut blocks: Vec<Value> = Vec::new();
    if let Some(Value::String(s)) = msg.get("content") {
        if !s.is_empty() {
            blocks.push(json!({ "type": "text", "text": s }));
        }
    }
    if let Some(calls) = msg.get("tool_calls").and_then(|v| v.as_array()) {
        for tc in calls {
            let id = tc.get("id").and_then(|v| v.as_str()).unwrap_or("");
            let (name, args) = match tc.get("function") {
                Some(f) => (
                    f.get("name").and_then(|v| v.as_str()).unwrap_or(""),
                    f.get("arguments").and_then(|v| v.as_str()).unwrap_or(""),
                ),
                None => ("", ""),
            };
            let input: Value = serde_json::from_str(args).unwrap_or_else(|_| json!({}));
            blocks.push(json!({ "type": "tool_use", "id": id, "name": name, "input": input }));
        }
    }
    if blocks.is_empty() {
        blocks.push(json!({ "type": "text", "text": "" }));
    }
    Value::Array(blocks)
}

/// Flatten an OpenAI `content` value (string or block array) into plain text.
fn content_to_string(content: Option<&Value>) -> String {
    match content {
        Some(Value::String(s)) => s.clone(),
        Some(Value::Array(arr)) => {
            let mut buf = String::new();
            for b in arr {
                if b.get("type").and_then(|t| t.as_str()) == Some("text") {
                    if let Some(s) = b.get("text").and_then(|v| v.as_str()) {
                        buf.push_str(s);
                    }
                }
            }
            buf
        }
        Some(other) => other.as_str().unwrap_or("").to_string(),
        None => String::new(),
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

    /// Anthropic has no OpenAI-compatible `/v1/embeddings` endpoint.
    async fn forward_embeddings(
        &self,
        _request: &ProxyRequest,
        _config: &ChannelConfig,
    ) -> Result<(u16, serde_json::Value, Option<String>), anyhow::Error> {
        Err(anyhow::anyhow!(
            "Embeddings API 不支持 Anthropic (Claude) 渠道"
        ))
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
    ) -> Result<(u16, Value, Option<TokenUsage>, Option<String>), anyhow::Error> {
        let client = build_client(config)?;
        let resp = client
            .post(self.request_url(config))
            .header("x-api-key", &config.api_key)
            .header("anthropic-version", "2023-06-01")
            .json(&self.to_anthropic_request(request, config))
            .send()
            .await?;

        let status = resp.status().as_u16();
        let provider_request_id = crate::adapter::extract_provider_request_id(resp.headers());
        let body: Value = resp.json().await?;

        // Non-streaming: convert back to OpenAI format; usage moves from
        // input/output_tokens into prompt/completion_tokens.
        let usage = extract_usage(&body);
        let converted = self.to_openai_response(&request.model, &body);
        Ok((status, converted, usage, provider_request_id))
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
/// Accumulates the JSON argument fragments of one in-flight Anthropic tool call.
/// (Its id/name are emitted immediately on `content_block_start`, so only the
/// arguments need buffering.)
#[derive(Debug, Clone)]
struct ToolCallState {
    args: String,
}

pub struct AnthropicSseConverter {
    model: String,
    id: String,
    role_emitted: bool,
    finished: bool,
    /// Tool calls seen so far, in emission order (OpenAI indexes into this).
    tool_calls: Vec<ToolCallState>,
    /// Index into `tool_calls` of the block currently receiving JSON deltas.
    active_tool: Option<usize>,
}

impl AnthropicSseConverter {
    pub fn new(model: String) -> Self {
        Self {
            model,
            id: format!("chatcmpl-{}", uuid::Uuid::new_v4().simple()),
            role_emitted: false,
            finished: false,
            tool_calls: Vec::new(),
            active_tool: None,
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
                    acc.prompt_tokens = u.get("input_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                }
            }
            "content_block_start" => {
                // A `tool_use` block opens a new OpenAI tool call: emit its
                // identity once, then stream `arguments` via input_json_delta.
                if let Some(cb) = json.get("content_block") {
                    if cb.get("type").and_then(|t| t.as_str()) == Some("tool_use") {
                        let id = cb
                            .get("id")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let name = cb
                            .get("name")
                            .and_then(|v| v.as_str())
                            .unwrap_or("")
                            .to_string();
                        let idx = self.tool_calls.len();
                        self.tool_calls.push(ToolCallState {
                            args: String::new(),
                        });
                        self.active_tool = Some(idx);
                        if !self.role_emitted {
                            frames.push(self.role_frame());
                        }
                        frames.push(self.delta_frame(json!({
                            "tool_calls": [{
                                "index": idx,
                                "id": id,
                                "type": "function",
                                "function": { "name": name, "arguments": "" }
                            }]
                        })));
                    }
                }
            }
            "content_block_stop" => {
                self.active_tool = None;
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
                if kind == "input_json_delta" {
                    // Tool arguments arrive as JSON fragments: accumulate them
                    // locally and forward each piece as an OpenAI delta.
                    let piece = delta
                        .and_then(|d| d.get("partial_json"))
                        .and_then(|v| v.as_str())
                        .unwrap_or("");
                    if !piece.is_empty() {
                        if let Some(idx) = self.active_tool {
                            if let Some(tc) = self.tool_calls.get_mut(idx) {
                                tc.args.push_str(piece);
                            }
                            if !self.role_emitted {
                                frames.push(self.role_frame());
                            }
                            frames.push(self.delta_frame(json!({
                                "tool_calls": [{
                                    "index": idx,
                                    "function": { "arguments": piece }
                                }]
                            })));
                        }
                    }
                } else {
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
                        _ => None,
                    };
                    if let Some((field, text)) = field_and_text {
                        if !self.role_emitted {
                            frames.push(self.role_frame());
                        }
                        frames.push(self.delta_frame(serde_json::json!({ field: text })));
                    }
                }
            }
            "message_delta" => {
                if let Some(u) = json.get("usage") {
                    acc.completion_tokens =
                        u.get("output_tokens").and_then(|v| v.as_i64()).unwrap_or(0);
                }
                let stop = json
                    .pointer("/delta/stop_reason")
                    .and_then(|s| s.as_str())
                    .unwrap_or("end_turn");
                frames.push(self.finish_frame(map_anthropic_stop(stop)));
                self.finished = true;
            }
            "message_stop" if !self.finished => {
                frames.push(self.finish_frame("stop"));
                self.finished = true;
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

#[cfg(test)]
mod tests {
    use super::*;

    fn config() -> ChannelConfig {
        ChannelConfig {
            base_url: "https://api.anthropic.com".to_string(),
            api_key: "k".to_string(),
            models: vec!["claude-sonnet-4-20250514".to_string()],
            model_mapping: serde_json::json!({}),
            extra: serde_json::json!({}),
            timeout_secs: 30,
            stream: false,
        }
    }

    fn req(body: Value) -> ProxyRequest {
        ProxyRequest {
            model: "claude-sonnet-4-20250514".to_string(),
            body,
            stream: false,
        }
    }

    #[test]
    fn assistant_with_tool_calls_becomes_tool_use_blocks() {
        let msg = json!({
            "role": "assistant",
            "content": "calling",
            "tool_calls": [{
                "id": "call_1",
                "type": "function",
                "function": { "name": "get_weather", "arguments": "{\"city\":\"SH\"}" }
            }]
        });
        let blocks = assistant_content(&msg);
        assert_eq!(blocks[0]["type"], "text");
        assert_eq!(blocks[1]["type"], "tool_use");
        assert_eq!(blocks[1]["id"], "call_1");
        assert_eq!(blocks[1]["name"], "get_weather");
        // arguments must be JSON-decoded into `input`, not kept as a string
        assert_eq!(blocks[1]["input"]["city"], "SH");
    }

    #[test]
    fn assistant_without_content_still_gets_a_block() {
        let msg = json!({ "role": "assistant", "content": null });
        let blocks = assistant_content(&msg);
        assert_eq!(blocks.as_array().unwrap().len(), 1);
        assert_eq!(blocks[0]["type"], "text");
    }

    #[test]
    fn tool_result_maps_to_anthropic_user_turn() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [
                { "role": "user", "content": "weather?" },
                {
                    "role": "assistant",
                    "content": null,
                    "tool_calls": [{ "id": "call_1", "type": "function",
                        "function": { "name": "get_weather", "arguments": "{}" } }]
                },
                { "role": "tool", "tool_call_id": "call_1", "content": "sunny" }
            ]
        });
        let out = ClaudeAdaptor.to_anthropic_request(&req(body), &config());
        let msgs = out["messages"].as_array().unwrap();
        assert_eq!(msgs.len(), 3, "no message may be dropped");
        // assistant turn carries the tool_use block
        assert_eq!(msgs[1]["role"], "assistant");
        assert_eq!(msgs[1]["content"][0]["type"], "tool_use");
        // tool result must be delivered as a user turn
        assert_eq!(msgs[2]["role"], "user");
        assert_eq!(msgs[2]["content"][0]["type"], "tool_result");
        assert_eq!(msgs[2]["content"][0]["tool_use_id"], "call_1");
        assert_eq!(msgs[2]["content"][0]["content"], "sunny");
    }

    #[test]
    fn tools_and_tool_choice_are_translated() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{ "role": "user", "content": "hi" }],
            "tools": [{
                "type": "function",
                "function": {
                    "name": "get_weather",
                    "description": "Get weather",
                    "parameters": { "type": "object", "properties": { "city": { "type": "string" } } }
                }
            }],
            "tool_choice": "auto"
        });
        let out = ClaudeAdaptor.to_anthropic_request(&req(body), &config());
        assert_eq!(out["tools"][0]["name"], "get_weather");
        assert_eq!(out["tools"][0]["description"], "Get weather");
        assert_eq!(
            out["tools"][0]["input_schema"]["properties"]["city"]["type"],
            "string"
        );
        assert_eq!(out["tool_choice"]["type"], "auto");

        let forced = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{ "role": "user", "content": "hi" }],
            "tool_choice": { "type": "function", "function": { "name": "get_weather" } }
        });
        let out2 = ClaudeAdaptor.to_anthropic_request(&req(forced), &config());
        assert_eq!(out2["tool_choice"]["type"], "tool");
        assert_eq!(out2["tool_choice"]["name"], "get_weather");
    }

    #[test]
    fn tool_choice_none_is_omitted() {
        let body = json!({
            "model": "claude-sonnet-4-20250514",
            "messages": [{ "role": "user", "content": "hi" }],
            "tool_choice": "none"
        });
        let out = ClaudeAdaptor.to_anthropic_request(&req(body), &config());
        assert!(
            out.get("tool_choice").is_none(),
            "Anthropic has no `none` equivalent"
        );
    }

    #[test]
    fn anthropic_tool_use_becomes_openai_tool_calls() {
        let body = json!({
            "id": "msg_1",
            "content": [
                { "type": "text", "text": "let me check" },
                { "type": "tool_use", "id": "toolu_1", "name": "get_weather",
                  "input": { "city": "SH" } }
            ],
            "stop_reason": "tool_use",
            "usage": { "input_tokens": 10, "output_tokens": 5 }
        });
        let out = ClaudeAdaptor.to_openai_response("claude-sonnet-4-20250514", &body);
        let msg = &out["choices"][0]["message"];
        assert_eq!(msg["content"], "let me check");
        assert_eq!(msg["tool_calls"][0]["id"], "toolu_1");
        assert_eq!(msg["tool_calls"][0]["type"], "function");
        assert_eq!(msg["tool_calls"][0]["function"]["name"], "get_weather");
        assert_eq!(
            msg["tool_calls"][0]["function"]["arguments"],
            "{\"city\":\"SH\"}"
        );
        assert_eq!(out["choices"][0]["finish_reason"], "tool_calls");
    }

    #[test]
    fn pure_tool_call_response_has_null_content() {
        let body = json!({
            "id": "msg_3",
            "content": [
                { "type": "tool_use", "id": "toolu_9", "name": "search", "input": {} }
            ],
            "stop_reason": "tool_use"
        });
        let out = ClaudeAdaptor.to_openai_response("claude-sonnet-4-20250514", &body);
        let msg = &out["choices"][0]["message"];
        assert!(
            msg["content"].is_null(),
            "OpenAI sends content:null on tool turns"
        );
        assert_eq!(msg["tool_calls"][0]["function"]["name"], "search");
    }

    #[test]
    fn plain_text_response_still_reports_stop() {
        let body = json!({
            "id": "msg_2",
            "content": [{ "type": "text", "text": "hello" }],
            "stop_reason": "end_turn",
            "usage": { "input_tokens": 1, "output_tokens": 2 }
        });
        let out = ClaudeAdaptor.to_openai_response("claude-sonnet-4-20250514", &body);
        assert_eq!(out["choices"][0]["message"]["content"], "hello");
        assert_eq!(out["choices"][0]["finish_reason"], "stop");
        assert!(out["choices"][0]["message"].get("tool_calls").is_none());
    }

    #[test]
    fn content_to_string_flattens_blocks() {
        let blocks = json!([
            { "type": "text", "text": "a" },
            { "type": "tool_use", "id": "x" },
            { "type": "text", "text": "b" }
        ]);
        assert_eq!(content_to_string(Some(&blocks)), "ab");
        assert_eq!(content_to_string(Some(&json!("plain"))), "plain");
        assert_eq!(content_to_string(None), "");
    }

    fn sse(data: &str) -> SseRecord {
        SseRecord {
            event: None,
            data: data.to_string(),
        }
    }

    #[test]
    fn stream_tool_use_emits_tool_call_deltas() {
        let mut conv = AnthropicSseConverter::new("claude-sonnet-4-20250514".to_string());
        let mut acc = StreamUsage::default();

        // 1) A tool_use block opens -> role frame + tool identity frame.
        let frames = conv.convert(
            &sse(r#"{"type":"content_block_start","index":0,"content_block":{"type":"tool_use","id":"toolu_1","name":"get_weather","input":{}}}"#),
            &mut acc,
        );
        assert_eq!(frames.len(), 2, "role frame + identity frame");
        assert!(frames[1].contains("\"id\":\"toolu_1\""), "{}", frames[1]);
        assert!(frames[1].contains("get_weather"), "{}", frames[1]);

        // 2) Arguments arrive as JSON fragments and must be buffered in order.
        conv.convert(
            &sse(r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"{\"city\":"}}"#),
            &mut acc,
        );
        conv.convert(
            &sse(r#"{"type":"content_block_delta","index":0,"delta":{"type":"input_json_delta","partial_json":"\"SH\"}"}}"#),
            &mut acc,
        );
        assert_eq!(conv.tool_calls[0].args, r#"{"city":"SH"}"#);

        // 3) Block closes, then the message ends with stop_reason=tool_use.
        conv.convert(&sse(r#"{"type":"content_block_stop","index":0}"#), &mut acc);
        let frames = conv.convert(
            &sse(r#"{"type":"message_delta","delta":{"stop_reason":"tool_use"},"usage":{"output_tokens":7}}"#),
            &mut acc,
        );
        assert_eq!(frames.len(), 1);
        assert!(
            frames[0].contains("\"finish_reason\":\"tool_calls\""),
            "{}",
            frames[0]
        );
        assert_eq!(acc.completion_tokens, 7);
    }

    #[test]
    fn stream_text_delta_still_works_alongside_tools() {
        let mut conv = AnthropicSseConverter::new("m".to_string());
        let mut acc = StreamUsage::default();
        let frames = conv.convert(
            &sse(r#"{"type":"content_block_delta","index":0,"delta":{"type":"text_delta","text":"hi"}}"#),
            &mut acc,
        );
        assert_eq!(frames.len(), 2, "role frame + content frame");
        assert!(frames[1].contains("\"content\":\"hi\""), "{}", frames[1]);
        assert!(
            conv.tool_calls.is_empty(),
            "text must not create tool state"
        );
    }
}
