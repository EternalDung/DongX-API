// `async_stream::stream!` in server/handler.rs expands into a deep macro
// recursion; raise the limit so it compiles (default 128 is too low).
#![recursion_limit = "1024"]

mod adapter;
mod channel_presets;
mod responses_stream;
mod commands;
mod core;
mod crypto;
mod db;
mod error;
mod models;
mod security;
mod server;
mod tray;

use std::sync::Arc;
use tauri::Manager;

/// Application shared state (shared between Axum data plane and Tauri management plane)
///
/// SqlitePool is internally an Arc'd pool handle: clone() shares the same pool,
/// so no Mutex is needed (unlike Java where you'd wrap a DataSource in a singleton).
pub struct AppState {
    pub db: sqlx::SqlitePool,
    /// 网关服务运行态句柄：查询/停止/重启数据面服务都通过它。
    pub server: server::ServerHandle,
    /// 关闭到托盘开关的运行态镜像（由设置页保存时更新，供窗口关闭钩子同步读取）。
    pub close_to_tray: std::sync::Arc<std::sync::Mutex<bool>>,
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
        .on_window_event(|window, event| {
            // 关闭到托盘：拦截关闭请求，按设置决定是否隐藏而非退出。
            // 托盘菜单的「退出」用 app.exit(0) 强制退出，绕过此拦截。
            if let tauri::WindowEvent::CloseRequested { api, .. } = event {
                let should_tray = window
                    .state::<Arc<AppState>>()
                    .close_to_tray
                    .lock()
                    .map(|g| *g)
                    .unwrap_or(false);
                if should_tray {
                    api.prevent_close();
                    let _ = window.hide();
                }
            }
        })
        .invoke_handler(tauri::generate_handler![
            commands::channel::list_channels,
            commands::channel::create_channel,
            commands::channel::update_channel,
            commands::channel::delete_channel,
            commands::channel::test_channel,
            commands::channel::list_provider_presets,
            commands::channel::list_provider_models,
            commands::key::list_api_keys,
            commands::key::create_api_key,
            commands::key::update_api_key,
            commands::key::delete_api_key,
            commands::key::set_api_key_status,
            commands::log::list_logs,
            commands::log::get_log_detail,
            commands::log::clear_logs,
            commands::log::delete_log,
            commands::audit::list_audit_events,
            commands::security::list_custom_rules,
            commands::security::create_custom_rule,
            commands::security::update_custom_rule,
            commands::security::delete_custom_rule,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::get_dashboard_stats,
            commands::server::get_server_status,
            commands::server::start_gateway_server,
            commands::server::stop_gateway_server,
            commands::server::restart_gateway_server,
        ])
        .setup(|app| {
            // Initialize logging before anything else
            tracing_subscriber::fmt()
                .with_env_filter(
                    tracing_subscriber::EnvFilter::try_from_default_env()
                        .unwrap_or_else(|_| "dongx=info,tauri=info".into()),
                )
                .init();

            // Resolve db path: %APPDATA%/<tauri.conf.json identifier>/dongx.db
            // (identifier = "com.wei.dongx", so: %APPDATA%/com.wei.dongx/dongx.db)
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

            // 启动即应用「开机自启动」与「关闭到托盘」设置。
            let startup_settings = tauri::async_runtime::block_on(
                commands::settings::load_all_settings(&pool),
            )
            .unwrap_or_default();
            if let Some(v) = startup_settings
                .get("auto_start")
                .and_then(|v| v.as_bool())
            {
                commands::settings::apply_autostart(app.handle(), v);
            }
            let close_to_tray = startup_settings
                .get("close_to_tray")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            let app_state = Arc::new(AppState {
                db: pool.clone(),
                server: server::ServerHandle::new(),
                close_to_tray: std::sync::Arc::new(std::sync::Mutex::new(close_to_tray)),
            });
            app.manage(app_state);

            // Spawn Axum HTTP server (data plane) in background
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = server::start_server(handle).await {
                    tracing::error!("Axum server error: {}", e);
                }
            });

            // 系统托盘 + 窗口关闭钩子（最小化/关闭到托盘的前置条件）
            tray::create(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running DongX");
}
