//! 服务注册框架：每个业务服务（RAG、未来可能有的 MCP / Wiki 等）自包含
//! 自己的路由、状态与启用开关，统一向 [`ServiceRegistry`] 注册。新增服务只需
//! 实现 [`Service`] trait 并在 [`ServiceRegistry::init`] 里 `register`。
//!
//! 服务注册框架适配 DongX 的 axum 状态类型
//! （DongX 用 [`tauri::AppHandle`] 作为 axum state）。

use std::collections::HashSet;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use axum::Router;
use serde::Serialize;
use sqlx::SqlitePool;
use tauri::async_runtime;
use tauri::AppHandle;

use crate::settings::{get_setting_raw, set_setting_raw};
use crate::AppState;

/// 所有业务服务统一实现的接口。
///
/// 路由定义、状态检查、启用/禁用开关都内聚在服务自身，而非散落在中心路由表里。
#[async_trait]
pub trait Service: Send + Sync {
    /// 服务唯一 id（URL/状态标识用）。
    fn id(&self) -> &'static str;
    /// 展示名。
    fn name(&self) -> &'static str;
    /// 描述。
    fn description(&self) -> &'static str;
    /// 是否启用：禁用则不挂载其路由、状态里 `enabled=false`。
    ///
    /// 默认始终启用；运行期禁用由 [`ServiceRegistry`] 的状态集合控制，
    /// 持久化到 `service.<id>.enabled` 设置键，无需改动注册代码。
    fn enabled(&self) -> bool {
        true
    }
    /// 服务运行状态（统计信息等）。
    async fn status(&self, state: &AppState) -> ServiceStatus;
    /// 服务的 axum 子路由。状态类型为 [`AppHandle`]，由最外层路由统一 `with_state`。
    fn routes(&self) -> Router<AppHandle>;
}

/// 单个服务的运行态快照（供 UI / 命令消费）。
#[derive(Debug, Serialize)]
pub struct ServiceStatus {
    pub id: String,
    pub name: String,
    pub description: String,
    pub enabled: bool,
    pub running: bool,
    pub stats: serde_json::Value,
}

/// 服务注册表：持有全部服务实例与运行期启用/删除状态，负责合并路由与汇总状态。
///
/// 采用全局单例（[`OnceLock`]）：`setup` 时 [`ServiceRegistry::init`] 创建并加载
/// 持久化状态；命令与 Axum 路由通过 [`ServiceRegistry::global`] 访问同一实例，
/// 因此 UI 的启用/禁用/删除操作对所有调用方即时生效。
pub struct ServiceRegistry {
    services: Vec<Box<dyn Service>>,
    /// 运行期被禁用的服务 id 集合（持久化到 `service.<id>.enabled=false`）。
    disabled: Mutex<HashSet<String>>,
    /// 运行期被移除（软删除）的服务 id 集合（持久化到 `service.<id>.removed=true`）。
    removed: Mutex<HashSet<String>>,
}

static GLOBAL: OnceLock<Arc<ServiceRegistry>> = OnceLock::new();

impl ServiceRegistry {
    /// 构造并注册所有已知服务（不含持久化状态）。
    fn new() -> Self {
        let mut registry = Self {
            services: vec![],
            disabled: Mutex::new(HashSet::new()),
            removed: Mutex::new(HashSet::new()),
        };
        registry.register(Box::new(knowledge::KnowledgeService));
        registry.register(Box::new(mcp::McpService));
        registry
    }

    /// 初始化全局单例：注册服务并从 settings 加载启用/删除状态。
    ///
    /// 必须在 `setup` 中、Axum server 启动前调用一次。重复调用视为无操作
    /// （保留首次创建的实例）。
    pub fn init(pool: &SqlitePool) -> Arc<Self> {
        let mut registry = Self::new();
        async_runtime::block_on(registry.load_state(pool));
        let arc = Arc::new(registry);
        match GLOBAL.set(arc.clone()) {
            Ok(()) => arc,
            Err(existing) => existing,
        }
    }

