use std::sync::Arc;

use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::json;
use tauri::{AppHandle, Manager};

use crate::adapter::{self, ChannelConfig, ProxyRequest};
use crate::core::dispatcher;
use crate::db::repository::{channels, gateway_keys, request_logs, settings};
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
///    protocol and proxies the request.
/// 5. Quota — debit the gateway key's used tokens.
/// 6. Log — async insert into `request_logs`.
/// 7. Return — pass the upstream body + status back to the client.
///
/// (MVP: non-streaming only. SSE streaming is a follow-up.)
pub async fn chat_completions(
    State(app): State<AppHandle>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
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

    // 3. Select channel
    let ctx = dispatcher::DispatchContext {
        model: model.clone(),
        api_key_id: gw_key.id.clone(),
        is_stream,
        request_body: body_json.clone(),
    };
    let selected = match dispatcher::select_channel(&state.db, &ctx).await {
        Ok(s) => s,
        Err(e) => {
            return error_response(
                StatusCode::SERVICE_UNAVAILABLE,
                "no_channel",
                &e.to_string(),
            )
        }
    };

    // 4. Build adapter config + request, then forward.
    let config = ChannelConfig {
        base_url: selected.base_url.clone(),
        api_key: selected.upstream_api_key.clone(),
        models: selected.models.clone(),
        model_mapping: selected.model_mapping.clone(),
        extra: selected.extra.clone(),
        timeout_secs: selected.timeout_secs,
    };
    let proxy_req = ProxyRequest {
        model: model.clone(),
        body: body_json,
        stream: is_stream,
    };
    // Apply model mapping for the log's `upstream_model` field.
    let upstream_model = adapter::map_model(&proxy_req, &config);
    let adaptor = adapter::get_adaptor(&selected.channel_type);

    // 5. Forward (non-streaming MVP).
    let forward_result = adaptor.forward(&proxy_req, &config).await;
    // Measure after forward so duration includes the upstream round-trip.
    let duration_ms = start.elapsed().as_millis() as i64;
    let (status, resp_body, usage) = match forward_result {
        Ok(r) => r,
        Err(e) => {
            let msg = e.to_string();
            spawn_log(
                state.clone(),
                Some(gw_key.name.clone()),
                Some(selected.name.clone()),
                model,
                Some(upstream_model),
                502,
                0,
                0,
                0,
                duration_ms,
                Some(msg.clone()),
                is_stream,
                raw_request.clone(),
                None,
            );
            return error_response(StatusCode::BAD_GATEWAY, "upstream_error", &msg);
        }
    };

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

    // 6. Debit gateway key quota.
    if tt > 0 {
        let _ = gateway_keys::add_quota_used(&state.db, &gw_key.id, tt).await;
    }

    // 7. Async log.
    let raw_response = if log_raw_body {
        serde_json::to_string(&resp_body).ok()
    } else {
        None
    };
    spawn_log(
        state.clone(),
        Some(gw_key.name.clone()),
        Some(selected.name.clone()),
        model,
        Some(upstream_model),
        status as i32,
        pt,
        ct,
        tt,
        duration_ms,
        None,
        is_stream,
        raw_request.clone(),
        raw_response,
    );

    // 8. Return upstream body + status.
    let st = StatusCode::from_u16(status).unwrap_or(StatusCode::OK);
    (st, Json(resp_body)).into_response()
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
    request_body: Option<String>,
    response_body: Option<String>,
) {
    tokio::spawn(async move {
        let _ = request_logs::insert(
            &state.db,
            api_key_name.as_deref(),
            channel_name.as_deref(),
            &model,
            upstream_model.as_deref(),
            "chat",
            status_code,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            duration_ms,
            error_message.as_deref(),
            is_stream,
            false,
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
