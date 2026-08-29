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
        Ok(Some(s)) => serde_json::from_str::<String>(&s).unwrap_or_else(|_| "warning".to_string()),
        _ => "warning".to_string(),
    };

    let sec = SecuritySettings {
        enabled,
        mode,
        scan_credentials: bool_setting(pool, "scan_credentials", true).await,
        scan_pii: bool_setting(pool, "scan_pii", true).await,
        scan_payment: bool_setting(pool, "scan_payment", true).await,
        scan_network: bool_setting(pool, "scan_network", true).await,
        scan_code_exec: bool_setting(pool, "scan_code_exec", true).await,
        scan_prompt_injection: bool_setting(pool, "scan_prompt_injection", true).await,
    };

    let builtin = BuiltinRuleRepository::get_enabled(pool).await?;
    let custom = CustomRuleRepository::get_enabled(pool).await?;

    let result = scanner::scan(&body, &sec, &builtin, &custom);
    if result.budget_exceeded {
        tracing::warn!("安全扫描触发字节预算上限，已跳过剩余内容（未阻断）");
    }

    let (action, outcome) = decide_action(&result.findings, &sec);
    let forward_body = if action == SecurityAction::Redact {
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
