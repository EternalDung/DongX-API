//! 安全规则的结构体与仓储。
//!
//! 内置规则（security_builtin_rules）由迁移 003 种子，用户可改 enabled；
//! 自定义规则（security_custom_rules）预留给用户黑名单/白名单（P2 UI）。
//! 扫描器只读取「已启用」的规则。

use sqlx::FromRow;
use sqlx::SqlitePool;

/// 内置检测规则（来自 security_builtin_rules 表）。
#[derive(Debug, Clone, FromRow)]
pub struct BuiltinRule {
    pub rule_id: String,
    pub category: String,
    pub severity: String,
    pub title: String,
    pub description: Option<String>,
    pub toggle_key: Option<String>,
    pub enabled: i64,
}

/// 自定义规则（来自 security_custom_rules 表）。
#[derive(Debug, Clone, FromRow)]
pub struct CustomRule {
    pub id: String,
    pub rule_type: String, // 'blacklist' | 'whitelist'
    pub category: String,  // 'domain' | 'tool' | 'path' | 'keyword'
    pub pattern: String,
    pub severity: String,
    pub action: String,
    pub enabled: i64,
    pub description: Option<String>,
    pub created_at: String,
}

/// 内置规则仓储。
pub struct BuiltinRuleRepository;

impl BuiltinRuleRepository {
    /// 仅取启用（enabled=1）的内置规则。扫描器据此 + toggle_key 开关双控。
    pub async fn get_enabled(pool: &SqlitePool) -> Result<Vec<BuiltinRule>, sqlx::Error> {
        sqlx::query_as::<_, BuiltinRule>(
            "SELECT rule_id, category, severity, title, description, toggle_key, enabled \
             FROM security_builtin_rules WHERE enabled = 1 ORDER BY rule_id",
        )
        .fetch_all(pool)
        .await
    }
}

/// 自定义规则仓储。
pub struct CustomRuleRepository;

impl CustomRuleRepository {
    /// 仅取启用的自定义规则（当前扫描器仅实现 blacklist 子串匹配）。
    pub async fn get_enabled(pool: &SqlitePool) -> Result<Vec<CustomRule>, sqlx::Error> {
        sqlx::query_as::<_, CustomRule>(
            "SELECT id, rule_type, category, pattern, severity, action, enabled, description, created_at \
             FROM security_custom_rules WHERE enabled = 1 ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await
    }

    /// 列出全部自定义规则（含已禁用），供前端管理页展示与开关切换。
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<CustomRule>, sqlx::Error> {
        sqlx::query_as::<_, CustomRule>(
            "SELECT id, rule_type, category, pattern, severity, action, enabled, description, created_at \
             FROM security_custom_rules ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await
    }

    /// 新建自定义规则（id 由调用方生成，created_at 用 DB 当前时间）。
    pub async fn insert(
        pool: &SqlitePool,
        id: &str,
        rule_type: &str,
        category: &str,
        pattern: &str,
        severity: &str,
        action: &str,
        enabled: i64,
        description: Option<&str>,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO security_custom_rules \
             (id, rule_type, category, pattern, severity, action, enabled, description, created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,datetime('now'))",
        )
        .bind(id)
        .bind(rule_type)
        .bind(category)
        .bind(pattern)
        .bind(severity)
        .bind(action)
        .bind(enabled)
        .bind(description)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// 更新自定义规则（按 id 全量覆盖可编辑字段）。返回受影响行数。
    pub async fn update(
        pool: &SqlitePool,
        id: &str,
        rule_type: &str,
        category: &str,
        pattern: &str,
        severity: &str,
        action: &str,
        enabled: i64,
        description: Option<&str>,
    ) -> Result<u64, sqlx::Error> {
        let n = sqlx::query(
            "UPDATE security_custom_rules SET \
             rule_type=?2, category=?3, pattern=?4, severity=?5, action=?6, enabled=?7, description=?8 \
             WHERE id=?1",
        )
        .bind(id)
        .bind(rule_type)
        .bind(category)
        .bind(pattern)
        .bind(severity)
        .bind(action)
        .bind(enabled)
        .bind(description)
        .execute(pool)
        .await?
        .rows_affected();
        Ok(n)
    }

    /// 启用 / 禁用（enabled: 0=禁用 1=启用）。返回受影响行数。
    pub async fn set_status(pool: &SqlitePool, id: &str, enabled: i64) -> Result<u64, sqlx::Error> {
        let n = sqlx::query("UPDATE security_custom_rules SET enabled=?2 WHERE id=?1")
            .bind(id)
            .bind(enabled)
            .execute(pool)
            .await?
            .rows_affected();
        Ok(n)
    }

    /// 删除自定义规则。返回受影响行数。
    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<u64, sqlx::Error> {
        let n = sqlx::query("DELETE FROM security_custom_rules WHERE id=?1")
            .bind(id)
            .execute(pool)
            .await?
            .rows_affected();
        Ok(n)
    }
}
