use std::sync::Arc;

use axum::body::{to_bytes, Body};
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use bytes::Bytes;
use futures_util::StreamExt;
use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::adapter::{
    self, Adaptor, ChannelConfig, ProxyRequest, StreamConverter, StreamUsage, scan_openai_usage,
    split_sse_records,
};
use crate::core::{dispatcher, failover};
use crate::db::repository::{channel_health, channels, gateway_keys, request_logs, settings};
use crate::server::auth;
use crate::AppState;

/// Health check endpoint.
pub async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "DongX Gateway" }))
}

/// POST /v1/chat/completions — OpenAI-compatible chat completions.
///
/// Pipeline:
/// 1. Auth — verify `sk-dongapi-*` gateway key against the DB.
/// 2. Parse — read `model` + `stream` from the OpenAI-shaped body.
/// 3. Dispatch — pick a channel (priority + weight) and one upstream key.
/// 4. Adapt — `get_adaptor(type).forward()` converts OpenAI -> upstream
///    protocol and proxies the request (non-streaming), or `forward_stream()`
///    + `build_stream_response()` streams SSE back: OpenAI-compatible upstreams
///    are passed through byte-for-byte, while Claude/Gemini native SSE is
///    converted chunk-by-chunk into OpenAI `chat.completion.chunk` frames.
/// 5. Quota — debit the gateway key's used tokens.
/// 6. Log — async insert into `request_logs`.
/// 7. Return — pass the upstream body + status back to the client.
pub async fn chat_completions(
    State(app): State<AppHandle>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // Thin wrapper: the public `/v1/chat/completions` entry always logs mode="chat".
    run_chat_pipeline(app, headers, body, "chat").await
}

