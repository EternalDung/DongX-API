//! 单请求内重复读 settings/rules 的缓存。
//!
//! 数据面每条请求原本要读 20+ 次 settings KV（log_raw_body / retry_* /
//! security_*）与安全规则（启用内置规则 + 启用自定义规则）。这些值在进程运行期
//! 几乎不变，仅在 `update_settings` 时变更。因此把它们缓存到 `AppState`，每条
//! 请求只读一次本地镜像（RwLock 读锁 + clone），消除热路径上的重复读库。

use sqlx::SqlitePool;

use crate::db::repository::settings::get as settings_get;
use crate::security::gate::SecurityContext;

/// 进程级缓存的应用设置镜像。
///
/// 启动与设置变更时由 [`Settings::load`] 重建；热路径上通过
/// `AppState.settings_cache` 读取。所有字段均为热路径真正需要的子集
/// （并非 settings 全量）。
#[derive(Debug, Clone)]
pub struct Settings {
    /// 是否记录原始请求/响应体（log_raw_body）。
    pub log_raw_body: bool,
    /// 请求失败自动换渠道重试开关（retry_enabled）。
    pub retry_enabled: bool,
    /// 额外重试次数（retry_times）。
    pub retry_times: i32,
    /// 安全扫描上下文：设置 + 启用规则（run_gate / scan_response / 流式增量审计共用）。
    pub security: SecurityContext,
}

impl Settings {
    /// 从数据库加载全部需要的设置与规则，构建缓存镜像。
    pub async fn load(pool: &SqlitePool) -> Result<Settings, sqlx::Error> {
        let log_raw_body = bool_setting(pool, "log_raw_body", false).await;
        let retry_enabled = bool_setting(pool, "retry_enabled", true).await;
        let retry_times = int_setting(pool, "retry_times", 3).await;
        let security = SecurityContext::load(pool).await?;
        Ok(Settings {
            log_raw_body,
            retry_enabled,
            retry_times,
            security,
        })
    }

    /// 锁中毒等极端情况下无法读缓存时的保守默认（与 settings 缺省一致）。
    ///
    /// security 上下文为空（不扫描），效果等同 fail-open 放行，保证主流程不中断。
    pub fn conservative_default() -> Settings {
        Settings {
            log_raw_body: false,
            retry_enabled: true,
            retry_times: 3,
            security: SecurityContext::disabled(),
        }
    }
}

async fn bool_setting(pool: &SqlitePool, key: &str, default: bool) -> bool {
    match settings_get(pool, key).await {
        Ok(Some(s)) => serde_json::from_str::<bool>(&s).unwrap_or(default),
        _ => default,
    }
}

async fn int_setting(pool: &SqlitePool, key: &str, default: i32) -> i32 {
    match settings_get(pool, key).await {
        Ok(Some(s)) => serde_json::from_str::<i32>(&s).unwrap_or(default),
        _ => default,
    }
}

/// 读取任意 settings KV 的原始字符串值（无默认值逻辑）。
///
/// 供服务注册表等「非标准设置键」使用（如 `service.<id>.enabled`）。
pub async fn get_setting_raw(pool: &SqlitePool, key: &str) -> Option<String> {
    settings_get(pool, key).await.ok().flatten()
}

/// 写入任意 settings KV（INSERT OR REPLACE，幂等）。
///
/// 注意：这不触发 `settings_cache` 重建，仅用于服务注册表这类「管理面低频、
/// 运行期不依赖缓存」的持久化状态。
pub async fn set_setting_raw(pool: &SqlitePool, key: &str, value: &str) -> Result<(), String> {
    sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)")
        .bind(key)
        .bind(value)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}
