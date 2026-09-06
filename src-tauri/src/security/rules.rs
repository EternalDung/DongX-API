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

    /// 列出全部内置规则（含已禁用），供管理页展示与开关/严重度编辑。
    pub async fn list_all(pool: &SqlitePool) -> Result<Vec<BuiltinRule>, sqlx::Error> {
        sqlx::query_as::<_, BuiltinRule>(
            "SELECT rule_id, category, severity, title, description, toggle_key, enabled \
             FROM security_builtin_rules ORDER BY rule_id",
        )
        .fetch_all(pool)
        .await
    }

    /// 启用 / 禁用单条内置规则（enabled: 0=禁用 1=启用）。返回受影响行数。
    pub async fn update_enabled(
        pool: &SqlitePool,
        rule_id: &str,
        enabled: i64,
    ) -> Result<u64, sqlx::Error> {
        let n = sqlx::query("UPDATE security_builtin_rules SET enabled=?2 WHERE rule_id=?1")
            .bind(rule_id)
            .bind(enabled)
            .execute(pool)
            .await?
            .rows_affected();
        Ok(n)
    }

    /// 更新单条内置规则的严重等级。返回受影响行数。
    pub async fn update_severity(
        pool: &SqlitePool,
        rule_id: &str,
        severity: &str,
    ) -> Result<u64, sqlx::Error> {
        let n = sqlx::query("UPDATE security_builtin_rules SET severity=?2 WHERE rule_id=?1")
            .bind(rule_id)
            .bind(severity)
            .execute(pool)
            .await?
            .rows_affected();
        Ok(n)
    }

    /// 恢复全部内置规则到出厂默认（severity/title/description/toggle_key/category/enabled 还原）。
    /// 在事务内执行，保证原子性。
    pub async fn reset_to_defaults(pool: &SqlitePool) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        for s in DEFAULT_BUILTIN_RULES {
            sqlx::query(
                "UPDATE security_builtin_rules SET \
                 category=?2, severity=?3, title=?4, description=?5, toggle_key=?6, enabled=1 \
                 WHERE rule_id=?1",
            )
            .bind(s.rule_id)
            .bind(s.category)
            .bind(s.severity)
            .bind(s.title)
            .bind(s.description.map(|d| d.to_string()))
            .bind(s.toggle_key.map(|t| t.to_string()))
            .execute(&mut *tx)
            .await?;
        }
        // 部分规则默认禁用（LLM 场景误报率高）：net.external_url（几乎必含外链，info 噪音）、
        // exec.git_info（Git 信息泄露场景窄，low 噪音）。重置时也一并关闭，与迁移 006 一致。
        sqlx::query(
            "UPDATE security_builtin_rules SET enabled=0 \
             WHERE rule_id IN ('net.external_url','exec.git_info')",
        )
        .execute(&mut *tx)
        .await?;
        tx.commit().await?;
        Ok(())
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

/// 内置规则出厂默认值（与迁移 003/004 种子完全一致）。
/// `reset_to_defaults` 据此还原 severity/enabled 等字段，使「恢复默认」幂等且可预期。
/// 字段顺序：rule_id, category, severity, title, description, toggle_key(NULL=常开)。
struct BuiltinRuleSeed {
    rule_id: &'static str,
    category: &'static str,
    severity: &'static str,
    title: &'static str,
    description: Option<&'static str>,
    toggle_key: Option<&'static str>,
}

