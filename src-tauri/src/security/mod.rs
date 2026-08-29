// 安全审计模块。
// 参考同类桌面 LLM 网关的安全闸门库表设计：内置规则(security_builtin_rules) +
// 自定义规则(security_custom_rules) + 每次请求发现明细(request_security_findings)，
// 请求日志(request_logs)汇总 risk_level/security_action 等。
//
// 子模块：
// - rules.rs   内置/自定义规则的结构体与仓储
// - scanner.rs 对请求 JSON 全树扫描，按启用规则产出发现
// - redact.rs  高风险类别脱敏转发体
// - gate.rs    编排：加载设置→扫描→决策动作→(可选)脱敏→输出
//
// rate_limit 仍属搁置未接线功能，保留并静音其死代码告警。
#![allow(dead_code)]

pub mod gate;
pub mod redact;
pub mod rules;
pub mod scanner;
pub mod rate_limit;

// 闸门输出结构，供 handler 直接以 security::GateOutput 引用。
pub use gate::GateOutput;

use serde::Serialize;

/// 风险等级（由低到高）。用于阈值判断与落库 risk_level。
#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
pub enum RiskLevel {
    Info,
    Low,
    Medium,
    High,
    Critical,
}

impl RiskLevel {
    /// 数值秩，便于阈值比较与风险评分加权。
    pub fn rank(&self) -> u8 {
        match self {
            RiskLevel::Info => 1,
            RiskLevel::Low => 2,
            RiskLevel::Medium => 3,
            RiskLevel::High => 4,
            RiskLevel::Critical => 5,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            RiskLevel::Info => "info",
            RiskLevel::Low => "low",
            RiskLevel::Medium => "medium",
            RiskLevel::High => "high",
            RiskLevel::Critical => "critical",
        }
    }
}

/// 字符串 -> 风险等级（未知归为 info）。
pub fn parse_risk_level(s: &str) -> RiskLevel {
    match s {
        "low" => RiskLevel::Low,
        "medium" => RiskLevel::Medium,
        "high" => RiskLevel::High,
        "critical" => RiskLevel::Critical,
        _ => RiskLevel::Info,
    }
}

/// 安全闸门对一次请求采取的最终动作。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecurityAction {
    /// 放行（宽松：仅记录）
    Allow,
    /// 标记告警（警告：中高风险）
    Warn,
    /// 脱敏后转发（脱敏：高风险）
    Redact,
    /// 阻断请求（严格：高风险）
    Block,
}

impl SecurityAction {
    pub fn as_str(&self) -> &'static str {
        match self {
            SecurityAction::Allow => "allow",
            SecurityAction::Warn => "warn",
            SecurityAction::Redact => "redact",
            SecurityAction::Block => "block",
        }
    }
}

/// 安全设置（来自 settings KV 表的 7 个键）。
#[derive(Debug, Clone)]
pub struct SecuritySettings {
    pub enabled: bool,
    /// permissive | warning | redact | strict
    pub mode: String,
    pub scan_credentials: bool,
    pub scan_pii: bool,
    pub scan_payment: bool,
    pub scan_network: bool,
    pub scan_code_exec: bool,
    pub scan_prompt_injection: bool,
}

/// 单条风险发现（对应 request_security_findings 一行）。
/// 由 scanner 产出，gate 在决策后填充 action。
#[derive(Debug, Clone, Serialize)]
pub struct SecurityFinding {
    pub rule_id: String,
    pub category: String,
    pub severity: String, // risk level 字符串
    pub title: String,
    pub description: Option<String>,
    pub location: Option<String>,
    pub evidence_masked: Option<String>,
}

/// 一次请求的安全审计结果汇总（写入 request_logs 的 6 个安全字段）。
#[derive(Debug, Clone)]
pub struct SecurityOutcome {
    pub risk_level: String,
    pub risk_score: i64,
    pub risk_summary: Option<String>,
    pub security_action: String,
    pub sanitized: bool,
    pub blocked_reason: Option<String>,
}

