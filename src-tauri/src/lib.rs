mod adapter;
mod commands;
mod config;
mod core;
mod crypto;
mod db;
mod error;
mod models;
mod security;
mod server;

use std::sync::Arc;
use tauri::Manager;

/// Application shared state (shared between Axum data plane and Tauri management plane)
///
/// SqlitePool is internally an Arc'd pool handle: clone() shares the same pool,
/// so no Mutex is needed (unlike Java where you'd wrap a DataSource in a singleton).
pub struct AppState {
    pub db: sqlx::SqlitePool,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_store::Builder::default().build())
        .plugin(tauri_plugin_clipboard_manager::init())
        .plugin(tauri_plugin_notification::init())
        .plugin(tauri_plugin_autostart::init(
            tauri_plugin_autostart::MacosLauncher::LaunchAgent,
            None,
        ))
        .plugin(tauri_plugin_dialog::init())
        .invoke_handler(tauri::generate_handler![
            commands::channel::list_channels,
            commands::channel::create_channel,
            commands::channel::update_channel,
            commands::channel::delete_channel,
            commands::channel::test_channel,
            commands::channel::list_provider_presets,
            commands::key::list_api_keys,
            commands::key::create_api_key,
            commands::key::update_api_key,
            commands::key::delete_api_key,
            commands::log::list_logs,
            commands::log::get_log_detail,
            commands::log::clear_logs,
            commands::audit::list_audit_events,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::get_dashboard_stats,
        ])
        .setup(|app| {
            // Initialize logging before anything else
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "dongx=info,tauri=info".into()),
                )
                .init();

            // Resolve db path: %APPDATA%/com.dongx.app/dongx.db
            let data_dir = app.path().app_data_dir()?;
            let db_path = data_dir.join("dongx.db");

            // setup() is sync; block on async pool init before the UI opens.
            // (Equivalent to initializing the DataSource eagerly at Spring Boot startup)
            let pool = tauri::async_runtime::block_on(db::init_pool(&db_path))
                .map_err(|e| {
                    eprintln!("Failed to initialize database: {}", e);
                    e
                })?;

            // AppState managed here; commands access it via
            // State<'_, Arc<AppState>> and clone the pool handle freely.
            app.manage(Arc::new(AppState { db: pool.clone() }));

            // Spawn Axum HTTP server (data plane) in background
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = server::start_server(handle).await {
                    tracing::error!("Axum server error: {}", e);
                }
            });

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running DongX");
}
