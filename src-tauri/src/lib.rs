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
pub struct AppState {
    pub db: tokio::sync::Mutex<Option<sqlx::SqlitePool>>,
}

impl AppState {
    pub fn new() -> Self {
        Self {
            db: tokio::sync::Mutex::new(None),
        }
    }
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
        .manage(Arc::new(AppState::new()))
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