/// Core chat pipeline — the de-facto "driver" in DongX. It owns the whole
/// request lifecycle (auth -> dispatch -> adapt -> log) and is shared by both
/// `/v1/chat/completions` (mode="chat") and `/v1/responses` (mode="responses"),
/// so the request log records each call's true entry surface (the driver
/// writes `mode` into `RequestLog`).
async fn run_chat_pipeline(
    app: AppHandle,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
    mode: &'static str,
) -> Response {
    let state: Arc<AppState> = app.state::<Arc<AppState>>().inner().clone();
    let start = std::time::Instant::now();

    // Read the "log raw request/response body" toggle (default off).
    let log_raw_body = match settings::get(&state.db, "log_raw_body").await {
        Ok(Some(s)) => serde_json::from_str::<bool>(&s).unwrap_or(false),
        _ => false,
    };
    // Capture the raw request body only when the toggle is on (privacy/perf).
    let raw_request = if log_raw_body {
        Some(String::from_utf8_lossy(&body).to_string())
    } else {
        None
    };

    // 1. Auth
    let key = match auth::extract_gateway_key(&headers) {
        Some(k) => k,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "missing_key",
                "缺少 Authorization 或非 sk-dongapi- 密钥",
            )
        }
    };
    let gw_key = match auth::validate_gateway_key(&state.db, &key).await {
        Some(k) => k,
        None => {
            return error_response(
                StatusCode::UNAUTHORIZED,
                "invalid_key",
                "网关密钥不存在/已禁用/已过期/配额耗尽",
            )
        }
    };

    // 2. Parse
    let body_json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "invalid_json", &e.to_string()),
    };
    let model = body_json
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if model.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "missing_model",
            "请求体缺少 model 字段",
        );
    }
    let is_stream = body_json
        .get("stream")
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    // 3. Load retry settings (defaults: enabled, 3 retries).
    //    retry_times = 「额外重试次数」; max_attempts = retry_times + 1（含首次）。
    let retry_enabled: bool = match settings::get(&state.db, "retry_enabled").await {
        Ok(Some(s)) => serde_json::from_str::<bool>(&s).unwrap_or(true),
        _ => true,
    };
    let retry_times: usize = match settings::get(&state.db, "retry_times").await {
        Ok(Some(s)) => serde_json::from_str::<i32>(&s).unwrap_or(3).max(0) as usize,
        _ => 3,
    };
    let max_attempts = if retry_enabled { retry_times + 1 } else { 1 };

    // 4. Dispatch + forward, wrapped in an automatic channel-failover state
    //    machine (core/failover.rs, modelled after waliapi's AttemptFlow):
    //    pick a candidate channel -> forward -> on a *retryable* failure, record
    //    it against the circuit breaker and try the next candidate channel.
    let ctx = dispatcher::DispatchContext {
        model: model.clone(),
        api_key_id: gw_key.id.clone(),
        is_stream,
        request_body: body_json.clone(),
    };
    let mut fo = failover::Failover::new(state.db.clone(), ctx, max_attempts);

    /// 一次成功尝试的产出（流 / 非流分两种形态）。
    enum Success {
        NonStream {
            selected: dispatcher::SelectedChannel,
            status: u16,
            resp_body: serde_json::Value,
            usage: Option<adapter::TokenUsage>,
            upstream_model: String,
        },
        Stream {
            selected: dispatcher::SelectedChannel,
            upstream_model: String,
            resp: reqwest::Response,
        },
    }
    let mut success: Option<Success> = None;

    loop {
        let step = match fo.next().await {
            Ok(s) => s,
            Err(e) => {
                return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal", &e.to_string());
            }
        };
        let selected = match step {
            failover::Step::Try(c) => c,
            failover::Step::NoChannel(msg) => {
                return error_response(StatusCode::SERVICE_UNAVAILABLE, "no_channel", &msg);
            }
            failover::Step::Exhausted => break,
        };

        // Build the per-attempt adapter config + request + mapped upstream name.
        let config = ChannelConfig {
            base_url: selected.base_url.clone(),
            api_key: selected.upstream_api_key.clone(),
            models: selected.models.clone(),
            model_mapping: selected.model_mapping.clone(),
            extra: selected.extra.clone(),
            timeout_secs: selected.timeout_secs,
            stream: is_stream,
        };
        let proxy_req = ProxyRequest {
            model: model.clone(),
            body: body_json.clone(),
            stream: is_stream,
        };
        let upstream_model = adapter::map_model(&proxy_req, &config);
        let adaptor = adapter::get_adaptor(&selected.channel_type);
        let is_retry = fo.is_retry();

        if is_stream {
            // 流式：只在「连接成功且拿到 2xx」之后才把连接交给 SSE 构建；
            // 连接/超时/非 2xx 均发生在向客户端写入任何字节之前，可安全重试。
            match acquire_stream_response(&*adaptor, &proxy_req, &config).await {
                Ok(resp) => {
                    // 2xx 流已建立 → 成功，跳出循环交给 serve_stream。
                    record_upstream_outcome(&state, &selected.id, true, false, "").await;
                    success = Some(Success::Stream {
                        selected,
                        upstream_model,
                        resp,
                    });
                    break;
                }
                Err(outcome) => {
                    let duration_ms = start.elapsed().as_millis() as i64;
                    spawn_log(
                        state.clone(),
                        Some(gw_key.name.clone()),
                        Some(selected.name.clone()),
                        model.clone(),
                        Some(upstream_model.clone()),
                        outcome.status as i32,
                        0,
                        0,
                        0,
                        duration_ms,
                        Some(outcome.message.clone()),
                        is_stream,
                        is_retry,
                        raw_request.clone(),
                        None,
                        mode,
                    );
                    // 按上游状态分类：5xx/429/408/409 计入熔断，其余 4xx 不计。
                    record_upstream_outcome(
                        &state,
                        &selected.id,
                        false,
                        outcome.retryable,
                        &outcome.message,
                    )
                    .await;
                    fo.observe(outcome);
                    if fo.should_retry() {
                        continue;
                    }
                    break;
                }
            }
        } else {
            match adaptor.forward(&proxy_req, &config).await {
                Ok((status, resp_body, usage)) => {
                    record_upstream_outcome(&state, &selected.id, true, false, "").await;
                    success = Some(Success::NonStream {
                        selected,
                        status,
                        resp_body,
                        usage,
                        upstream_model,
                    });
                    break;
                }
                Err(e) => {
                    let msg = e.to_string();
                    let duration_ms = start.elapsed().as_millis() as i64;
                    spawn_log(
                        state.clone(),
                        Some(gw_key.name.clone()),
                        Some(selected.name.clone()),
                        model.clone(),
                        Some(upstream_model.clone()),
                        502,
                        0,
                        0,
                        0,
                        duration_ms,
                        Some(msg.clone()),
                        is_stream,
                        is_retry,
                        raw_request.clone(),
                        None,
                        mode,
                    );
                    // 上游连接/超时失败 → 可重试，计入熔断。
                    record_upstream_outcome(&state, &selected.id, false, true, &msg).await;
                    fo.observe(failover::Outcome::connection(msg));
                    if fo.should_retry() {
                        continue;
                    }
                    break;
                }
            }
        }
    }

    // 所有候选耗尽（且最后一次失败）→ 回退最后一个上游错误给客户端。
    let success = match success {
        Some(s) => s,
        None => {
            let outcome = fo
                .last_outcome()
                .cloned()
                .unwrap_or_else(|| failover::Outcome::no_channel("没有可用的候选渠道".into()));
            return error_response(
                StatusCode::from_u16(outcome.status).unwrap_or(StatusCode::BAD_GATEWAY),
                &outcome.code,
                &outcome.message,
            );
        }
    };

    // 5. Success path — stream and non-stream diverge here.
    match success {
        Success::Stream {
            selected,
            upstream_model,
            resp,
        } => {
            // 流式：交由 serve_stream 建立 SSE（循环内已确认 2xx）。
            return serve_stream(
                state.clone(),
                gw_key.name.clone(),
                selected.name.clone(),
                selected.id.clone(),
                model.clone(),
                upstream_model.clone(),
                is_stream,
                start,
                raw_request.clone(),
                log_raw_body,
                &selected.channel_type,
                resp,
                fo.is_retry(),
                mode,
            )
            .await;
        }
        Success::NonStream {
            selected,
            status,
            resp_body,
            usage,
            upstream_model,
        } => {
            let (pt, ct, tt) = usage
                .as_ref()
                .map(|u| {
                    (
                        u.prompt_tokens as i64,
                        u.completion_tokens as i64,
                        u.total_tokens as i64,
                    )
                })
                .unwrap_or((0, 0, 0));

            // Debit gateway key quota.
            if tt > 0 {
                let _ = gateway_keys::add_quota_used(&state.db, &gw_key.id, tt).await;
            }

            let duration_ms = start.elapsed().as_millis() as i64;
            let raw_response = if log_raw_body {
                serde_json::to_string(&resp_body).ok()
            } else {
                None
            };
            spawn_log(
                state.clone(),
                Some(gw_key.name.clone()),
                Some(selected.name.clone()),
                model.clone(),
                Some(upstream_model.clone()),
                status as i32,
                pt,
                ct,
                tt,
                duration_ms,
                None,
                is_stream,
                fo.is_retry(),
                raw_request.clone(),
                raw_response,
                mode,
            );

            // Return upstream body + status.
            let st = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
            (st, Json(resp_body)).into_response()
        }
    }
}

