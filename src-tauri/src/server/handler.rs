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
use crate::db::repository::{
    channel_health, channels, gateway_keys, request_logs, security_findings,
};
use crate::security::{self, redact, SecurityAction, SecurityFinding, SecurityOutcome};
use crate::server::auth;
use crate::app_settings::AppSettings;
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

    // 单请求内设置/规则缓存：启动与设置变更时由 AppState.settings_cache 加载，
    // 这里只读一次本地镜像，避免每条请求 20+ 次 settings/rules 的重复读库。
    // 注意：必须先把 guard 的结果取出来再 await。若直接在 `match state.settings_cache.read()`
    // 的 Err 分支里 await，guard 临时值会存活到整个 match 结束，跨 await 持有非 Send 的
    // 锁守卫会让 handler 的 future 失去 Send，axum 的 Handler 约束随之不满足。
    let cached: Option<AppSettings> = match state.settings_cache.read() {
        Ok(g) => Some(g.clone()),
        Err(_) => None,
    };
    let app_settings = match cached {
        Some(s) => s,
        None => {
            // 锁中毒（理论不可能）：降级即时读库，保证 fail-open 不丢功能。
            tracing::warn!("settings_cache 读锁中毒，降级即时读库");
            AppSettings::load(&state.db)
                .await
                .unwrap_or_else(|_| AppSettings::conservative_default())
        }
    };
    // Read the "log raw request/response body" toggle (default off).
    let log_raw_body = app_settings.log_raw_body;
    // 响应体日志脱敏开关（security_redact_secrets）：开启时落库的
    // 响应体(非流式 JSON / 流式 SSE 文本)统一掩高风险明文，闭合 G3 响应半边。
    let sec_redact = app_settings.security.settings.redact_secrets;
    // 日志请求体在请求 JSON 解析后构造（见 body_json 之后），统一走脱敏副本，确保 DB 不落明文。

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

    // 1.5 请求限流：按网关密钥滑动窗口限速（设置 enable_rate_limit 开启时生效）。
    //     超限直接返回 429，不进后续解析/分发，避免无效上游请求占用配额。
    {
        let rl = match state.rate_limiter.lock() {
            Ok(g) => g,
            Err(_) => {
                return error_response(
                    StatusCode::INTERNAL_SERVER_ERROR,
                    "internal",
                    "限速器状态损坏",
                );
            }
        };
        if rl.enabled && rl.limiter.check(key.as_str()).is_err() {
            return error_response(
                StatusCode::TOO_MANY_REQUESTS,
                "rate_limited",
                "请求过于频繁，已超过每分钟允许的请求数上限",
            );
        }
    }

    // 2. Parse
    let body_json: serde_json::Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => return error_response(StatusCode::BAD_REQUEST, "invalid_json", &e.to_string()),
    };

    // 日志请求体：仅 log_raw_body 开启时记录。统一走脱敏副本——redact 仅掩 high+ 类别
    // （密钥/卡号/私钥/外传命令/可疑域名等），确保本地 DB 永不落明文高风险凭证，
    // 闭合初版 G3「日志永远脱敏」隐私目标。低/中风险（邮箱/手机/身份证）仍保留以便调试。
    let raw_request = if log_raw_body {
        let sanitized = redact::redact(&body_json);
        serde_json::to_string(&sanitized).ok()
    } else {
        None
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

    // 2.5 安全审计闸门：解析后、分发前对原始请求体扫描。
    // 使用缓存的安全上下文（无 DB 读）；上下文为空/禁用时 gate 内部 fail-open 放行。
    let gate = security::gate::run_gate_ctx(&app_settings.security, body_json.clone());
    let sec_outcome = gate.outcome.clone();
    let sec_findings = gate.findings.clone();

    // 严格模式命中高风险 → 直接阻断（先落日志与发现，再回 403）。
    if gate.action == SecurityAction::Block {
        let duration_ms = start.elapsed().as_millis() as i64;
        spawn_log(
            state.clone(),
            Some(gw_key.name.clone()),
            None,
            model.clone(),
            None,
            403,
            0,
            0,
            0,
            duration_ms,
            gate.outcome.blocked_reason.clone(),
            is_stream,
            false,
            raw_request.clone(),
            None,
            mode,
            sec_outcome.clone(),
            sec_findings.clone(),
        );
        return error_response(
            StatusCode::FORBIDDEN,
            "security_blocked",
            gate.outcome.blocked_reason.as_deref().unwrap_or("请求被安全审计阻断"),
        );
    }

    // 实际转发体：脱敏模式下为脱敏副本，否则为原始请求体。
    let forward_body = gate.forward_body.clone();

    // 3. Load retry settings (defaults: enabled, 3 retries).
    //    retry_times = 「额外重试次数」; max_attempts = retry_times + 1（含首次）。
    //    取自缓存镜像，避免重复读库。
    let retry_enabled: bool = app_settings.retry_enabled;
    let retry_times: usize = app_settings.retry_times.max(0) as usize;
    let max_attempts = if retry_enabled { retry_times + 1 } else { 1 };

    // 4. Dispatch + forward, wrapped in an automatic channel-failover state
    //    machine (core/failover.rs):
    //    pick a candidate channel -> forward -> on a *retryable* failure, record
    //    it against the circuit breaker and try the next candidate channel.
    let ctx = dispatcher::DispatchContext {
        model: model.clone(),
        api_key_id: gw_key.id.clone(),
        is_stream,
        request_body: forward_body.clone(),
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
            body: forward_body.clone(),
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
                        sec_outcome.clone(),
                        sec_findings.clone(),
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
                        sec_outcome.clone(),
                        sec_findings.clone(),
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
                gw_key.id.clone(),
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
                sec_outcome.clone(),
                sec_findings.clone(),
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
            // 配额超限自动禁用由 add_quota_used 的 SQL 层完成。
            if tt > 0 {
                let _ = gateway_keys::add_quota_used(&state.db, &gw_key.id, tt).await;
            }

            let duration_ms = start.elapsed().as_millis() as i64;
            // 日志落库的响应体须与「用户实际收到的形态」一致：
            // - chat / responses 模式：上游返回即 OpenAI / Responses 形态，直接落库；
            // - messages 模式：上游返回 OpenAI Chat 完成体，handler 随后会转成
            //   Anthropic Messages 形态回给客户端（见 `messages()` 的
            //   `chat_completion_to_anthropic`）。此处先转成 Anthropic 形态再落库，
            //   否则日志响应仍是 OpenAI 结构，LogsPage 按 mode="messages" 用
            //   parseAnthropicResponse 解析会失败、回退原始 JSON 视图。
            let logged_resp_body = if mode == "messages" {
                chat_completion_to_anthropic(&resp_body)
            } else {
                resp_body.clone()
            };
            // 响应体日志脱敏：security_redact_secrets 开启时掩高风险明文（与请求体一致）。
            let raw_response = if log_raw_body {
                let body = if sec_redact {
                    redact::redact(&logged_resp_body)
                } else {
                    logged_resp_body.clone()
                };
                serde_json::to_string(&body).ok()
            } else {
                None
            };

            // 响应侧扫描（security_scan_response）：非流式响应按启用规则扫描，
            // 发现的 phase="response" 并入本次审计；并以「请求+响应」合并发现
            // 重算 risk_level/risk_score/risk_summary，确保主行汇总与落库
            // findings 一致（原逻辑只取较高阶段 summary、用 += 叠加 score，
            // 三者互不对应——修复见 security::compute_risk_metrics）。
            let mut sec_outcome = sec_outcome.clone();
            let mut sec_findings = sec_findings.clone();
            // 使用缓存的安全上下文（无 DB 读）。
            let resp_gate =
                security::gate::scan_response_ctx(&app_settings.security, resp_body.clone());
            if !resp_gate.findings.is_empty() {
                for f in &resp_gate.findings {
                    sec_findings.push(f.clone());
                }
                // 合并全部发现后重算风险汇总（保留请求阶段已定的
                // 动作/脱敏/拦截原因，仅刷新风险指标字段）。
                let (rl, rs, summ, _top_title) = security::compute_risk_metrics(&sec_findings);
                sec_outcome.risk_level = rl;
                sec_outcome.risk_score = rs;
                sec_outcome.risk_summary = summ;
            }

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
                sec_outcome.clone(),
                sec_findings.clone(),
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

/// POST /v1/messages — Anthropic Messages API bridge.
///
/// Translates the Anthropic request (`system` / `messages` / `max_tokens`) into
/// an OpenAI Chat request, delegates to the shared chat pipeline
/// (`run_chat_pipeline`, which performs auth + dispatch + adapt + log — the
/// internal representation is always OpenAI Chat, so the gateway key carried in
/// the `x-api-key` header is the DongX *gateway* key, validated exactly like
/// the OpenAI `Authorization: Bearer` one), and translates the Chat response
/// back into the Anthropic Messages shape. Both streaming and non-streaming are
/// supported: the outbound Chat SSE stream is converted frame-by-frame into
/// Anthropic Messages SSE events. DongX's `/v1/messages` reuses the same chat
/// executor and only the entry (request) and exit (response) translation
/// differs.
pub async fn messages(
    State(app): State<AppHandle>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // 1. Parse Anthropic request.
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

    // 2. Translate Anthropic `system`/`messages`/`max_tokens` -> Chat `messages`.
    let chat_body = anthropic_to_chat_request(&req);
    let chat_bytes = match serde_json::to_vec(&chat_body) {
        Ok(b) => b,
        Err(e) => {
            return error_response(StatusCode::INTERNAL_SERVER_ERROR, "internal", &e.to_string())
        }
    };

    // 3. Delegate to the shared chat pipeline (auth/dispatch/adapt/log reused).
    //    `mode = "messages"` so the request log records this entry as a
    //    Messages call. The downstream `x-api-key` header (carrying the DongX
    //    gateway key) is recognised by `auth::extract_gateway_key`.
    let chat_resp =
        run_chat_pipeline(app, headers, axum::body::Bytes::from(chat_bytes), "messages").await;

    // 4. Translate the Chat response back into Anthropic shape.
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
            // Translate the upstream OpenAI-shaped error into Anthropic's error
            // shape so Anthropic SDK clients parse it correctly.
            if let Ok(j) = serde_json::from_slice::<serde_json::Value>(&bytes) {
                if let Some(err) = j.get("error") {
                    let msg = err
                        .get("message")
                        .and_then(|v| v.as_str())
                        .unwrap_or("gateway error");
                    let kind = err
                        .get("code")
                        .and_then(|v| v.as_str())
                        .unwrap_or("api_error");
                    let anthropic_err = json!({
                        "type": "error",
                        "error": { "type": kind, "message": msg }
                    });
                    return (parts.status, Json(anthropic_err)).into_response();
                }
            }
            // Non-JSON / unrecognised error — forward as-is.
            return (parts.status, bytes).into_response();
        }
        let cc: serde_json::Value = match serde_json::from_slice(&bytes) {
            Ok(v) => v,
            // Non-JSON 200 — forward as-is rather than guessing.
            Err(_) => return (parts.status, bytes).into_response(),
        };
        let anthropic = chat_completion_to_anthropic(&cc);
        return (StatusCode::OK, Json(anthropic)).into_response();
    }

    // 4b. Streaming: `serve_stream` (invoked inside `run_chat_pipeline`) already
    //     converted the upstream Chat SSE into Anthropic Messages SSE events
    //     AND wrote the request log (with the full Anthropic-shaped body). Pass
    //     it through unchanged.
    chat_resp
}

