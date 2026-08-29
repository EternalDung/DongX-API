//! 安全闸门编排。
//!
//! 一次内容请求的核心漏斗：加载安全设置 → 取启用规则 → 扫描原始 JSON →
//! 决策动作（Allow/Warn/Redact/Block）→ 脱敏模式替换转发体 → 输出 GateOutput。
//!
//! 任何内部错误（读设置/读规则失败）一律 fail-open 放行，并记录告警，
//! 绝不因审计子系统异常而阻断正常请求（与配额/健康统计同原则）。

use sqlx::SqlitePool;

use serde_json::Value;

use super::{decide_action, redact, scanner, SecurityAction, SecurityOutcome, SecuritySettings, SecurityFinding};
use crate::db::repository::settings::get as settings_get;
use crate::security::rules::{BuiltinRuleRepository, CustomRuleRepository};

/// 闸门输出。
pub struct GateOutput {
    /// 实际转发给上游的请求体（脱敏模式下已被替换）。
    pub forward_body: Value,
    /// 汇总结果（写入 request_logs 的 6 个安全字段）。
    pub outcome: SecurityOutcome,
    /// 命中明细（写入 request_security_findings）。
    pub findings: Vec<SecurityFinding>,
    /// 决策动作（handler 据此判断是否阻断）。
    pub action: SecurityAction,
}

async fn bool_setting(pool: &SqlitePool, key: &str, default: bool) -> bool {
    match settings_get(pool, key).await {
        Ok(Some(s)) => serde_json::from_str::<bool>(&s).unwrap_or(default),
        _ => default,
    }
}

/// 运行安全闸门。Err 表示子系统异常，调用方应 fail-open 放行。
pub async fn run_gate(pool: &SqlitePool, body: Value) -> Result<GateOutput, sqlx::Error> {
    let enabled = bool_setting(pool, "security_enabled", true).await;
    if !enabled {
        return Ok(GateOutput {
            forward_body: body,
            outcome: SecurityOutcome::allow(),
            findings: Vec::new(),
            action: SecurityAction::Allow,
        });
    }

    let mode = match settings_get(pool, "security_mode").await {
        Ok(Some(s)) => serde_json::from_str::<String>(&s).unwrap_or_else(|_| "audit".to_string()),
        _ => "audit".to_string(),
    };

    // 对齐 waliapi 的 6 开关：3 个可切换扫描类目(默认开) + 响应扫描(默认关)
    // + 2 个行为开关(redact_secrets/block_on_critical 默认关，与模式解耦)。
    let sec = SecuritySettings {
        enabled,
        mode,
        scan_unicode: bool_setting(pool, "security_scan_unicode", true).await,
        scan_tools: bool_setting(pool, "security_scan_tools", true).await,
        scan_network: bool_setting(pool, "security_scan_network", true).await,
        scan_response: bool_setting(pool, "security_scan_response", false).await,
        redact_secrets: bool_setting(pool, "security_redact_secrets", false).await,
        block_on_critical: bool_setting(pool, "security_block_on_critical", false).await,
    };

    let builtin = BuiltinRuleRepository::get_enabled(pool).await?;
    let custom = CustomRuleRepository::get_enabled(pool).await?;

    let result = scanner::scan(&body, &sec, &builtin, &custom);
    if result.budget_exceeded {
        tracing::warn!("安全扫描触发字节预算上限，已跳过剩余内容（未阻断）");
    }

    let (action, outcome) = decide_action(&result.findings, &sec);
    // 转发体脱敏由独立开关 redact_secrets 控制（对齐 waliapi，与模式解耦）。
    let forward_body = if sec.redact_secrets {
        redact::redact(&body)
    } else {
        body
    };

    Ok(GateOutput {
        forward_body,
        outcome,
        findings: result.findings,
        action,
    })
}