/// GET /v1/models — list distinct models across all enabled channels.
pub async fn list_models(State(app): State<AppHandle>) -> Response {
    let state = app.state::<Arc<AppState>>();

    let rows = match channels::list_enabled(&state.db).await {
        Ok(r) => r,
        Err(_) => return Json(json!({ "object": "list", "data": [] })).into_response(),
    };

    let mut seen = std::collections::HashSet::new();
    let mut data: Vec<serde_json::Value> = Vec::new();
    for row in rows {
        let models: Vec<String> = serde_json::from_str(&row.models).unwrap_or_default();
        for m in models {
            if seen.insert(m.clone()) {
                data.push(json!({ "id": m, "object": "model", "owned_by": row.name }));
            }
        }
    }

    Json(json!({ "object": "list", "data": data })).into_response()
}

/// POST /v1/completions — not yet implemented.
pub async fn completions(
    State(app): State<AppHandle>,
    body: axum::body::Bytes,
) -> Response {
    let _ = (app, body);
    error_response(
        StatusCode::NOT_IMPLEMENTED,
        "not_implemented",
        "Completions endpoint not yet implemented",
    )
}

/// POST /v1/embeddings — not yet implemented.
pub async fn embeddings(
    State(app): State<AppHandle>,
    body: axum::body::Bytes,
) -> Response {
    let _ = (app, body);
    error_response(
        StatusCode::NOT_IMPLEMENTED,
        "not_implemented",
        "Embeddings endpoint not yet implemented",
    )
}

