use std::sync::Arc;
use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::server::{resolve_bind_addr, ListenAddr};
use crate::AppState;

/// 网关服务状态快照（管理面查询用）。
///
/// 同时给出「运行态」（running/host/port）与「配置态」（configured*），
/// 前端据此判断是否需要重启服务 —— 两者不一致时 `restart_required` 为 true。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerStatus {
    /// 服务当前是否在运行
    pub running: bool,
    /// 实际监听地址（未运行时为 null，前端应展示「未运行」）
    pub host: Option<String>,
    pub port: Option<u16>,
    /// 完整端点 URL（未运行时为 null）
    pub url: Option<String>,
    /// settings 里配置的值（重启服务后才会成为运行态值）
    pub configured_host: String,
    pub configured_port: u16,
    /// 配置与运行态不一致 → 需要重启服务才生效
    pub restart_required: bool,
}

/// 组装状态快照：运行态来自 ServerHandle，配置态来自 settings。
async fn snapshot(state: &Arc<AppState>) -> ServerStatus {
    let listening: Option<ListenAddr> = state.server.current();
    let configured = resolve_bind_addr(&state.db).await;

    let restart_required = match &listening {
        Some(l) => l.host != configured.host || l.port != configured.port,
        // 没在跑：配置存在就该起来，也算「待应用」
        None => true,
    };

    ServerStatus {
        running: listening.is_some(),
        host: listening.as_ref().map(|l| l.host.clone()),
        port: listening.as_ref().map(|l| l.port),
        url: listening.as_ref().map(|l| l.url()),
        configured_host: configured.host,
        configured_port: configured.port,
        restart_required,
    }
}

/// 查询网关服务状态（侧边栏、设置页共用）。
#[tauri::command]
pub async fn get_server_status(state: State<'_, Arc<AppState>>) -> AppResult<ServerStatus> {
    let state = state.inner().clone();
    Ok(snapshot(&state).await)
}

/// 停止网关服务（运行中的流式连接会被 graceful shutdown 等待结束）。
#[tauri::command]
pub async fn stop_gateway_server(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> AppResult<ServerStatus> {
    let state = state.inner().clone();
    crate::server::stop_server(&app).await;
    Ok(snapshot(&state).await)
}

/// 用 settings 里的最新配置重启网关服务。
///
/// 流程：停旧实例（等它退出并让出端口）→ 按新配置重新 bind → 等启动确认。
/// bind 失败（端口被占用等）会返回 Err，此时服务处于「已停止」状态。
#[tauri::command]
pub async fn restart_gateway_server(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> AppResult<ServerStatus> {
    let state = state.inner().clone();

    crate::server::stop_server(&app).await;

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::server::start_server(handle).await {
            tracing::error!("Failed to start gateway server: {}", e);
        }
    });

    // 等待新实例 bind 成功（或失败退出），最长 3s。
    // 不轮询的话调用方无法区分「启动中」和「启动失败」。
    let mut started = false;
    for _ in 0..30 {
        if state.server.is_running() {
            started = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let status = snapshot(&state).await;
    if !started {
        return Err(AppError::Internal(format!(
            "服务未能启动，请检查端口 {} 是否被占用",
            status.configured_port
        )));
    }
    Ok(status)
}

/// 启动网关服务（当前未运行时使用）。
#[tauri::command]
pub async fn start_gateway_server(
    app: AppHandle,
    state: State<'_, Arc<AppState>>,
) -> AppResult<ServerStatus> {
    let state = state.inner().clone();

    if state.server.is_running() {
        return Ok(snapshot(&state).await);
    }

    let handle = app.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) = crate::server::start_server(handle).await {
            tracing::error!("Failed to start gateway server: {}", e);
        }
    });

    let mut started = false;
    for _ in 0..30 {
        if state.server.is_running() {
            started = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(100)).await;
    }

    let status = snapshot(&state).await;
    if !started {
        return Err(AppError::Internal(format!(
            "服务未能启动，请检查端口 {} 是否被占用",
            status.configured_port
        )));
    }
    Ok(status)
}
