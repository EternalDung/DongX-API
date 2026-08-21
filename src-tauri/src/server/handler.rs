use axum::extract::State;
use axum::response::{IntoResponse, Response};
use axum::Json;
use serde_json::{json, Value};
use tauri::AppHandle;

/// Health check endpoint
pub async fn health() -> impl IntoResponse {
    Json(json!({ "status": "ok", "service": "DongX Gateway" }))
}

/// POST /v1/chat/completions
/// OpenAI-compatible chat completions endpoint with SSE streaming support
pub async fn chat_completions(
    State(app): State<AppHandle>,
    headers: axum::http::HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // TODO: Implement full pipeline:
    // 1. Auth: verify gateway key from Authorization header
    // 2. Parse request body as OpenAI chat completion request
    // 3. Select channel via dispatcher (model + priority + weight)
    // 4. Adapt request format via adapter (OpenAI -> upstream protocol)
    // 5. Proxy request via reqwest (with SSE streaming)
    // 6. Stream response back to client
    // 7. Log request asynchronously

    let _ = (app, headers, body);

    // Placeholder response
    (
        axum::http::StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": "Chat completions endpoint not yet implemented",
                "code": "not_implemented"
            }
        })),
    )
        .into_response()
}

/// POST /v1/completions
pub async fn completions(
    State(app): State<AppHandle>,
    body: axum::body::Bytes,
) -> Response {
    let _ = (app, body);
    (
        axum::http::StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": "Completions endpoint not yet implemented",
                "code": "not_implemented"
            }
        })),
    )
        .into_response()
}

/// POST /v1/embeddings
pub async fn embeddings(
    State(app): State<AppHandle>,
    body: axum::body::Bytes,
) -> Response {
    let _ = (app, body);
    (
        axum::http::StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": {
                "message": "Embeddings endpoint not yet implemented",
                "code": "not_implemented"
            }
        })),
    )
        .into_response()
}

/// GET /v1/models
pub async fn list_models(
    State(app): State<AppHandle>,
) -> Response {
    let _ = app;
    // TODO: Return list of available models from channels
    Json(json!({
        "object": "list",
        "data": []
    }))
    .into_response()
}