/// Convert an Anthropic Messages request into an OpenAI Chat request.
///
/// Handles `system` (string or `[{type:"text",text}]` blocks), `messages`
/// (string or `[{type:"text",text}]` blocks; tool_result text is extracted
/// best-effort), and forwards common sampler params (`temperature`, `top_p`,
/// `max_tokens`, `stop`, `top_k`, `seed`). `max_tokens` is `required` by
/// Anthropic and maps directly onto the same-named Chat field.
fn anthropic_to_chat_request(req: &serde_json::Value) -> serde_json::Value {
    let mut messages: Vec<serde_json::Value> = Vec::new();

    // system -> Chat system message
    if let Some(sys) = req.get("system") {
        let text = match sys {
            serde_json::Value::String(s) => serde_json::Value::String(s.clone()),
            serde_json::Value::Array(arr) => blocks_to_text(arr),
            _ => serde_json::Value::Null,
        };
        if let serde_json::Value::String(t) = &text {
            if !t.is_empty() {
                messages.push(json!({ "role": "system", "content": t }));
            }
        }
    }

    if let Some(msgs) = req.get("messages").and_then(|v| v.as_array()) {
        for m in msgs {
            let role = m.get("role").and_then(|v| v.as_str()).unwrap_or("user");
            let content = m.get("content");
            let text = match content {
                Some(serde_json::Value::String(s)) => serde_json::Value::String(s.clone()),
                Some(serde_json::Value::Array(arr)) => {
                    let t = blocks_to_text(arr);
                    if let serde_json::Value::String(s) = &t {
                        if s.is_empty() {
                            serde_json::Value::Null
                        } else {
                            t
                        }
                    } else {
                        serde_json::Value::Null
                    }
                }
                _ => serde_json::Value::Null,
            };
            if text.is_null() {
                // Fall back to passing the raw content through (e.g. tool_use
                // blocks) — best effort; the upstream adapts what it can.
                messages.push(json!({ "role": role, "content": content }));
            } else {
                messages.push(json!({ "role": role, "content": text }));
            }
        }
    }

    let mut chat = json!({
        "model": req.get("model").cloned().unwrap_or(json!("")),
        "messages": messages,
        "stream": req.get("stream").cloned().unwrap_or(json!(false)),
    });
    for f in [
        "temperature",
        "top_p",
        "top_k",
        "max_tokens",
        "stop",
        "seed",
    ] {
        if let Some(v) = req.get(f) {
            chat[f] = v.clone();
        }
    }
    chat
}

