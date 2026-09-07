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
    /// Whether this request is a streaming (SSE) request. Used to pick the
    /// right timeout semantics in `build_client`.
    #[serde(default)]
    pub stream: bool,
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
    #[allow(dead_code)]
    fn channel_type(&self) -> &'static str;
    fn default_models(&self) -> Vec<&'static str>;
    #[allow(dead_code)]
    fn default_base_url(&self) -> &str;

    /// List models from the upstream provider and return their ids.
    ///
    /// The default implementation targets the OpenAI-compatible `/v1/models`
    /// shape (Bearer auth, `data[].id`). Providers with different auth or
    /// paths override this (e.g. Gemini uses a `?key=` query on the native
    /// `/v1beta/models` path; Claude uses the `x-api-key` header).
    async fn list_models(&self, config: &ChannelConfig) -> Result<Vec<String>, anyhow::Error> {
        fetch_openai_models(config).await
    }

    /// Test channel connectivity with a minimal request; measure latency.
    async fn test(&self, config: &ChannelConfig) -> Result<TestResult, anyhow::Error>;

    /// Forward a non-streaming request. Returns (status, body, usage,
    /// provider_request_id). The 4th element is the upstream's own request id
    /// echoed back via response headers (e.g. `x-request-id` / `request-id`) —
    /// distinct from DongX's server-generated gateway `trace_id`.
    async fn forward(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<
        (u16, serde_json::Value, Option<TokenUsage>, Option<String>),
        anyhow::Error,
    >;

    /// Forward a streaming (SSE) request. Returns the raw upstream response;
    /// the caller streams `bytes_stream()` through to the client.
    async fn forward_stream(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<reqwest::Response, anyhow::Error>;

    /// Forward an embeddings request to the OpenAI-compatible `/v1/embeddings`
    /// endpoint. Default implementation POSTs the raw body with Bearer auth;
    /// providers without an embeddings API (Claude / Gemini) override this to
    /// return a clear error instead of a confusing 404. Returns
    /// (status, body, provider_request_id).
    async fn forward_embeddings(
        &self,
        request: &ProxyRequest,
        config: &ChannelConfig,
    ) -> Result<(u16, serde_json::Value, Option<String>), anyhow::Error> {
        let _ = self;
        let client = build_client(config)?;
        let base = config.base_url.trim_end_matches('/');
        let url = format!("{}/embeddings", base);
        let resp = client
            .post(url)
            .bearer_auth(&config.api_key)
            .json(&request.body)
            .send()
            .await?;
        let status = resp.status().as_u16();
        let provider_request_id = extract_provider_request_id(resp.headers());
        // Read the raw body once, then parse. This keeps the real upstream
        // status + a body snippet in the error instead of an opaque
        // "error decoding response body" when the payload isn't JSON
        // (e.g. a proxy/HTML error page, or a body the client couldn't
        // decode because the matching compression feature was disabled).
        let text = resp.text().await?;
        let body: serde_json::Value = serde_json::from_str(&text).map_err(|_| {
            let snippet: String = text.chars().take(300).collect();
            anyhow::anyhow!("嵌入上游返回非 JSON（HTTP {}）：{}", status, snippet)
        })?;
        Ok((status, body, provider_request_id))
    }
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
///
/// - Non-streaming: `timeout()` bounds the *whole* request — the full response
///   must arrive within `timeout_secs`.
/// - Streaming (SSE): only `connect_timeout()` bounds the *connection-establishment*
///   phase (TCP/TLS handshake). The stream body itself may run as long as the
///   upstream keeps sending; long dialogues must not be cut off mid-stream.
pub(crate) fn build_client(config: &ChannelConfig) -> Result<reqwest::Client, anyhow::Error> {
    let secs = Duration::from_secs(config.timeout_secs.max(1));
    let builder = reqwest::Client::builder();
    let builder = if config.stream {
        builder.connect_timeout(secs)
    } else {
        builder.timeout(secs)
    };
    Ok(builder.build()?)
}

/// Join a base URL and a path, tolerating stray slashes on either side.
pub(crate) fn join_url(base: &str, path: &str) -> String {
    let base = base.trim_end_matches('/');
    let path = path.trim_start_matches('/');
    format!("{}/{}", base, path)
}

/// reqwest requires a scheme; users often type only a host (e.g.
/// `127.0.0.1:11434`), so default a missing scheme to `http://`.
pub(crate) fn ensure_scheme(url: &str) -> String {
    if url.starts_with("http://") || url.starts_with("https://") {
        url.to_string()
    } else {
        format!("http://{url}")
    }
}

/// Fetch the model list from an OpenAI-compatible `/models` endpoint.
///
/// DongX's channel base_url already carries the version segment (the chat
/// endpoint is `{base}/chat/completions`), so the models path is `/models`,
/// not `/v1/models`. Returns the upstream `data[].id` values. Used as the
/// default `Adaptor::list_models` implementation for OpenAI-compatible
/// providers (openai / deepseek / zhipu / qwen / ollama / moonshot / ...).
pub(crate) async fn fetch_openai_models(
    config: &ChannelConfig,
) -> Result<Vec<String>, anyhow::Error> {
    let client = build_client(config)?;
    let url = ensure_scheme(&join_url(&config.base_url, "/models"));
    let resp = client.get(&url).bearer_auth(&config.api_key).send().await?;
    if !resp.status().is_success() {
        anyhow::bail!("list models failed: upstream status {}", resp.status());
    }
    let json: serde_json::Value = resp.json().await?;
    Ok(parse_model_ids(&json, "data", "id"))
}

/// Extract model ids from a provider's model-list payload.
///
/// `array_key` is the JSON array field (`data` for OpenAI/Claude, `models`
/// for Gemini); `id_key` is the field holding the id (`id`, or `name` for
/// Gemini where it arrives as `models/<id>` and is stripped to the short id).
pub(crate) fn parse_model_ids(
    json: &serde_json::Value,
    array_key: &str,
    id_key: &str,
) -> Vec<String> {
    json.get(array_key)
        .and_then(|d| d.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(|m| {
                    let raw = m.get(id_key).and_then(|v| v.as_str())?;
                    let short = raw.strip_prefix("models/").unwrap_or(raw);
                    Some(short.to_string())
                })
                .collect()
        })
        .unwrap_or_default()
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

/// Extract the upstream's own request id from response headers, if present.
///
/// OpenAI-compatible providers echo `x-request-id`; Anthropic uses
/// `request-id`. This is the id the *model provider* assigns to the call —
/// distinct from DongX's server-generated gateway `trace_id`. `None` when the
/// upstream didn't return one (or it wasn't decodable).
pub(crate) fn extract_provider_request_id(
    headers: &reqwest::header::HeaderMap,
) -> Option<String> {
    headers
        .get("x-request-id")
        .or_else(|| headers.get("request-id"))
        .and_then(|v| v.to_str().ok())
        .map(|s| s.to_string())
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

/// OpenAI's `developer` role (a more stable variant of `system`, emitted by
/// clients such as Codex and o-series models) is rejected by most
/// OpenAI-compatible upstreams (DeepSeek, local vLLM/llama.cpp servers, older
/// OpenAI models). Rewrite any `developer` message role to `system` before
/// forwarding so the request is accepted. The original role is preserved in the
/// request log — this normalization happens only at the upstream boundary.
pub(crate) fn normalize_developer_role(body: &mut serde_json::Value) {
    if let Some(msgs) = body.get_mut("messages").and_then(|m| m.as_array_mut()) {
        for m in msgs.iter_mut() {
            if m.get("role").and_then(|v| v.as_str()) == Some("developer") {
                m["role"] = serde_json::json!("system");
            }
        }
    }
}

// ---------------------------------------------------------------------------
// Streaming SSE helpers (shared by the proxy layer)
// ---------------------------------------------------------------------------

/// One SSE record: an `event:` type (optional) plus its `data:` payload.
/// Records are delimited by a blank line on the wire.
#[derive(Debug, Clone, Default)]
pub struct SseRecord {
    #[allow(dead_code)]
    pub event: Option<String>,
    pub data: String,
}

/// Token usage accumulated while scanning a stream of SSE frames.
#[derive(Debug, Clone, Default)]
pub struct StreamUsage {
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub total_tokens: i64,
}

/// Which transform a streaming attempt needs. `None` = native OpenAI SSE
/// passthrough (OpenAI / DeepSeek / OpenAI-compatible providers); the others
/// convert the upstream's native SSE into OpenAI `chat.completion.chunk` SSE.
#[derive(Debug, Clone, Copy)]
pub enum StreamConverter {
    None,
    Claude,
    Gemini,
}

/// Split a raw SSE buffer into complete records. Returns the complete records
/// and the trailing partial record (kept by the caller for the next chunk).
pub fn split_sse_records(buf: &str) -> (Vec<SseRecord>, String) {
    let normalized = buf.replace("\r\n", "\n");
    let parts: Vec<&str> = normalized.split("\n\n").collect();
    let n = parts.len();
    let mut records = Vec::new();
    // Every part except the last is a complete record; the last part is the
    // remainder — it may be empty, or an incomplete record the caller buffers.
    for part in &parts[..n.saturating_sub(1)] {
        if part.trim().is_empty() {
            continue;
        }
        records.push(parse_sse_record(part));
    }
    let remainder = if n == 0 {
        String::new()
    } else {
        parts[n - 1].to_string()
    };
    (records, remainder)
}

fn parse_sse_record(block: &str) -> SseRecord {
    let mut event = None;
    let mut data_lines: Vec<String> = Vec::new();
    for line in block.lines() {
        if let Some(rest) = line.strip_prefix("event:") {
            event = Some(rest.trim().to_string());
        } else if let Some(rest) = line.strip_prefix("data:") {
            data_lines.push(rest.strip_prefix(' ').unwrap_or(rest).to_string());
        }
        // other fields (id:, retry:) and comments are ignored
    }
    SseRecord {
        event,
        data: data_lines.join("\n"),
    }
}

/// Scan OpenAI-shaped SSE text for the last `usage` object (streaming usage
/// arrives on a dedicated frame or the final choice frame). Last wins.
pub fn scan_openai_usage(text: &str, acc: &mut StreamUsage) {
    for line in text.lines() {
        let trimmed = line.trim();
        if !trimmed.starts_with("data:") {
            continue;
        }
        let payload = trimmed.trim_start_matches("data:").trim();
        if payload.is_empty() || payload == "[DONE]" {
            continue;
        }
        if let Ok(json) = serde_json::from_str::<serde_json::Value>(payload) {
            if let Some(u) = json.get("usage") {
                acc.prompt_tokens = u
                    .get("prompt_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(acc.prompt_tokens);
                acc.completion_tokens = u
                    .get("completion_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(acc.completion_tokens);
                acc.total_tokens = u
                    .get("total_tokens")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(acc.total_tokens);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    #[test]
    fn normalize_developer_role_maps_to_system() {
        let mut body = json!({
            "model": "gpt-4o",
            "messages": [
                { "role": "system", "content": "sys" },
                { "role": "developer", "content": "dev instruction" },
                { "role": "user", "content": "hi" },
                { "role": "developer", "content": "more dev" },
            ]
        });
        normalize_developer_role(&mut body);
        let roles: Vec<&str> = body["messages"]
            .as_array()
            .unwrap()
            .iter()
            .map(|m| m["role"].as_str().unwrap())
            .collect();
        assert_eq!(roles, vec!["system", "system", "user", "system"]);
    }

    #[test]
    fn normalize_developer_role_no_messages_is_noop() {
        let mut body = json!({ "model": "x" });
        normalize_developer_role(&mut body);
        assert!(body.get("messages").is_none());
    }
}
