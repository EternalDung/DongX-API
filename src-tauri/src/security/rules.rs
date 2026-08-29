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
    pub rule_type: String, // 'blacklist' | 'whitelist'
    pub category: String,  // 'domain' | 'tool' | 'path' | 'keyword'
    pub pattern: String,
    pub severity: String,
    pub action: String,
    pub enabled: i64,
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
            "SELECT rule_type, category, pattern, severity, action, enabled \
             FROM security_custom_rules WHERE enabled = 1 ORDER BY created_at DESC",
        )
        .fetch_all(pool)
        .await
    }
}