/// Concatenate the text of `[{type:"text",text}, ...]` content blocks.
fn blocks_to_text(blocks: &[serde_json::Value]) -> serde_json::Value {
    let mut buf = String::new();
    for b in blocks {
        let t = b.get("type").and_then(|v| v.as_str()).unwrap_or("");
        match t {
            "text" => {
                if let Some(s) = b.get("text").and_then(|v| v.as_str()) {
                    buf.push_str(s);
                }
            }
            "tool_result" => {
                if let Some(c) = b.get("content").and_then(|v| v.as_str()) {
                    buf.push_str(c);
                }
            }
            _ => {}
        }
    }
    serde_json::Value::String(buf)
}

/// Convert an OpenAI Chat completion into the Anthropic Messages shape.
fn chat_completion_to_anthropic(cc: &serde_json::Value) -> serde_json::Value {
    let id = cc.get("id").and_then(|v| v.as_str()).unwrap_or("msg_unknown");
    // OpenAI ids look like "chatcmpl-xxx"; Anthropic message ids are "msg_...".
    let anthropic_id = if let Some(core) = id.strip_prefix("chatcmpl-") {
        format!("msg_{}", core)
    } else {
        id.to_string()
    };
    let model = cc.get("model").and_then(|v| v.as_str()).unwrap_or("");
    let created = cc.get("created").and_then(|v| v.as_i64()).unwrap_or(0);
    let message = cc
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"));
    let content_text = message
        .and_then(|m| m.get("content"))
        .and_then(|v| v.as_str())
        .unwrap_or("");
    let reasoning = message
        .and_then(|m| m.get("reasoning_content"))
        .and_then(|v| v.as_str());

    let mut content_blocks: Vec<serde_json::Value> = Vec::new();
    if let Some(r) = reasoning {
        if !r.is_empty() {
            content_blocks.push(json!({ "type": "thinking", "thinking": r }));
        }
    }
    content_blocks.push(json!({ "type": "text", "text": content_text }));

    let usage = cc.get("usage");
    let (in_t, out_t) = match usage {
        Some(u) => (
            u.get("prompt_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
            u.get("completion_tokens").and_then(|v| v.as_i64()).unwrap_or(0),
        ),
        None => (0, 0),
    };

    json!({
        "id": anthropic_id,
        "type": "message",
        "role": "assistant",
        "model": model,
        "created_at": created,
        "content": content_blocks,
        "stop_reason": "end_turn",
        "stop_sequence": null,
        "usage": { "input_tokens": in_t, "output_tokens": out_t },
    })
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

/// Spawn an async task to insert a `request_logs` row plus its security
/// findings. Fire-and-forget — failures must never break the response path
/// (same principle as quota/health stats).
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
    sec: SecurityOutcome,
    findings: Vec<SecurityFinding>,
) {
    tokio::spawn(async move {
        let log_id = match request_logs::insert(
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
            &sec.risk_level,
            sec.risk_score,
            sec.risk_summary.as_deref(),
            &sec.security_action,
            sec.sanitized,
            sec.blocked_reason.as_deref(),
        )
        .await
        {
            Ok(id) => id,
            Err(e) => {
                tracing::warn!("请求日志写入失败: {}", e);
                return;
            }
        };

        // 安全发现明细：关联本次请求日志。
        for f in &findings {
            if let Err(e) =
                security_findings::insert(&state.db, &log_id, f, &sec.security_action).await
            {
                tracing::warn!("安全发现写入失败: {}", e);
            }
        }
    });
}

/// 上游 HTTP 状态是否应计入熔断器（按错误分类思想）：
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
    gw_key_id: String,
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
    sec: SecurityOutcome,
    findings: Vec<SecurityFinding>,
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
            Some(gw_key_id.clone()),
            channel_name.clone(),
            model.clone(),
            upstream_model.clone(),
            is_stream,
            start,
            raw_request.clone(),
            resp,
            converter,
            // do_log=false: this pass is a pure Chat->Chat relay; the Responses
            // converter below owns the log AND the quota debit.
            false,
            log_raw_body,
            is_retry,
            "responses",
            sec.clone(),
            findings.clone(),
        );
        let (parts, body) = chat_resp.into_parts();
        if !parts.status.is_success() {
            return (parts.status, body).into_response();
        }
        return build_responses_stream_response(
            body,
            state,
            gw_key_name,
            Some(gw_key_id),
            channel_name,
            model,
            upstream_model,
            start,
            raw_request,
            log_raw_body,
            is_retry,
            mode,
            sec,
            findings,
        );
    }

    // Messages streaming: same as the Responses branch — the shared pipeline
    // produced a Chat SSE stream (OpenAI Chat SSE, always), which is converted
    // into Anthropic Messages SSE events here. This pass owns the log (with the
    // full Anthropic-shaped body) AND the gateway-key quota debit; the relay
    // pass below runs with do_log=false to avoid a duplicate, empty-body row.
    if mode == "messages" {
        let chat_resp = build_stream_response(
            state.clone(),
            gw_key_name.clone(),
            Some(gw_key_id.clone()),
            channel_name.clone(),
            model.clone(),
            upstream_model.clone(),
            is_stream,
            start,
            raw_request.clone(),
            resp,
            converter,
            // do_log=false: this pass is a pure Chat->Chat relay; the Messages
            // converter below owns the log AND the quota debit.
            false,
            log_raw_body,
            is_retry,
            "messages",
            sec.clone(),
            findings.clone(),
        );
        let (parts, body) = chat_resp.into_parts();
        if !parts.status.is_success() {
            return (parts.status, body).into_response();
        }
        return build_messages_stream_response(
            body,
            state,
            gw_key_name,
            Some(gw_key_id),
            channel_name,
            model,
            upstream_model,
            start,
            raw_request,
            log_raw_body,
            is_retry,
            mode,
            sec,
            findings,
        );
    }

    build_stream_response(
        state,
        gw_key_name,
        Some(gw_key_id),
        channel_name,
        model,
        upstream_model,
        is_stream,
        start,
        raw_request,
        resp,
        converter,
        true, // do_log — also owns the end-of-stream quota debit
        log_raw_body,
        is_retry,
        mode,
        sec,
        findings,
    )
}

