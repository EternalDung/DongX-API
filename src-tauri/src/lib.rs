// `async_stream::stream!` in server/handler.rs expands into a deep macro
// recursion; raise the limit so it compiles (default 128 is too low).
#![recursion_limit = "1024"]
// 数据面 handler / rag / wiki 等函数参数较多（见 P1-1 重构计划），
// 暂时对 clippy 的 too_many_arguments 放宽，避免阻塞 CI。
#![allow(clippy::too_many_arguments)]

mod adapter;
mod commands;
mod core;
mod crypto;
mod db;
mod error;
mod mcp;
mod models;
mod protocol;
mod rag;
mod security;
mod server;
mod services;
mod settings;
mod tray;
mod wiki;

use std::sync::{Arc, Mutex, RwLock};
use tauri::Manager;

use crate::settings::Settings;

/// Application shared state (shared between Axum data plane and Tauri management plane)
///
/// SqlitePool is internally an Arc'd pool handle: clone() shares the same pool,
/// so no Mutex is needed (unlike Java where you'd wrap a DataSource in a singleton).
pub struct AppState {
    pub db: sqlx::SqlitePool,
    /// 网关服务运行态句柄：查询/停止/重启数据面服务都通过它。
    /// MCP 服务挂载在网关路由下（`services::mcp`），与网关同源、共用此句柄。
    pub server: server::ServerHandle,
    /// 关闭到托盘开关的运行态镜像（由设置页保存时更新，供窗口关闭钩子同步读取）。
    pub close_to_tray: std::sync::Arc<std::sync::Mutex<bool>>,
    /// 请求限流器运行态：按网关密钥滑动窗口限速，设置变更时整体重建。
    pub rate_limiter: Arc<Mutex<security::rate_limit::RateLimiterState>>,
    /// 设置/规则缓存：启动与设置变更时重建，数据面热路径只读一次本地镜像，
    /// 消除每条请求 20+ 次 settings/rules 重复读库。
    pub settings_cache: Arc<RwLock<Settings>>,
}

