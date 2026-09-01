use axum::Router;
use tauri::AppHandle;
use super::handler;

pub fn create_router(app: AppHandle) -> Router {
    Router::new()
        .route("/health", axum::routing::get(handler::health))
        .route("/v1/chat/completions", axum::routing::post(handler::chat_completions))
        .route("/v1/responses", axum::routing::post(handler::responses))
        .route("/v1/messages", axum::routing::post(handler::messages))
        .route("/v1/completions", axum::routing::post(handler::completions))
        .route("/v1/embeddings", axum::routing::post(handler::embeddings))
        .route("/v1/models", axum::routing::get(handler::list_models))
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http())
        .with_state(app)
}
