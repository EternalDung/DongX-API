pub mod router;
pub mod handler;
pub mod auth;

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde::Serialize;
use sqlx::SqlitePool;
use tauri::AppHandle;
use tauri::Manager;
use tokio::sync::{oneshot, Notify};

use crate::db::repository::settings as settings_repo;
use crate::error::{AppError, AppResult};
use crate::AppState;

/// 监听地址默认值：settings 里没有记录（或记录非法）时的兜底。
pub const DEFAULT_HOST: &str = "127.0.0.1";
pub const DEFAULT_PORT: u16 = 9842;

/// 服务当前实际监听的地址。
///
/// 这是「运行态」数据——服务真正 bind 成功后才写入，与 settings 里用户
/// 填的「配置态」值区分开（后者改了要重启才生效）。侧边栏和设置页展示的
/// 都应该是这份运行态值。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ListenAddr {
    pub host: String,
    pub port: u16,
}

impl ListenAddr {
    pub fn url(&self) -> String {
        format!("http://{}:{}/v1", self.host, self.port)
    }
}

/// 一个正在运行的服务实例：持有 graceful shutdown 的触发端。
struct RunningServer {
    shutdown: oneshot::Sender<()>,
    addr: ListenAddr,
}

/// 网关服务的运行态句柄（跨命令共享）。
///
/// 用 `Mutex<Option<..>>` 而非原子量，是因为要同时维护「是否运行」和
/// 「shutdown 触发端」两个值的一致性。锁只在同步块内短暂持有，不跨 await。
pub struct ServerHandle {
    inner: Mutex<Option<RunningServer>>,
    /// 服务每次退出后 notify，供 restart 等待旧实例真正让出端口。
    stopped: Notify,
}

impl ServerHandle {
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(None),
            stopped: Notify::new(),
        }
    }

    /// 登记一个即将启动的服务，返回它专用的 shutdown 信号接收端。
    fn register(&self, addr: ListenAddr) -> oneshot::Receiver<()> {
        let (tx, rx) = oneshot::channel();
        // 理论上不该发生（restart 前会先停旧的）；真出现就覆盖旧句柄，
        // 旧实例会因为没有 receiver 而继续跑，由 mark_stopped 兜底清理。
        *self.inner.lock().unwrap() = Some(RunningServer { shutdown: tx, addr });
        rx
    }

    /// 当前实际监听的地址；服务未运行时返回 None。
    pub fn current(&self) -> Option<ListenAddr> {
        self.inner.lock().unwrap().as_ref().map(|s| s.addr.clone())
    }

    pub fn is_running(&self) -> bool {
        self.inner.lock().unwrap().is_some()
    }

    /// 请求停止当前服务。返回是否确实停了一个运行中的实例。
    pub fn request_stop(&self) -> bool {
        let taken = self.inner.lock().unwrap().take();
        match taken {
            Some(s) => {
                // send 失败说明任务已结束，receiver 被 drop，属正常情况。
                let _ = s.shutdown.send(());
                true
            }
            None => false,
        }
    }

    /// 由服务任务在退出时调用：清空运行态并唤醒等待者。
    fn mark_stopped(&self) {
        self.inner.lock().unwrap().take();
        self.stopped.notify_waiters();
    }

    /// 等待服务退出（restart 用），带超时避免长连接把流程卡死。
    pub async fn wait_stopped(&self, timeout: Duration) {
        let _ = tokio::time::timeout(timeout, self.stopped.notified()).await;
    }
}

/// 从 settings 解析监听地址（配置态，不是运行态）。
///
/// 值非法或缺失时回退到默认值，保证服务永远能起来。
pub async fn resolve_bind_addr(pool: &SqlitePool) -> ListenAddr {
    let host = settings_repo::get(pool, "server_host")
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str::<String>(&v).ok())
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .unwrap_or_else(|| DEFAULT_HOST.to_string());

    let port = settings_repo::get(pool, "server_port")
        .await
        .ok()
        .flatten()
        .and_then(|v| serde_json::from_str::<u16>(&v).ok())
        .filter(|p| *p > 0)
        .unwrap_or(DEFAULT_PORT);

    ListenAddr { host, port }
}

/// Start the Axum HTTP server (data plane).
///
/// 监听地址来自 settings（`server_host` / `server_port`），改完端口需重启
/// 服务（或应用）才会生效。返回 Err 表示 bind 失败（端口被占用/无权限）。
pub async fn start_server(app: AppHandle) -> AppResult<()> {
    let state = app.state::<Arc<AppState>>().inner().clone();

    let addr = resolve_bind_addr(&state.db).await;
    let bind = format!("{}:{}", addr.host, addr.port);

    // Data-plane handlers access the shared pool via AppHandle::state (Tauri
    // managed state), so the router itself carries no Axum state for now.
    let router = router::create_router(app.clone());

    let listener = tokio::net::TcpListener::bind(&bind)
        .await
        .map_err(|e| AppError::Internal(format!("监听 {} 失败：{}", bind, e)))?;

    let rx = state.server.register(addr.clone());
    tracing::info!("DongX gateway server listening on http://{}", bind);

    let result = axum::serve(listener, router)
        .with_graceful_shutdown(async move {
            let _ = rx.await;
        })
        .await;

    state.server.mark_stopped();
    tracing::info!("DongX gateway server stopped");

    result.map_err(|e| AppError::Internal(format!("Server error: {}", e)))
}

/// 停止服务并等待其真正退出（端口让出）。
pub async fn stop_server(app: &AppHandle) {
    let state = app.state::<Arc<AppState>>().inner().clone();

    if state.server.request_stop() {
        state.server.wait_stopped(Duration::from_secs(5)).await;
        // 给操作系统一点时间回收监听端口，避免立刻重绑同一端口失败。
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}