impl AppState {
    /// 重建 settings/rules 缓存。
    ///
    /// 数据面热路径读的是这份镜像，因此**任何**影响以下内容的写操作都必须调用它，
    /// 否则数据面会一直读到旧值：
    /// - settings：`log_raw_body` / `retry_enabled` / `retry_times` / `security_*`
    /// - 规则：内置规则启用与严重度、自定义规则增删改
    ///
    /// 失败只告警不中断：缓存保持旧值，下次变更时重试（不会让设置保存失败）。
    pub async fn reload_settings_cache(&self) {
        match Settings::load(&self.db).await {
            Ok(next) => match self.settings_cache.write() {
                Ok(mut g) => *g = next,
                Err(_) => tracing::warn!("settings_cache 写锁中毒，缓存未刷新（下次变更时重试）"),
            },
            Err(e) => tracing::warn!("设置缓存重建失败，暂用旧值: {}", e),
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
            commands::channel::set_channel_status,
            commands::channel::test_channel,
            commands::channel::list_provider_presets,
            commands::channel::list_provider_models,
            commands::channel::get_channel_stats,
            commands::key::list_api_keys,
            commands::key::create_api_key,
            commands::key::update_api_key,
            commands::key::delete_api_key,
            commands::key::set_api_key_status,
            commands::key::get_api_key_stats,
            commands::log::list_logs,
            commands::log::get_log_detail,
            commands::log::get_log_security_findings,
            commands::log::clear_logs,
            commands::log::delete_log,
            commands::log::get_model_stats,
            commands::mcp::get_mcp_status,
            commands::security::list_custom_rules,
            commands::security::create_custom_rule,
            commands::security::update_custom_rule,
            commands::security::delete_custom_rule,
            commands::security::list_builtin_rules,
            commands::security::update_builtin_rule,
            commands::security::reset_builtin_rules,
            commands::services::list_services,
            commands::rag::list_knowledge_bases,
            commands::rag::create_knowledge_base,
            commands::rag::update_knowledge_base,
            commands::rag::delete_knowledge_base,
            commands::rag::ingest_kb_text,
            commands::rag::ask_kb,
            commands::rag::list_documents,
            commands::rag::list_document_chunks,
            commands::rag::delete_document,
            commands::rag::import_source,
            commands::rag::list_sources,
            commands::rag::delete_source,
            commands::rag::retrieve_kb,
            commands::rag::get_index_status,
            commands::rag::reindex_kb,
            commands::settings::get_settings,
            commands::settings::update_settings,
            commands::settings::get_dashboard_stats,
            commands::server::get_server_status,
            commands::server::start_gateway_server,
            commands::server::stop_gateway_server,
            commands::server::restart_gateway_server,
            commands::client_config::get_client_configs,
            commands::client_config::apply_client_config,
            commands::client_config::restore_client_config,
            commands::client_config::get_client_config_content,
            // Wiki 知识库模块
            commands::wiki::list_wiki_projects,
            commands::wiki::create_wiki_project,
            commands::wiki::update_wiki_project,
            commands::wiki::delete_wiki_project,
            commands::wiki::list_wiki_pages,
            commands::wiki::list_wiki_sources,
            commands::wiki::add_wiki_source,
            commands::wiki::delete_wiki_source,
            commands::wiki::ingest_wiki_source,
            commands::wiki::ask_wiki,
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
            let pool = tauri::async_runtime::block_on(db::init_pool(&db_path)).map_err(|e| {
                eprintln!("Failed to initialize database: {}", e);
                e
            })?;

            // 启动即把 DEFAULTS（commands/settings.rs）声明的所有设置键回填进库，
            // 使 DEFAULTS 成为「存在哪些设置」的唯一真源。INSERT OR IGNORE 不覆盖
            // 已有值，故不改变任何生效行为；失败仅告警、不阻断启动（fail-open）。
            if let Err(e) =
                tauri::async_runtime::block_on(commands::settings::ensure_default_settings(&pool))
            {
                tracing::warn!("回填默认设置失败（已忽略，沿用库内现有值）: {}", e);
            }

            // 初始化服务注册表（加载服务启用/禁用/移除状态），须先于 Axum 启动，
            // 使 `ServiceRegistry::global()` 在路由合并时可用。
            crate::services::ServiceRegistry::init(&pool);

            // AppState managed here; commands access it via
            // State<'_, Arc<AppState>> and clone the pool handle freely.

            // 启动即应用「开机自启动」与「关闭到托盘」设置。
            let startup_settings =
                tauri::async_runtime::block_on(commands::settings::load_all_settings(&pool))
                    .unwrap_or_default();
            if let Some(v) = startup_settings.get("auto_start").and_then(|v| v.as_bool()) {
                commands::settings::apply_autostart(app.handle(), v);
            }
            let close_to_tray = startup_settings
                .get("close_to_tray")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);

            // 初始化请求限流器（启动即按设置构建；运行期改 RPM/开关由 update_settings 重建）。
            let rl_enabled = startup_settings
                .get("enable_rate_limit")
                .and_then(|v| v.as_bool())
                .unwrap_or(false);
            let rl_rpm = startup_settings
                .get("rate_limit_rpm")
                .and_then(|v| v.as_i64())
                .unwrap_or(60)
                .max(1) as u32;
            let rate_limiter = Arc::new(Mutex::new(
                crate::security::rate_limit::RateLimiterState::new(rl_enabled, rl_rpm),
            ));

            // 初始化设置/规则缓存（启动即加载；运行期改设置由 update_settings 重建）。
            let settings_cache = Arc::new(RwLock::new(
                tauri::async_runtime::block_on(Settings::load(&pool)).unwrap_or_else(|e| {
                    tracing::warn!("设置缓存加载失败，使用保守默认: {}", e);
                    Settings::conservative_default()
                }),
            ));

            let app_state = Arc::new(AppState {
                db: pool.clone(),
                server: server::ServerHandle::new(),
                close_to_tray: std::sync::Arc::new(std::sync::Mutex::new(close_to_tray)),
                rate_limiter,
                settings_cache,
            });
            app.manage(app_state);

            // Spawn Axum HTTP server (data plane) in background
            let handle = app.handle().clone();
            tauri::async_runtime::spawn(async move {
                if let Err(e) = server::start_server(handle).await {
                    tracing::error!("Axum server error: {}", e);
                }
            });

            // MCP 无独立启动流程：其路由由 services::mcp::McpService 注册进
            // 网关路由表（见 server::router::create_router），端点 = 网关地址 + /mcp。
            // 因此改网关端口 → 重启网关 → MCP 端点自动跟随，无需额外联动。

            // 后台日志保留清理：按 log_retention_days 定期清理过期日志
            // （含关联的 request_security_findings），避免日志无限增长。
            commands::log::spawn_log_retention_sweeper(pool.clone());

            // 系统托盘 + 窗口关闭钩子（最小化/关闭到托盘的前置条件）
            tray::create(app)?;

            Ok(())
        })
        .run(tauri::generate_context!())
        .expect("error while running DongX");
}