const DEFAULT_BUILTIN_RULES: &[BuiltinRuleSeed] = &[
    BuiltinRuleSeed {
        rule_id: "cred.secret_token",
        category: "credential",
        severity: "high",
        title: "疑似密钥/Token",
        description: Some("检测 sk-/ghp_/AKIA/AIza/JWT/Bearer/xoxb 等凭证格式"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "cred.private_key",
        category: "credential",
        severity: "critical",
        title: "私钥内容",
        description: Some("检测 PEM/OpenSSH 私钥头部"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "cred.named_secret",
        category: "credential",
        severity: "high",
        title: "敏感凭证字段",
        description: Some("检测 Authorization/Cookie/Session/Secret/Password/Token 等字段名赋值"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "cred.database_url",
        category: "credential",
        severity: "high",
        title: "数据库连接串",
        description: Some("检测 mysql:// postgres:// mongodb:// redis://"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "cred.cloud_key",
        category: "credential",
        severity: "high",
        title: "云厂商密钥",
        description: Some("检测 AWS AKIA/腾讯云 SecretId/阿里云 AccessKey/GCP AIza"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "pii.id_card",
        category: "personal",
        severity: "medium",
        title: "中国居民身份证号",
        description: Some("检测 18 位居民身份证号码"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "pii.email",
        category: "personal",
        severity: "low",
        title: "邮箱地址",
        description: Some("检测邮箱格式字符串"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "pii.phone",
        category: "personal",
        severity: "low",
        title: "手机号码",
        description: Some("检测中国大陆手机号"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "pay.credit_card",
        category: "payment",
        severity: "high",
        title: "信用卡号",
        description: Some("检测主流卡组织卡号（4/5/3/6 开头分组）"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "pay.bank_card",
        category: "payment",
        severity: "high",
        title: "银行卡号",
        description: Some("检测 16-19 位银行卡号（含银联 62 开头）"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "net.ip_probe",
        category: "network",
        severity: "high",
        title: "公网 IP 探测",
        description: Some("检测 ifconfig.me/ipinfo.io/ipify.org 等"),
        toggle_key: Some("security_scan_network"),
    },
    BuiltinRuleSeed {
        rule_id: "net.suspicious_domain",
        category: "network",
        severity: "high",
        title: "可疑外联域名",
        description: Some("检测 webhook.site/ngrok/pastebin/requestbin"),
        toggle_key: Some("security_scan_network"),
    },
    BuiltinRuleSeed {
        rule_id: "net.external_url",
        category: "network",
        severity: "info",
        title: "外部 URL",
        description: Some("检测请求中的 http(s) 外链"),
        toggle_key: Some("security_scan_network"),
    },
    BuiltinRuleSeed {
        rule_id: "net.tracking_pixel",
        category: "network",
        severity: "high",
        title: "追踪像素",
        description: Some("检测 1x1 图片/track/pixel/beacon"),
        toggle_key: Some("security_scan_network"),
    },
    BuiltinRuleSeed {
        rule_id: "exec.shell_command",
        category: "tool",
        severity: "medium",
        title: "高风险命令片段",
        description: Some("检测 curl/wget/nc/scp/bash -c/python -c"),
        toggle_key: Some("security_scan_tools"),
    },
    BuiltinRuleSeed {
        rule_id: "exec.exfiltration",
        category: "tool",
        severity: "critical",
        title: "疑似数据外传命令",
        description: Some("检测敏感文件读取与网络外传的组合"),
        toggle_key: Some("security_scan_tools"),
    },
    BuiltinRuleSeed {
        rule_id: "exec.remote_script",
        category: "tool",
        severity: "critical",
        title: "远程脚本执行",
        description: Some("检测 curl|wget 下载脚本管道到 sh/bash"),
        toggle_key: Some("security_scan_tools"),
    },
    BuiltinRuleSeed {
        rule_id: "exec.git_info",
        category: "tool",
        severity: "low",
        title: "Git 信息泄露",
        description: Some("检测 git remote/gh auth token"),
        toggle_key: Some("security_scan_tools"),
    },
    BuiltinRuleSeed {
        rule_id: "exec.ssh_key",
        category: "tool",
        severity: "critical",
        title: "SSH 密钥文件",
        description: Some("检测 id_rsa/id_ed25519/id_ecdsa"),
        toggle_key: Some("security_scan_tools"),
    },
    BuiltinRuleSeed {
        rule_id: "prompt.injection",
        category: "prompt",
        severity: "high",
        title: "提示注入/越权",
        description: Some("检测要求忽略指令/隐藏行为/绕过审计"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "prompt.fingerprint",
        category: "prompt",
        severity: "medium",
        title: "账号画像/风控上下文",
        description: Some("检测指纹/代理/风控相关词同时出现"),
        toggle_key: None,
    },
    BuiltinRuleSeed {
        rule_id: "unicode.zero_width",
        category: "unicode",
        severity: "medium",
        title: "零宽 Unicode 字符",
        description: Some("检测 U+200B/200C/200D/2060/FEFF 等不可见字符"),
        toggle_key: Some("security_scan_unicode"),
    },
    BuiltinRuleSeed {
        rule_id: "unicode.bidi_control",
        category: "unicode",
        severity: "high",
        title: "方向控制 Unicode 字符",
        description: Some("检测 U+202A-202E、U+2066-2069 等 Bidi 控制字符"),
        toggle_key: Some("security_scan_unicode"),
    },
    BuiltinRuleSeed {
        rule_id: "unicode.variation_selector",
        category: "unicode",
        severity: "medium",
        title: "变体选择符",
        description: Some("检测 U+FE00-FE0F、U+E0100-E01EF 等 variation selector"),
        toggle_key: Some("security_scan_unicode"),
    },
    BuiltinRuleSeed {
        rule_id: "unicode.homograph",
        category: "unicode",
        severity: "medium",
        title: "同形异义字符",
        description: Some("检测西里尔、希腊等与拉丁字母同形的字符，可能用于域名混淆"),
        toggle_key: Some("security_scan_unicode"),
    },
];