/// POST /v1/responses — OpenAI Responses API bridge.
///
/// Translates the Responses request (`input` / `instructions`) into an OpenAI
/// Chat request, delegates to the shared chat pipeline (`run_chat_pipeline`,
/// which performs auth + dispatch + adapt + log) and translates the Chat
/// response back into the Responses shape. Both streaming and non-streaming are
/// supported: the outbound Chat SSE stream is converted frame-by-frame into
/// Responses SSE events. DongX's `/v1/responses` reuses the
/// same chat executor and only the entry (request) and exit (response)
/// translation differs.
pub async fn responses(
    State(app): State<AppHandle>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // 1. Parse Responses request.
    let req: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "invalid_json", &e.to_string()),
    };
    let model = req
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_string();
    if model.is_empty() {
        return error_response(
            StatusCode::BAD_REQUEST,
            "missing_model",
            "请求体缺少 model 字段",
        );
    }
    let is_stream = req.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);

    // 2. Translate Responses `input` -> Chat `messages`.
    let messages = responses_input_to_messages(&req);

    // 3. Build the Chat request body, forwarding common sampler params.
    let mut chat_body = json!({
        "model": model,
        "messages": messages,
        "stream": is_stream,
    });
    for f in [
        "temperature",
        "top_p",
        "max_tokens",
        "max_completion_tokens",
        "stop",
        "seed",
        "n",
    ] {
        if let Some(v) = req.get(f) {
            chat_body[f] = v.clone();
        }
    }
    let chat_bytes = match serde_json::to_vec(&chat_body) {
        Ok(b) => b,
        Err(e) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal", &e.to_string())
        }
    };

    // 4. Delegate to the shared chat pipeline (auth/dispatch/adapt/log all
    //    reused). `mode = "responses"` so the request log records this entry as
    //    a Responses call — the driver writes `mode`
    //    into `RequestLog`.
    let chat_resp =
        run_chat_pipeline(app, headers, axum::body::Bytes::from(chat_bytes), "responses").await;

    // 5. Translate the Chat response back into Responses shape.
    if !is_stream {
        let (parts, resp_body) = chat_resp.into_parts();
        let bytes = match to_bytes(resp_body, usize::MAX).await {
            Ok(b) => b,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "读取上游响应失败",
                )
            }
        };
        if !parts.status.is_success() {
            // Forward the upstream error (status + body) unchanged.
            return (parts.status, bytes).into_response();
        }
        let cc: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            // Non-JSON 200 — forward as-is rather than guessing.
            Err(_) => return (parts.status, bytes).into_response(),
        };
        let resp_json = chat_completion_to_responses(&cc);
        return (StatusCode::OK, Json(resp_json)).into_response();
    }

    // 5b. Streaming: the chat pipeline (via `serve_stream`) already converted
    //     the upstream Chat SSE into Responses SSE events AND wrote the request
    //     log (with the full Response-shaped body). Just pass it through.
    chat_resp
}

/// Convert a Responses `input` (plus optional top-level `instructions`) into
/// OpenAI Chat `messages`.
fn responses_input_to_messages(req: &serde_json::Value) -> Vec<serde_json::Value> {
    let mut messages = Vec::new();
    if let Some(instr) = req.get("instructions").and_then(|v| v.as_str()) {
        if !instr.is_empty() {
            messages.push(json!({ "role": "system", "content": instr }));
        }
    }
    if let Some(input) = req.get("input") {
        match input {
            serde_json::Value::String(s) => {
                messages.push(json!({ "role": "user", "content": s }));
            }
            serde_json::Value::Array(items) => {
                for item in items {
                    let role = item
                        .get("role")
                        .and_then(|v| v.as_str())
                        .unwrap_or("user");
                    let text = match item.get("content") {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(serde_json::Value::Array(parts)) => {
                            let mut buf = String::new();
                            for p in parts {
                                if let Some(t) = p.get("text").and_then(|v| v.as_str()) {
                                    buf.push_str(t);
                                }
                            }
                            buf
                        }
                        _ => String::new(),
                    };
                    messages.push(json!({ "role": role, "content": text }));
                }
            }
            _ => {}
        }
    }
    messages
}

