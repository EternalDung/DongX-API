pub mod router;
pub mod handler;
pub mod auth;

use tauri::{AppHandle, Manager};
use crate::error::{AppError, AppResult};

/// Start the Axum HTTP server (data plane)
/// Listens on localhost:port (configurable via settings)
pub async fn start_server(app: AppHandle) -> AppResult<()> {
    // TODO: Read port from settings
    let port = 9842u16;
    let host = "127.0.0.1";

    let state = app.state::<std::sync::Arc<crate::AppState>>().clone();
    let router = router::create_router(app.clone());

    let _ = state; // Will be used as Axum state when fully wired

    let addr = format!("{}:{}", host, port);
    tracing::info!("DongX gateway server starting on http://{}", addr);

    let listener = tokio::net::TcpListener::bind(&addr)
        .await
        .map_err(|e| AppError::Internal(format!("Bind failed: {}", e)))?;

    axum::serve(listener, router)
        .await
        .map_err(|e| AppError::Internal(format!("Server error: {}", e)))?;

    Ok(())
}