    /// 获取全局单例（命令 / 路由使用）。
    ///
    /// # Panics
    /// 若 [`ServiceRegistry::init`] 尚未在 `setup` 调用。
    pub fn global() -> Arc<Self> {
        GLOBAL
            .get()
            .expect("ServiceRegistry::init 未在 setup 中调用")
            .clone()
    }

    /// 注册一个服务实例。
    pub fn register(&mut self, service: Box<dyn Service>) {
        self.services.push(service);
    }

    /// 从 settings 加载每个服务的启用/删除持久化状态。
    async fn load_state(&mut self, pool: &SqlitePool) {
        for svc in &self.services {
            let id = svc.id();
            if let Some(v) = get_setting_raw(pool, &format!("service.{}.enabled", id)).await {
                if v == "false" {
                    self.disabled.lock().unwrap().insert(id.to_string());
                }
            }
            if let Some(v) = get_setting_raw(pool, &format!("service.{}.removed", id)).await {
                if v == "true" {
                    self.removed.lock().unwrap().insert(id.to_string());
                }
            }
        }
    }

    /// 服务当前是否应挂载路由 / 出现在列表（已注册 + 未禁用 + 未删除）。
    fn is_active(&self, id: &str) -> bool {
        let disabled = self.disabled.lock().unwrap();
        let removed = self.removed.lock().unwrap();
        self.services.iter().any(|s| s.id() == id)
            && !disabled.contains(id)
            && !removed.contains(id)
    }

    /// 把启用中的服务路由合并进传入的网关路由（状态类型均为 [`AppHandle`]，
    /// 由最外层路由统一 `with_state`）。
    pub fn merge_into(&self, mut router: Router<AppHandle>) -> Router<AppHandle> {
        for service in &self.services {
            if self.is_active(service.id()) {
                router = router.merge(service.routes());
            }
        }
        router
    }

    /// 汇总所有未删除服务的状态（`enabled` 反映运行期禁用状态）。
    pub async fn list_status(&self, state: &AppState) -> Vec<ServiceStatus> {
        let mut result = Vec::with_capacity(self.services.len());
        for service in &self.services {
            // 在 await 前取完禁用/删除状态并释放 MutexGuard，
            // 否则 MutexGuard（非 Send）跨 await 会让整个 future 不满足 Send 约束。
            let (removed, disabled) = {
                let removed = self.removed.lock().unwrap();
                let disabled = self.disabled.lock().unwrap();
                (
                    removed.contains(service.id()),
                    !disabled.contains(service.id()),
                )
            };
            if removed {
                continue;
            }
            let mut st = service.status(state).await;
            st.enabled = service.enabled() && disabled;
            result.push(st);
        }
        result
    }

    /// 启用 / 禁用服务（持久化到 settings，重启后仍生效）。
    #[allow(dead_code)]
    pub async fn set_enabled(
        &self,
        id: &str,
        enabled: bool,
        pool: &SqlitePool,
    ) -> Result<(), String> {
        if !self.services.iter().any(|s| s.id() == id) {
            return Err(format!("未知服务: {}", id));
        }
        {
            let mut disabled = self.disabled.lock().unwrap();
            if enabled {
                disabled.remove(id);
            } else {
                disabled.insert(id.to_string());
            }
        }
        set_setting_raw(
            pool,
            &format!("service.{}.enabled", id),
            &enabled.to_string(),
        )
        .await?;
        Ok(())
    }

    /// 移除（软删除）服务：从列表隐藏、不挂载路由（持久化，重启后仍隐藏）。
    ///
    /// 该操作不可撤销（需手动清除 `service.<id>.removed` 设置键恢复），因此
    /// 命令层应要求前端二次确认。
    #[allow(dead_code)]
    pub async fn remove(&self, id: &str, pool: &SqlitePool) -> Result<(), String> {
        if !self.services.iter().any(|s| s.id() == id) {
            return Err(format!("未知服务: {}", id));
        }
        {
            let mut removed = self.removed.lock().unwrap();
            removed.insert(id.to_string());
        }
        set_setting_raw(pool, &format!("service.{}.removed", id), "true").await?;
        Ok(())
    }
}

pub mod knowledge;
pub mod mcp;
