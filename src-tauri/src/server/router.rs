use super::handler;
use crate::services::ServiceRegistry;
use axum::Router;
use tauri::AppHandle;

pub fn create_router(app: AppHandle) -> Router {
    // 网关路由（不含服务路由）。状态类型为 `AppHandle`，由最外层统一 `with_state`。
    let gateway = Router::new()
        .route("/health", axum::routing::get(handler::health))
        .route(
            "/v1/chat/completions",
            axum::routing::post(handler::chat_completions),
        )
        .route("/v1/responses", axum::routing::post(handler::responses))
        .route("/v1/messages", axum::routing::post(handler::messages))
        .route("/v1/completions", axum::routing::post(handler::completions))
        .route("/v1/embeddings", axum::routing::post(handler::embeddings))
        .route("/v1/models", axum::routing::get(handler::list_models));

    // 合并所有已注册服务的路由（ServiceRegistry 内部按运行期状态过滤禁用/移除）。
    let router = ServiceRegistry::global()
        .merge_into(gateway)
        .layer(tower_http::cors::CorsLayer::permissive())
        .layer(tower_http::trace::TraceLayer::new_for_http());

    router.with_state(app)
}