/// Build the SSE `Response`. Frames are forwarded/transformed lazily as the
/// upstream produces them, so the client sees tokens arrive in real time.
/// Token usage is scanned from the stream and written to the log at
/// end-of-stream (the streaming usage arrives on a dedicated frame).
///
/// `do_log` marks which pass owns the request's end-of-stream bookkeeping: the
/// log row AND the gateway-key quota debit. Only one pass may do it, otherwise a
/// Responses call (relay + converter) would debit twice.
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
    gw_key_id: Option<String>,
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
    sec: SecurityOutcome,
    findings: Vec<SecurityFinding>,
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

        // 流式响应增量审计上下文：与 gate::scan_response 同口径
        // （security_enabled + security_scan_response 同时开启才扫描）。
        // 每块转发前扫其文本，命中累积，流末与请求侧发现合并、重算风险汇总写主行。
        // 仅记录不阻断（流式内容已实时发往客户端）。do_log=false 的中继路径
        // （Responses 模式）不在此审计，避免与 build_responses_stream_response 重复。
        // 单请求内设置/规则缓存镜像（来自 AppState.settings_cache，无 DB 读）。
        let app_settings = state
            .settings_cache
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| AppSettings::conservative_default());
        // 沿用原 (settings, builtin, custom) 元组形态，下游 scan_text_chunk 调用不变。
        // 缓存读取失败已降级为 conservative_default，故恒为 Some（不会失败）。
        let scan_ctx = Some((
            app_settings.security.settings.clone(),
            app_settings.security.builtin_rules.clone(),
            app_settings.security.custom_rules.clone(),
        ));
        let do_stream_audit = do_log
            && app_settings.security.settings.enabled
            && app_settings.security.settings.scan_response;
        let mut stream_findings: Vec<SecurityFinding> = Vec::new();

        // 响应体日志脱敏开关（与请求体同一开关 security_redact_secrets）。
        let sec_redact = app_settings.security.settings.redact_secrets;

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
                        if do_stream_audit {
                            if let Some(ctx) = &scan_ctx {
                                for sf in security::scanner::scan_text_chunk(
                                    s,
                                    &ctx.0,
                                    &ctx.1,
                                    &ctx.2,
                                ) {
                                    stream_findings.push(sf);
                                }
                            }
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
                            if do_stream_audit {
                                if let Some(ctx) = &scan_ctx {
                                    for sf in security::scanner::scan_text_chunk(
                                        &f,
                                        &ctx.0,
                                        &ctx.1,
                                        &ctx.2,
                                    ) {
                                        stream_findings.push(sf);
                                    }
                                }
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
                            if do_stream_audit {
                                if let Some(ctx) = &scan_ctx {
                                    for sf in security::scanner::scan_text_chunk(
                                        &f,
                                        &ctx.0,
                                        &ctx.1,
                                        &ctx.2,
                                    ) {
                                        stream_findings.push(sf);
                                    }
                                }
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
                            if do_stream_audit {
                                if let Some(ctx) = &scan_ctx {
                                    for sf in security::scanner::scan_text_chunk(
                                        &f,
                                        &ctx.0,
                                        &ctx.1,
                                        &ctx.2,
                                    ) {
                                        stream_findings.push(sf);
                                    }
                                }
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
                            if do_stream_audit {
                                if let Some(ctx) = &scan_ctx {
                                    for sf in security::scanner::scan_text_chunk(
                                        &f,
                                        &ctx.0,
                                        &ctx.1,
                                        &ctx.2,
                                    ) {
                                        stream_findings.push(sf);
                                    }
                                }
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
            // Debit the gateway key quota. Unlike the non-streaming path (which
            // only debits an already-successful response), a stream can break
            // mid-flight after upstream has generated billable tokens — so we
            // debit whatever the stream actually reported, even when
            // `had_error` is set. A break before any usage frame leaves
            // `total_tokens` at 0, so nothing is debited in that case.
            if acc.total_tokens > 0 {
                if let Some(ref key_id) = gw_key_id {
                    // 配额超限自动禁用由 add_quota_used 的 SQL 层完成。
                    let _ = gateway_keys::add_quota_used(&state.db, key_id, acc.total_tokens).await;
                }
            }

            // 流式增量审计：把响应侧逐块发现并入请求侧发现，重算风险汇总（与主行一致）。
            // 仅当确实有响应侧命中时才覆盖，否则沿用请求侧结论。
            let (sec_final, findings_final) = if do_stream_audit && !stream_findings.is_empty() {
                let mut all = findings;
                all.append(&mut stream_findings);
                let (rl, rs, summ, _t) = security::compute_risk_metrics(&all);
                let mut s = sec;
                s.risk_level = rl;
                s.risk_score = rs;
                s.risk_summary = summ;
                (s, all)
            } else {
                (sec, findings)
            };

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
                    let s = if sec_redact {
                        redact::redact_text(&response_body_acc)
                    } else {
                        response_body_acc
                    };
                    Some(s)
                } else {
                    None
                },
                mode,
                sec_final,
                findings_final,
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
    gw_key_id: Option<String>,
    channel_name: String,
    model: String,
    upstream_model: String,
    start: std::time::Instant,
    raw_request: Option<String>,
    log_raw_body: bool,
    is_retry: bool,
    mode: &'static str,
    sec: SecurityOutcome,
    findings: Vec<SecurityFinding>,
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

        // 流式响应增量审计上下文：与 gate::scan_response 同口径。
        // Responses 模式由 build_stream_response 中继（do_log=false）转交此处，
        // 故审计只在此处做一次，避免与 Chat 中继路径重复扫描。
        // 单请求内设置/规则缓存镜像（来自 AppState.settings_cache，无 DB 读）。
        let app_settings = state
            .settings_cache
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| AppSettings::conservative_default());
        // 沿用原 (settings, builtin, custom) 元组形态，下游 scan_text_chunk 调用不变。
        // 缓存读取失败已降级为 conservative_default，故恒为 Some（不会失败）。
        let scan_ctx = Some((
            app_settings.security.settings.clone(),
            app_settings.security.builtin_rules.clone(),
            app_settings.security.custom_rules.clone(),
        ));
        let do_stream_audit = app_settings.security.settings.enabled
            && app_settings.security.settings.scan_response;
        let mut stream_findings: Vec<SecurityFinding> = Vec::new();

        // 响应体日志脱敏开关（与请求体同一开关 security_redact_secrets）。
        let sec_redact = app_settings.security.settings.redact_secrets;

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
            if do_stream_audit {
                if let Some(ctx) = &scan_ctx {
                    for sf in security::scanner::scan_text_chunk(
                        &text,
                        &ctx.0,
                        &ctx.1,
                        &ctx.2,
                    ) {
                        stream_findings.push(sf);
                    }
                }
            }
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

        // End-of-stream: debit the gateway key quota, then write the request log
        // with the full Response-shaped body. The relay pass in
        // `build_stream_response` runs with do_log=false, so this is the single
        // place the quota is charged for a Responses call.
        let duration_ms = start.elapsed().as_millis() as i64;
        if acc.total_tokens > 0 {
            if let Some(ref key_id) = gw_key_id {
                let _ = gateway_keys::add_quota_used(&state.db, key_id, acc.total_tokens).await;
            }
        }

        // 流式增量审计：把响应侧逐块发现并入请求侧发现，重算风险汇总（与主行一致）。
        let (sec_final, findings_final) = if do_stream_audit && !stream_findings.is_empty() {
            let mut all = findings;
            all.append(&mut stream_findings);
            let (rl, rs, summ, _t) = security::compute_risk_metrics(&all);
            let mut s = sec;
            s.risk_level = rl;
            s.risk_score = rs;
            s.risk_summary = summ;
            (s, all)
        } else {
            (sec, findings)
        };

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
                let s = if sec_redact {
                    redact::redact_text(&response_body_acc)
                } else {
                    response_body_acc
                };
                Some(s)
            } else {
                None
            },
            mode,
            sec_final,
            findings_final,
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

/// Build the Anthropic Messages SSE `Response` for `/v1/messages` streaming.
///
/// The upstream (normalized to OpenAI Chat SSE by the shared pipeline) is fed
/// through a converter (`crate::messages_stream`) that turns each Chat
/// `chat.completion.chunk` frame into the Anthropic Messages event stream
/// (message_start -> content_block_start -> content_block_delta ->
/// content_block_stop -> message_delta -> message_stop). Like
/// `build_responses_stream_response`, the conversion runs on a spawned task and
/// frames are pushed through an `mpsc` channel; this pass owns the request log
/// and the gateway-key quota debit (the relay pass in `build_stream_response`
/// runs with `do_log=false`).
fn build_messages_stream_response(
    body: Body,
    state: Arc<AppState>,
    gw_key_name: String,
    gw_key_id: Option<String>,
    channel_name: String,
    model: String,
    upstream_model: String,
    start: std::time::Instant,
    raw_request: Option<String>,
    log_raw_body: bool,
    is_retry: bool,
    mode: &'static str,
    sec: SecurityOutcome,
    findings: Vec<SecurityFinding>,
) -> Response {
    use futures_util::StreamExt;
    use tokio::sync::mpsc;

    let message_id = format!("msg_{}", uuid::Uuid::new_v4().simple());
    let (tx, rx) = mpsc::channel::<Result<Bytes, std::io::Error>>(64);
    // Accumulate the Anthropic-shaped events so the request log can record the
    // full response body (only when `log_raw_body` is enabled).
    let mut response_body_acc = String::new();

    tokio::spawn(async move {
        let mut data_stream = body.into_data_stream();
        let mut st = crate::messages_stream::AnthropicStreamState::new(model.clone(), message_id);
        let mut acc = crate::adapter::StreamUsage::default();
        let mut had_error = false;

        // 流式响应增量审计上下文（与 gate::scan_response 同口径）。
        // 单请求内设置/规则缓存镜像（来自 AppState.settings_cache，无 DB 读）。
        let app_settings = state
            .settings_cache
            .read()
            .map(|g| g.clone())
            .unwrap_or_else(|_| AppSettings::conservative_default());
        // 沿用原 (settings, builtin, custom) 元组形态，下游 scan_text_chunk 调用不变。
        // 缓存读取失败已降级为 conservative_default，故恒为 Some（不会失败）。
        let scan_ctx = Some((
            app_settings.security.settings.clone(),
            app_settings.security.builtin_rules.clone(),
            app_settings.security.custom_rules.clone(),
        ));
        let do_stream_audit = app_settings.security.settings.enabled
            && app_settings.security.settings.scan_response;
        let mut stream_findings: Vec<SecurityFinding> = Vec::new();

        // 响应体日志脱敏开关（与请求体同一开关 security_redact_secrets）。
        let sec_redact = app_settings.security.settings.redact_secrets;

        while let Some(chunk) = data_stream.next().await {
            let chunk = match chunk {
                Ok(c) => c,
                Err(e) => {
                    had_error = true;
                    let err_frame = format!(
                        "event: error\ndata: {}\n\n",
                        json!({ "type": "error", "error": { "type": "api_error", "message": e.to_string() } })
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
            if do_stream_audit {
                if let Some(ctx) = &scan_ctx {
                    for sf in security::scanner::scan_text_chunk(
                        &text,
                        &ctx.0,
                        &ctx.1,
                        &ctx.2,
                    ) {
                        stream_findings.push(sf);
                    }
                }
            }
            // Parse each frame (separated by blank lines). A frame may carry
            // `event:` + `data:` (Anthropic) or just `data:` (Chat / Responses);
            // in both cases the JSON we care about lives on the `data:` line.
            // Skip `[DONE]`, which triggers finalize instead.
            for frame in text.split("\n\n") {
                let data_line = frame.lines().find(|l| l.starts_with("data:"));
                let Some(data_line) = data_line else {
                    continue;
                };
                let data_str = data_line.trim_start_matches("data:").trim();
                if data_str.is_empty() || data_str == "[DONE]" {
                    continue;
                }
                if let Ok(j) = serde_json::from_str::<serde_json::Value>(data_str) {
                    let mut out = String::new();
                    st.on_chat_chunk(&j, &mut out);
                    if !out.is_empty() {
                        if log_raw_body {
                            response_body_acc.push_str(&out);
                        }
                        if do_stream_audit {
                            if let Some(ctx) = &scan_ctx {
                                for sf in security::scanner::scan_text_chunk(
                                    &out,
                                    &ctx.0,
                                    &ctx.1,
                                    &ctx.2,
                                ) {
                                    stream_findings.push(sf);
                                }
                            }
                        }
                        let _ = tx.send(Ok::<_, std::io::Error>(Bytes::from(out))).await;
                    }
                }
            }
        }

        // Finalize: close any open block + message_delta + message_stop.
        if !had_error {
            let mut closing = String::new();
            st.finalize(&acc, &mut closing);
            if log_raw_body {
                response_body_acc.push_str(&closing);
            }
            let _ = tx
                .send(Ok::<_, std::io::Error>(Bytes::from(closing)))
                .await;
        }

        // End-of-stream: debit the gateway key quota, then write the request log
        // with the full Anthropic-shaped body. The relay pass in
        // `build_stream_response` runs with do_log=false, so this is the single
        // place the quota is charged for a Messages call.
        let duration_ms = start.elapsed().as_millis() as i64;
        if acc.total_tokens > 0 {
            if let Some(ref key_id) = gw_key_id {
                // 配额超限自动禁用由 add_quota_used 的 SQL 层完成。
                let _ = gateway_keys::add_quota_used(&state.db, key_id, acc.total_tokens).await;
            }
        }

        // 流式增量审计：把响应侧逐块发现并入请求侧发现，重算风险汇总（与主行一致）。
        let (sec_final, findings_final) = if do_stream_audit && !stream_findings.is_empty() {
            let mut all = findings;
            all.append(&mut stream_findings);
            let (rl, rs, summ, _t) = security::compute_risk_metrics(&all);
            let mut s = sec;
            s.risk_level = rl;
            s.risk_score = rs;
            s.risk_summary = summ;
            (s, all)
        } else {
            (sec, findings)
        };

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
                let s = if sec_redact {
                    redact::redact_text(&response_body_acc)
                } else {
                    response_body_acc
                };
                Some(s)
            } else {
                None
            },
            mode,
            sec_final,
            findings_final,
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