impl SecurityOutcome {
    /// 未启用/无命中时的默认（放行、无风险）。
    pub fn allow() -> Self {
        SecurityOutcome {
            risk_level: "none".to_string(),
            risk_score: 0,
            risk_summary: None,
            security_action: "allow".to_string(),
            sanitized: false,
            blocked_reason: None,
        }
    }
}

/// toggle_key -> 对应的 6 个检测开关。NULL/未知 -> 常开（true）。
pub fn is_switch_on(s: &SecuritySettings, toggle_key: &str) -> bool {
    match toggle_key {
        "scan_credentials" => s.scan_credentials,
        "scan_pii" => s.scan_pii,
        "scan_payment" => s.scan_payment,
        "scan_network" => s.scan_network,
        "scan_code_exec" => s.scan_code_exec,
        "scan_prompt_injection" => s.scan_prompt_injection,
        _ => true,
    }
}

/// 根据发现与模式决策最终动作 + 输出汇总。
pub fn decide_action(findings: &[SecurityFinding], settings: &SecuritySettings) -> (SecurityAction, SecurityOutcome) {
    if findings.is_empty() {
        return (SecurityAction::Allow, SecurityOutcome::allow());
    }

    let max = findings
        .iter()
        .map(|f| parse_risk_level(&f.severity))
        .max()
        .unwrap_or(RiskLevel::Info);
    let risk_level = max.as_str().to_string();

    // 风险评分：各发现秩之和，封顶 999，便于排序与展示。
    let risk_score: i64 = findings
        .iter()
        .map(|f| parse_risk_level(&f.severity).rank() as i64)
        .sum::<i64>()
        .min(999);

    // 汇总文案：按等级计数 + 最高风险标题。
    let mut counts = [0u32; 5]; // Info/Low/Medium/High/Critical
    let mut top: Option<&SecurityFinding> = None;
    for f in findings {
        let lvl = parse_risk_level(&f.severity);
        counts[lvl.rank() as usize - 1] += 1;
        if top.map_or(true, |t| parse_risk_level(&t.severity) < lvl) {
            top = Some(f);
        }
    }
    let risk_summary = Some(format!(
        "检出 {} 项风险（高:{} 中:{} 低:{}）",
        findings.len(),
        counts[RiskLevel::High.rank() as usize - 1] + counts[RiskLevel::Critical.rank() as usize - 1],
        counts[RiskLevel::Medium.rank() as usize - 1],
        counts[RiskLevel::Low.rank() as usize - 1] + counts[RiskLevel::Info.rank() as usize - 1],
    ));

    let action = match settings.mode.as_str() {
        // 宽松：只记录，永远放行。
        "permissive" => SecurityAction::Allow,
        // 警告：中高风险标记告警，仍放行原文。
        "warning" => {
            if max.rank() >= RiskLevel::Medium.rank() {
                SecurityAction::Warn
            } else {
                SecurityAction::Allow
            }
        }
        // 脱敏：高风险脱敏后转发。
        "redact" => {
            if max.rank() >= RiskLevel::High.rank() {
                SecurityAction::Redact
            } else {
                SecurityAction::Allow
            }
        }
        // 严格：高风险阻断；中高标记告警；其余放行。
        "strict" => {
            if max.rank() >= RiskLevel::High.rank() {
                SecurityAction::Block
            } else if max.rank() >= RiskLevel::Medium.rank() {
                SecurityAction::Warn
            } else {
                SecurityAction::Allow
            }
        }
        _ => SecurityAction::Allow,
    };

    let sanitized = action == SecurityAction::Redact;
    let blocked_reason = if action == SecurityAction::Block {
        Some(format!(
            "安全审计（{} 模式）：命中 {} 级风险「{}」，已阻断请求",
            settings.mode,
            risk_level,
            top.map(|t| t.title.as_str()).unwrap_or("未知")
        ))
    } else {
        None
    };

    let outcome = SecurityOutcome {
        risk_level,
        risk_score,
        risk_summary,
        security_action: action.as_str().to_string(),
        sanitized,
        blocked_reason,
    };
    (action, outcome)
}