/// 扫描出站响应体（对齐 waliapi 的 security_scan_response 开关）。
///
/// 与 run_gate 的区别：
/// - 仅当 `security_enabled` 且 `security_scan_response` 同时开启才扫描；否则返回空（无发现）。
/// - 响应已发送给客户端，无需脱敏转发体，也不据此阻断；只产出发现与风险汇总供落库审计。
/// - 发现统一标记 `phase = "response"`，落库 request_security_findings.phase 以区分请求阶段。
pub async fn scan_response(pool: &SqlitePool, body: Value) -> Result<GateOutput, sqlx::Error> {
    let enabled = bool_setting(pool, "security_enabled", true).await;
    if !enabled {
        return Ok(GateOutput {
            forward_body: body,
            outcome: SecurityOutcome::allow(),
            findings: Vec::new(),
            action: SecurityAction::Allow,
        });
    }

    // 响应侧独立开关：关闭则不扫描响应（与请求扫描解耦，对齐 waliapi）。
    let scan_response = bool_setting(pool, "security_scan_response", false).await;
    if !scan_response {
        return Ok(GateOutput {
            forward_body: body,
            outcome: SecurityOutcome::allow(),
            findings: Vec::new(),
            action: SecurityAction::Allow,
        });
    }

    // 响应扫描复用同样的 SecuritySettings（含各类目开关），但模式固定 audit：
    // 响应不触发阻断，仅记录风险等级/评分供审计。
    let mode = match settings_get(pool, "security_mode").await {
        Ok(Some(s)) => serde_json::from_str::<String>(&s).unwrap_or_else(|_| "audit".to_string()),
        _ => "audit".to_string(),
    };
    let sec = SecuritySettings {
        enabled,
        mode,
        scan_unicode: bool_setting(pool, "security_scan_unicode", true).await,
        scan_tools: bool_setting(pool, "security_scan_tools", true).await,
        scan_network: bool_setting(pool, "security_scan_network", true).await,
        scan_response: true,
        redact_secrets: false,
        block_on_critical: false,
    };

    let builtin = BuiltinRuleRepository::get_enabled(pool).await?;
    let custom = CustomRuleRepository::get_enabled(pool).await?;
    let result = scanner::scan(&body, &sec, &builtin, &custom);

    let (action, outcome) = decide_action(&result.findings, &sec);
    let mut findings = result.findings;
    for f in findings.iter_mut() {
        f.phase = "response".to_string();
    }

    Ok(GateOutput {
        forward_body: body,
        outcome,
        findings,
        action,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use sqlx::sqlite::SqlitePoolOptions;
    use sqlx::SqlitePool;

    /// 建内存库 + 跑全部迁移（含 003/004），得到可复用的单连接池。
    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1) // 内存库：单连接共享同一份数据
            .connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    /// 写一条 setting（value 为 JSON 编码字符串，与 settings_get 解析一致）。
    async fn set(pool: &SqlitePool, key: &str, value: &str) {
        sqlx::query("INSERT OR REPLACE INTO settings (key, value) VALUES (?, ?)")
            .bind(key)
            .bind(value)
            .execute(pool)
            .await
            .expect("set setting");
    }

    #[tokio::test]
    async fn audit_mode_finds_but_allows() {
        let pool = test_pool().await;
        set(&pool, "security_mode", "\"audit\"").await;
        let body = json!({"content":"my key is sk-abcdefghijklmnopqrstuvwx"});
        let out = run_gate(&pool, body).await.expect("gate");
        assert_eq!(out.action, SecurityAction::Allow, "audit 模式永不阻断");
        assert!(!out.findings.is_empty(), "应检出凭证");
        assert_eq!(out.outcome.risk_level, "high");
    }

    #[tokio::test]
    async fn block_mode_blocks_secret() {
        let pool = test_pool().await;
        set(&pool, "security_mode", "\"block\"").await;
        let body = json!({"content":"use AKIAABCDEFGHIJKLMNOP as creds"});
        let out = run_gate(&pool, body).await.expect("gate");
        assert_eq!(out.action, SecurityAction::Block, "block 模式应阻断 High");
        assert!(out.outcome.blocked_reason.is_some());
    }

    #[tokio::test]
    async fn redact_secrets_redacts_forward_body() {
        let pool = test_pool().await;
        set(&pool, "security_mode", "\"audit\"").await;
        set(&pool, "security_redact_secrets", "true").await;
        let secret = "sk-abcdefghijklmnopqrstuvwx";
        let body = json!({"content": format!("key {}", secret)});
        let out = run_gate(&pool, body).await.expect("gate");
        let fwd = serde_json::to_string(&out.forward_body).unwrap();
        assert!(fwd.contains("[REDACTED]"), "转发体应被脱敏: {}", fwd);
        assert!(!fwd.contains(secret), "转发体不应含明文密钥");
        assert_eq!(out.outcome.security_action, "allow");
        assert!(out.outcome.sanitized, "sanitized 应反映 redact_secrets");
    }

    #[tokio::test]
    async fn block_on_critical_overrides_warn_mode() {
        let pool = test_pool().await;
        set(&pool, "security_mode", "\"warn\"").await;
        set(&pool, "security_block_on_critical", "true").await;
        // 私钥命中 critical
        let body = json!({"content":"-----BEGIN PRIVATE KEY-----\nabc"});
        let out = run_gate(&pool, body).await.expect("gate");
        assert_eq!(
            out.action,
            SecurityAction::Block,
            "warn 模式下 critical 应被 block_on_critical 强制阻断"
        );
    }

    #[tokio::test]
    async fn network_toggle_off_skips_category() {
        let pool = test_pool().await;
        set(&pool, "security_mode", "\"audit\"").await;
        set(&pool, "security_scan_network", "false").await;
        let body = json!({"content":"send it to webhook.site now"});
        let out = run_gate(&pool, body).await.expect("gate");
        assert!(
            out.findings.is_empty(),
            "关闭 security_scan_network 后不应检出 webhook.site"
        );
    }

    #[tokio::test]
    async fn scan_unicode_key_is_wired_not_dead_key() {
        // 证明 gate 读的是 waliapi 键 security_scan_unicode，而非旧死键 scan_*。
        let pool = test_pool().await;
        set(&pool, "security_mode", "\"audit\"").await;

        set(&pool, "security_scan_unicode", "false").await;
        let off = run_gate(&pool, json!({"content":"\u{200B}sneaky"}))
            .await
            .expect("gate");
        assert!(
            off.findings.iter().all(|f| f.rule_id != "unicode.zero_width"),
            "关闭 security_scan_unicode 后零宽字符不应检出（证明新键已接线）"
        );

        set(&pool, "security_scan_unicode", "\"true\"").await;
        let on = run_gate(&pool, json!({"content":"\u{200B}sneaky"}))
            .await
            .expect("gate");
        assert!(
            on.findings.iter().any(|f| f.rule_id == "unicode.zero_width"),
            "开启 security_scan_unicode 后应检出零宽字符"
        );
    }

    #[tokio::test]
    async fn scan_response_off_no_findings() {
        // security_scan_response 关闭时，响应体不扫描。
        let pool = test_pool().await;
        set(&pool, "security_scan_response", "false").await;
        let body = json!({"choices":[{"message":{"content":"leak sk-abcdefghijklmnopqrstuvwx"}}]});
        let out = scan_response(&pool, body).await.expect("gate");
        assert!(out.findings.is_empty(), "响应扫描关闭时不应有发现");
    }

    #[tokio::test]
    async fn scan_response_on_finds_secret_and_tags_phase() {
        // security_scan_response 开启时，响应体中的密钥应被检出且 phase=response。
        let pool = test_pool().await;
        set(&pool, "security_scan_response", "true").await;
        let body = json!({"choices":[{"message":{"content":"leak sk-abcdefghijklmnopqrstuvwx"}}]});
        let out = scan_response(&pool, body).await.expect("gate");
        assert!(
            out.findings.iter().any(|f| f.rule_id == "cred.secret_token"),
            "响应体中的密钥应被检出"
        );
        assert!(
            out.findings.iter().all(|f| f.phase == "response"),
            "响应阶段发现 phase 应为 response"
        );
        assert!(out.outcome.risk_level == "high");
    }

    #[tokio::test]
    async fn scan_response_respects_unicode_toggle() {
        // 响应扫描仍受 category 开关约束：关闭 scan_unicode 后响应内零宽字符不检出。
        let pool = test_pool().await;
        set(&pool, "security_scan_response", "true").await;
        set(&pool, "security_scan_unicode", "false").await;
        let off = scan_response(&pool, json!({"content":"\u{200B}sneaky"}))
            .await
            .expect("gate");
        assert!(
            off.findings.iter().all(|f| f.rule_id != "unicode.zero_width"),
            "响应扫描关闭 scan_unicode 后不应检出零宽字符"
        );

        set(&pool, "security_scan_unicode", "\"true\"").await;
        let on = scan_response(&pool, json!({"content":"\u{200B}sneaky"}))
            .await
            .expect("gate");
        assert!(
            on.findings.iter().any(|f| f.rule_id == "unicode.zero_width"),
            "响应扫描开启 scan_unicode 后应检出零宽字符"
        );
    }
}