/// Convert an OpenAI Chat completion into the Responses API shape.
fn chat_completion_to_responses(cc: &serde_json::Value) -> serde_json::Value {
    let model = cc.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let created = cc.get("created").and_then(|v| v.as_i64()).unwrap_or(0);
    let id = cc.get("id").and_then(|v| v.as_str()).unwrap_or("unknown");
    let id_core = id.trim_start_matches("chatcmpl-");
    let choice = cc.get("choices").and_then(|c| c.get(0));
    let message = choice.and_then(|c| c.get("message"));
    let content_text = message
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let reasoning = message
        .and_then(|m| m.get("reasoning_content"))
        .and_then(|v| v.as_str());
    let mut content_items: Vec<serde_json::Value> = Vec::new();
    if let Some(r) = reasoning {
        if !r.is_empty() {
            content_items.push(json!({ "type": "reasoning", "summary": [r] }));
        }
    }
    content_items.push(json!({ "type": "output_text", "text": content_text }));
    let usage = cc.get("usage");
    let (in_t, out_t, tot_t) = match usage {
        Some(u) => (
            u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            u.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            u.get("total_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        ),
        None => (0, 0, 0),
    };
    json!({
        "id": format!("resp_{}", id_core),
        "object": "response",
        "created_at": created,
        "model": model,
        "status": "completed",
        "output": [
            {
                "type": "message",
                "id": format!("msg_{}", id_core),
                "role": "assistant",
                "status": "completed",
                "content": content_items,
            }
        ],
        "usage": {
            "input_tokens": in_t,
            "output_tokens": out_t,
            "total_tokens": tot_t,
        }
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

/// Build an OpenAI-shaped error response.
fn error_response(status: StatusCode, code: &str, message: &str) -> Response {
    (
        status,
        Json(json!({ "error": { "message": message, "code": code } })),
    )
        .into_response()
}

/// Spawn an async task to insert a `request_logs` row. Fire-and-forget —
/// logging failures must never break the response path.
#[allow(clippy::too_many_arguments)]
fn spawn_log(
    state: Arc<AppState>,
    api_key_name: Option<String>,
    channel_name: Option<String>,
    model: String,
    upstream_model: Option<String>,
    status_code: i32,
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
    duration_ms: i64,
    error_message: Option<String>,
    is_stream: bool,
    is_retry: bool,
    request_body: Option<String>,
    response_body: Option<String>,
    mode: &'static str,
) {
    tokio::spawn(async move {
        let _ = request_logs::insert(
            &state.db,
            api_key_name.as_deref(),
            channel_name.as_deref(),
            &model,
            upstream_model.as_deref(),
            mode,
            status_code,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            duration_ms,
            error_message.as_deref(),
            is_stream,
            is_retry,
            request_body.as_deref(),
            response_body.as_deref(),
            "none",
            0,
            None,
            "none",
            false,
            None,
        )
        .await;
    });
}

/// 上游 HTTP 状态是否应计入熔断器（参考 waliapi 的错误分类思想）：
/// 5xx / 429 / 408 / 409 = 可重试的上游故障 → 计入熔断；
/// 其余（401/403 鉴权、400/422 客户端错误）不是渠道本身的问题 → 不计入。
fn is_retryable_status(status: Option<u16>) -> bool {
    matches!(
        status,
        Some(408) | Some(409) | Some(429) | Some(500..=599)
    )
}

/// 把一次上游调用结果写回渠道健康表，驱动熔断器：
/// - 成功 → 重置熔断器；
/// - 失败且可重试 → 累加失败，达阈值后打开熔断器；
/// - 失败但不可重试（鉴权/客户端错误）→ 不动熔断器。
///
/// 必须忽略错误：健康统计不能影响响应路径（与配额扣减同理）。
async fn record_upstream_outcome(
    state: &Arc<AppState>,
    channel_id: &str,
    success: bool,
    retryable: bool,
    reason: &str,
) {
    if success {
        let _ = channel_health::record_success(&state.db, channel_id).await;
    } else if retryable {
        let _ = channel_health::record_failure(&state.db, channel_id, reason).await;
    }
}

// ---------------------------------------------------------------------------
// Streaming (SSE)
// ---------------------------------------------------------------------------

/// 流式「尝试」单元（可被故障转移循环包裹）：发起上游 SSE 请求并校验状态码，
/// 仅在返回 2xx 时才把 `reqwest::Response` 交给调用方建立 SSE；
/// 连接失败 / 超时 / 非 2xx 均发生在向客户端写入任何字节之前，因此可安全重试。
async fn acquire_stream_response(
    adaptor: &dyn Adaptor,
    proxy_req: &ProxyRequest,
    config: &ChannelConfig,
) -> Result<reqwest::Response, failover::Outcome> {
    let resp = match adaptor.forward_stream(proxy_req, config).await {
        Ok(r) => r,
        Err(e) => return Err(failover::Outcome::connection(e.to_string())),
    };
    let status = resp.status();
    if !status.is_success() {
        let status_code = status.as_u16();
        let body_text = resp.text().await.unwrap_or_default();
        let msg = serde_json::from_str::<serde_json::Value>(&body_text)
            .ok()
            .and_then(|v| {
                v.pointer("/error/message")
                    .and_then(|m| m.as_str())
                    .map(String::from)
            })
            .unwrap_or_else(|| truncate(&body_text, 300));
        return Err(failover::Outcome::upstream(
            status_code,
            msg,
            is_retryable_status(Some(status_code)),
        ));
    }
    Ok(resp)
}

/// Forward a streaming request and return an SSE `Response`.
///
/// OpenAI-compatible upstreams (OpenAI / DeepSeek / OpenAI-compatible custom
/// channels) are passed through byte-for-byte. Claude and Gemini return their
/// native SSE, which is converted chunk-by-chunk into OpenAI
/// `chat.completion.chunk` frames by the per-provider converter.
///
/// The upstream `model` name is preserved in the converted frames (it is NOT
/// rewritten to the client's alias) — keeping
/// "which model actually generated this" truthful.
async fn serve_stream(
    state: Arc<AppState>,
    gw_key_name: String,
    channel_name: String,
    _channel_id: String,
    model: String,
    upstream_model: String,
    is_stream: bool,
    start: std::time::Instant,
    raw_request: Option<String>,
    log_raw_body: bool,
    channel_type: &str,
    resp: reqwest::Response,
    is_retry: bool,
    mode: &'static str,
) -> Response {
    // `resp` 已在调用方（故障转移循环）确认是 2xx，这里直接构建 SSE。
    let converter = match channel_type {
        "claude" => StreamConverter::Claude,
        "gemini" => StreamConverter::Gemini,
        _ => StreamConverter::None,
    };

    // Responses streaming: the shared pipeline produced a Chat SSE stream.
    // Convert it into Responses SSE events here AND own the log (with the full
    // Response-shaped body). We deliberately skip `build_stream_response`'s own
    // log (do_log=false) to avoid a duplicate, empty-body row.
    if mode == "responses" {
        let chat_resp = build_stream_response(
            state.clone(),
            gw_key_name.clone(),
            channel_name.clone(),
            model.clone(),
            upstream_model.clone(),
            is_stream,
            start,
            raw_request.clone(),
            resp,
            converter,
            false, // do_log: the Responses converter below logs instead
            log_raw_body,
            is_retry,
            "responses",
        );
        let (parts, body) = chat_resp.into_parts();
        if !parts.status.is_success() {
            return (parts.status, body).into_response();
        }
        return build_responses_stream_response(
            body,
            state,
            gw_key_name,
            channel_name,
            model,
            upstream_model,
            start,
            raw_request,
            log_raw_body,
            is_retry,
            mode,
        );
    }

    build_stream_response(
        state,
        gw_key_name,
        channel_name,
        model,
        upstream_model,
        is_stream,
        start,
        raw_request,
        resp,
        converter,
        true, // do_log
        log_raw_body,
        is_retry,
        mode,
    )
}

/// Build the SSE `Response`. Frames are forwarded/transformed lazily as the
/// upstream produces them, so the client sees tokens arrive in real time.
/// Token usage is scanned from the stream and written to the log at
/// end-of-stream (the streaming usage arrives on a dedicated frame).
///
/// `Body::from_stream` requires `S: TryStream + Send + 'static`, where `TryStream`
/// comes from `futures-core` 0.3. We isolate the upstream reading + SSE conversion
/// inside a spawned task and push converted frames through an `mpsc` channel. The
/// response body is then built from `ReceiverStream` (also `futures-core` 0.3), which
/// satisfies the bound directly. We deliberately avoid `async_stream!` here: its
/// `AsyncStream` resolves `Stream` against `futures-core-preview` (a different crate
/// version), which fails the `TryStream` trait bound.
fn build_stream_response(
    state: Arc<AppState>,
    gw_key_name: String,
    channel_name: String,
    model: String,
    upstream_model: String,
    is_stream: bool,
    start: std::time::Instant,
    raw_request: Option<String>,
    resp: reqwest::Response,
    converter: StreamConverter,
    do_log: bool,
    log_raw_body: bool,
    is_retry: bool,
    mode: &'static str,
) -> Response {
    use tokio::sync::mpsc;

    /// Per-protocol streaming converter state.
    enum Conv {
        Claude(crate::adapter::claude::AnthropicSseConverter),
        Gemini(crate::adapter::gemini::GeminiSseConverter),
        None,
    }

    // Channel carrying converted SSE frames. Bounded so a slow client doesn't
    // let the upstream buffer grow unbounded.
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);

    // Producer: owns the (possibly non-`Send`) upstream byte stream and runs
    // all conversion. Runs on its own task so the response body stream below
    // never has to be `Send`.
    tokio::spawn(async move {
        let mut upstream = resp.bytes_stream();
        let mut acc = StreamUsage::default();
        let mut buf = String::new();
        // Accumulate the (Chat-shaped) frames so the request log can record the
        // full response body when `log_raw_body` is enabled.
        let mut response_body_acc = String::new();
        let mut conv = match converter {
            StreamConverter::Claude => {
                Conv::Claude(crate::adapter::claude::AnthropicSseConverter::new(upstream_model.clone()))
            }
            StreamConverter::Gemini => {
                Conv::Gemini(crate::adapter::gemini::GeminiSseConverter::new(upstream_model.clone()))
            }
            StreamConverter::None => Conv::None,
        };
        let mut had_error = false;

        while let Some(chunk) = upstream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    had_error = true;
                    let _ = tx
                        .send(Ok::<_, std::io::Error>(Bytes::from(format!(
                            "data: {}\n\n",
                            json!({ "error": { "message": e.to_string() } })
                        ))))
                        .await;
                    break;
                }
            };
            match &mut conv {
                Conv::None => {
                    // OpenAI-compatible passthrough: scan usage, forward bytes.
                    if let Ok(s) = std::str::from_utf8(&chunk) {
                        scan_openai_usage(s, &mut acc);
                        if log_raw_body {
                            response_body_acc.push_str(s);
                        }
                        let _ = tx.send(Ok::<_, std::io::Error>(chunk)).await;
                    }
                }
                Conv::Claude(c) => {
                    buf.push_str(&String::from_utf8_lossy(&chunk));
                    let (records, rest) = split_sse_records(&buf);
                    buf = rest;
                    for rec in records {
                        for f in c.convert(&rec, &mut acc) {
                            if log_raw_body {
                                response_body_acc.push_str(&f);
                            }
                            let _ = tx.send(Ok::<_, std::io::Error>(Bytes::from(f))).await;
                        }
                    }
                }
                Conv::Gemini(c) => {
                    buf.push_str(&String::from_utf8_lossy(&chunk));
                    let (records, rest) = split_sse_records(&buf);
                    buf = rest;
                    for rec in records {
                        for f in c.convert(&rec, &mut acc) {
                            if log_raw_body {
                                response_body_acc.push_str(&f);
                            }
                            let _ = tx.send(Ok::<_, std::io::Error>(Bytes::from(f))).await;
                        }
                    }
                }
            }
        }

        // Flush any trailing partial record (converter modes only).
        if !buf.is_empty() {
            let (records, _) = split_sse_records(&buf);
            match &mut conv {
                Conv::Claude(c) => {
                    for rec in records {
                        for f in c.convert(&rec, &mut acc) {
                            if log_raw_body {
                                response_body_acc.push_str(&f);
                            }
                            let _ = tx.send(Ok::<_, std::io::Error>(Bytes::from(f))).await;
                        }
                    }
                }
                Conv::Gemini(c) => {
                    for rec in records {
                        for f in c.convert(&rec, &mut acc) {
                            if log_raw_body {
                                response_body_acc.push_str(&f);
                            }
                            let _ = tx.send(Ok::<_, std::io::Error>(Bytes::from(f))).await;
                        }
                    }
                }
                Conv::None => {}
            }
        }

        // Anthropic/Gemini don't emit [DONE]; append it for converted protocols.
        if !matches!(converter, StreamConverter::None) && !had_error {
            if log_raw_body {
                response_body_acc.push_str("data: [DONE]\n\n");
            }
            let _ = tx
                .send(Ok::<_, std::io::Error>(Bytes::from("data: [DONE]\n\n")))
                .await;
        }

        let duration_ms = start.elapsed().as_millis() as i64;
        if do_log {
            spawn_log(
                state,
                Some(gw_key_name),
                Some(channel_name),
                model,
                Some(upstream_model),
                if had_error { 502 } else { 200 },
                acc.prompt_tokens,
                acc.completion_tokens,
                acc.total_tokens,
                duration_ms,
                if had_error {
                    Some("stream interrupted".to_string())
                } else {
                    None
                },
                is_stream,
                is_retry,
                raw_request,
                if log_raw_body {
                    Some(response_body_acc)
                } else {
                    None
                },
                mode,
            );
        }
    });

    // Consumer: a `Send` `Stream` over the channel. `ReceiverStream` is built on
    // `futures-core` 0.3 (the same crate `axum` uses for `Body::from_stream`), so it
    // satisfies `TryStream + Send + 'static` directly — unlike `async_stream!`, whose
    // `AsyncStream` resolves `Stream` against `futures-core-preview` and therefore
    // fails the trait bound.
    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
        .header(axum::http::header::CACHE_CONTROL, "no-cache")
        .header(axum::http::header::CONNECTION, "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap()
}

/// Build the Responses SSE `Response` for `/v1/responses` streaming.
///
/// The upstream (already normalized to OpenAI Chat SSE by the shared pipeline)
/// is fed through a converter that turns each Chat `chat.completion.chunk`
/// frame into the appropriate Responses event(s). Like `build_stream_response`,
/// the conversion runs on a spawned task and frames are pushed through an
/// `mpsc` channel so the outer `ReceiverStream` is `Send` and satisfies
/// `Body::from_stream`'s bound (no `async_stream!` / futures-core conflict).
fn build_responses_stream_response(
    body: Body,
    state: Arc<AppState>,
    gw_key_name: String,
    channel_name: String,
    model: String,
    upstream_model: String,
    start: std::time::Instant,
    raw_request: Option<String>,
    log_raw_body: bool,
    is_retry: bool,
    mode: &'static str,
) -> Response {
    use futures_util::StreamExt;
    use tokio::sync::mpsc;

    let response_id = crate::responses_stream::new_response_id();
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);
    // Accumulate the Responses-shaped events so the request log can record the
    // full response body (only when `log_raw_body` is enabled).
    let mut response_body_acc = String::new();

    tokio::spawn(async move {
        let mut data_stream = body.into_data_stream();
        let mut rs_state = crate::responses_stream::ResponsesStreamState::default();
        let mut acc = crate::adapter::StreamUsage::default();

        // Opening events must precede any delta.
        let created = crate::responses_stream::created_events(&response_id, &model);
        if log_raw_body {
            response_body_acc.push_str(&created);
        }
        let _ = tx
            .send(Ok::<_, std::io::Error>(Bytes::from(created)))
            .await;
        // created_events consumed sequence numbers 0 and 1; continue from there.
        rs_state.sequence_number = 1;

        let mut had_error = false;
        while let Some(chunk) = data_stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    had_error = true;
                    let err_frame = format!(
                        "data: {}\n\n",
                        json!({ "error": { "message": e.to_string() } })
                    );
                    if log_raw_body {
                        response_body_acc.push_str(&err_frame);
                    }
                    let _ = tx
                        .send(Ok::<_, std::io::Error>(Bytes::from(err_frame)))
                        .await;
                    break;
                }
            };
            let text = String::from_utf8_lossy(&chunk);
            crate::adapter::scan_openai_usage(&text, &mut acc);
            for ev in crate::responses_stream::convert_chunk(&text, &response_id, &mut rs_state) {
                if log_raw_body {
                    response_body_acc.push_str(&ev);
                }
                let _ = tx.send(Ok::<_, std::io::Error>(Bytes::from(ev))).await;
            }
        }

        if !had_error {
            for ev in crate::responses_stream::completed_events(
                &response_id,
                &model,
                &mut rs_state,
                &acc,
            ) {
                if log_raw_body {
                    response_body_acc.push_str(&ev);
                }
                let _ = tx.send(Ok::<_, std::io::Error>(Bytes::from(ev))).await;
            }
        }

        // End-of-stream: write the request log with the full Response-shaped body.
        let duration_ms = start.elapsed().as_millis() as i64;
        spawn_log(
            state,
            Some(gw_key_name),
            Some(channel_name),
            model,
            Some(upstream_model),
            if had_error { 502 } else { 200 },
            acc.prompt_tokens,
            acc.completion_tokens,
            acc.total_tokens,
            duration_ms,
            if had_error {
                Some("stream interrupted".to_string())
            } else {
                None
            },
            true, // is_stream
            is_retry,
            raw_request,
            if log_raw_body {
                Some(response_body_acc)
            } else {
                None
            },
            mode,
        );
    });

    let stream = tokio_stream::wrappers::ReceiverStream::new(rx);

    Response::builder()
        .status(StatusCode::OK)
        .header(axum::http::header::CONTENT_TYPE, "text/event-stream")
        .header(axum::http::header::CACHE_CONTROL, "no-cache")
        .header(axum::http::header::CONNECTION, "keep-alive")
        .body(Body::from_stream(stream))
        .unwrap()
}

/// Truncate a string to at most `n` chars for error messages / logs.
fn truncate(s: &str, n: usize) -> String {
    if s.chars().count() <= n {
        s.to_string()
    } else {
        format!("{}…", s.chars().take(n).collect::<String>())
    }
}
