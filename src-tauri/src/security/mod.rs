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
// rate_limit 已实现并在请求链路中启用（见 server/handler.rs 的网关密钥限速）。

pub mod gate;
pub mod rate_limit;
pub mod redact;
pub mod rules;
pub mod scanner;

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

/// 安全设置（来自 settings KV 表）。
/// 安全设置模型：4 模式(audit/warn/redact/block) + 6 开关(3 检测 + 响应 + 2 行为)。
#[derive(Debug, Clone)]
pub struct SecuritySettings {
    pub enabled: bool,
    /// audit | warn | redact | block
    pub mode: String,
    /// 3 个可被独立开关控制的扫描类目。
    pub scan_unicode: bool,
    pub scan_tools: bool,
    pub scan_network: bool,
    /// 响应侧扫描开关（已实装：gate::scan_response + handler 非流式/流式两处均接入）。
    pub scan_response: bool,
    /// 行为开关：独立于模式，控制转发体脱敏（redact_secrets）。
    pub redact_secrets: bool,
    /// 行为开关：跨模式覆盖，Critical 一律阻断（block_on_critical）。
    pub block_on_critical: bool,
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
    /// 明文证据 SHA-256（十六进制）。用于审计取证链：跨 request/response/response_delta
    /// 阶段以同一哈希去重/串联「同一明文」，而不在任何落库字段保留明文；不暴露给前端。
    pub evidence_hash: Option<String>,
    /// 扫描阶段：request（入站请求体）/ response（出站响应体）。落库 request_security_findings.phase。
    pub phase: String,
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

    /// 审计未启用（关闭）时的占位结果：明确区别于「启用且无风险 = 安全(none)」。
    /// 前端据此渲染灰色「未审计」徽章，避免把「没开审计」误展示成「安全」。
    pub fn skipped() -> Self {
        SecurityOutcome {
            risk_level: "skipped".to_string(),
            risk_score: 0,
            risk_summary: None,
            security_action: "allow".to_string(),
            sanitized: false,
            blocked_reason: None,
        }
    }
}

/// toggle_key -> 对应的 3 个可切换扫描类目开关。
/// 仅 unicode/tools/network 受独立开关控制；NULL/未知/凭证/PII/支付/命令/提示注入
/// 等类目视为常开（true——这些类别不可单独关闭）。
pub fn is_switch_on(s: &SecuritySettings, toggle_key: &str) -> bool {
    match toggle_key {
        "security_scan_unicode" => s.scan_unicode,
        "security_scan_tools" => s.scan_tools,
        "security_scan_network" => s.scan_network,
        _ => true,
    }
}

/// 仅根据发现计算风险指标（risk_level / risk_score / risk_summary / 最高风险标题），
/// 不决定动作。供两种场景复用：
/// - `decide_action` 内部生成 `SecurityOutcome`；
/// - 响应阶段扫描后，把「请求+响应」合并发现重算汇总，使主行与落库 findings 一致。
pub fn compute_risk_metrics(
    findings: &[SecurityFinding],
) -> (String, i64, Option<String>, Option<String>) {
    if findings.is_empty() {
        return ("none".to_string(), 0, None, None);
    }

    let mut counts = [0u32; 5]; // Info/Low/Medium/High/Critical
    let mut top: Option<&SecurityFinding> = None;
    for f in findings {
        let lvl = parse_risk_level(&f.severity);
        counts[lvl.rank() as usize - 1] += 1;
        if top.is_none_or(|t| parse_risk_level(&t.severity) < lvl) {
            top = Some(f);
        }
    }
    let max = top
        .map(|t| parse_risk_level(&t.severity))
        .unwrap_or(RiskLevel::Info);
    let risk_level = max.as_str().to_string();

    // 风险评分：各发现秩之和，封顶 999，便于排序与展示。
    let risk_score: i64 = findings
        .iter()
        .map(|f| parse_risk_level(&f.severity).rank() as i64)
        .sum::<i64>()
        .min(999);

    let risk_summary = Some(format!(
        "检出 {} 项风险（高:{} 中:{} 低:{}）",
        findings.len(),
        counts[RiskLevel::High.rank() as usize - 1]
            + counts[RiskLevel::Critical.rank() as usize - 1],
        counts[RiskLevel::Medium.rank() as usize - 1],
        counts[RiskLevel::Low.rank() as usize - 1] + counts[RiskLevel::Info.rank() as usize - 1],
    ));

    let top_title = top.map(|t| t.title.clone());
    (risk_level, risk_score, risk_summary, top_title)
}

/// 根据发现与模式决策最终动作 + 输出汇总。
pub fn decide_action(
    findings: &[SecurityFinding],
    settings: &SecuritySettings,
) -> (SecurityAction, SecurityOutcome) {
    if findings.is_empty() {
        return (SecurityAction::Allow, SecurityOutcome::allow());
    }

    let (risk_level, risk_score, risk_summary, top_title) = compute_risk_metrics(findings);
    let max = parse_risk_level(&risk_level);

    let mut action = match settings.mode.as_str() {
        // 审计：仅记录，永远放行。
        "audit" => SecurityAction::Allow,
        // 警告：中高风险标记告警，仍放行原文。
        "warn" => {
            if max.rank() >= RiskLevel::Medium.rank() {
                SecurityAction::Warn
            } else {
                SecurityAction::Allow
            }
        }
        // 脱敏：高风险动作标记为 Redact；实际转发脱敏由 redact_secrets 独立控制。
        "redact" => {
            if max.rank() >= RiskLevel::High.rank() {
                SecurityAction::Redact
            } else {
                SecurityAction::Allow
            }
        }
        // 阻断：高风险阻断；其余放行。
        "block" => {
            if max.rank() >= RiskLevel::High.rank() {
                SecurityAction::Block
            } else {
                SecurityAction::Allow
            }
        }
        _ => SecurityAction::Allow,
    };

    // 跨模式覆盖：严重风险强制阻断（block_on_critical）。
    if settings.block_on_critical && max == RiskLevel::Critical {
        action = SecurityAction::Block;
    }

    let sanitized = settings.redact_secrets;
    let blocked_reason = if action == SecurityAction::Block {
        Some(format!(
            "安全审计（{} 模式）：命中 {} 级风险「{}」，已阻断请求",
            settings.mode,
            risk_level,
            top_title.as_deref().unwrap_or("未知")
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

#[cfg(test)]
mod tests {
    use super::*;

    fn settings(mode: &str) -> SecuritySettings {
        SecuritySettings {
            enabled: true,
            mode: mode.to_string(),
            scan_unicode: true,
            scan_tools: true,
            scan_network: true,
            scan_response: true,
            redact_secrets: false,
            block_on_critical: false,
        }
    }

    fn finding(severity: &str) -> SecurityFinding {
        SecurityFinding {
            rule_id: "x".to_string(),
            category: "credential".to_string(),
            severity: severity.to_string(),
            title: "t".to_string(),
            description: None,
            location: None,
            evidence_masked: None,
            evidence_hash: None,
            phase: "request".to_string(),
        }
    }

    #[test]
    fn empty_findings_always_allow() {
        let (a, o) = decide_action(&[], &settings("block"));
        assert_eq!(a, SecurityAction::Allow);
        assert_eq!(o.risk_level, "none");
    }

    #[test]
    fn block_blocks_high() {
        let (a, o) = decide_action(&[finding("high")], &settings("block"));
        assert_eq!(a, SecurityAction::Block);
        assert!(o.blocked_reason.is_some());
    }

    #[test]
    fn block_allows_medium() {
        let (a, _o) = decide_action(&[finding("medium")], &settings("block"));
        assert_eq!(a, SecurityAction::Allow);
    }

    #[test]
    fn warn_warns_medium() {
        let (a, _o) = decide_action(&[finding("medium")], &settings("warn"));
        assert_eq!(a, SecurityAction::Warn);
    }

    #[test]
    fn warn_allows_low() {
        let (a, _o) = decide_action(&[finding("low")], &settings("warn"));
        assert_eq!(a, SecurityAction::Allow);
    }

    #[test]
    fn redact_redacts_high() {
        let (a, _o) = decide_action(&[finding("high")], &settings("redact"));
        assert_eq!(a, SecurityAction::Redact);
    }

    #[test]
    fn redact_allows_medium() {
        let (a, _o) = decide_action(&[finding("medium")], &settings("redact"));
        assert_eq!(a, SecurityAction::Allow);
    }

    #[test]
    fn audit_allows_high() {
        let (a, _o) = decide_action(&[finding("high")], &settings("audit"));
        assert_eq!(a, SecurityAction::Allow);
    }

    #[test]
    fn block_on_critical_overrides_mode() {
        // 即便模式是 warn（本不阻断），block_on_critical 也应强制阻断 Critical。
        let mut s = settings("warn");
        s.block_on_critical = true;
        let (a, _o) = decide_action(&[finding("critical")], &s);
        assert_eq!(a, SecurityAction::Block);
    }

    #[test]
    fn block_on_critical_ignores_high() {
        // block_on_critical 仅对 Critical 生效，High 不被强制阻断。
        let mut s = settings("audit");
        s.block_on_critical = true;
        let (a, _o) = decide_action(&[finding("high")], &s);
        assert_eq!(a, SecurityAction::Allow);
    }

    #[test]
    fn risk_score_is_sum_of_ranks() {
        // medium(3) + high(4) = 7
        let (_a, o) = decide_action(&[finding("medium"), finding("high")], &settings("warning"));
        assert_eq!(o.risk_score, 7);
    }

    #[test]
    fn unknown_toggle_key_is_always_on() {
        let s = settings("warning");
        assert!(is_switch_on(&s, "not_a_real_toggle"));
    }

    #[test]
    fn known_toggle_off_is_respected() {
        let mut s = settings("audit");
        s.scan_network = false;
        assert!(!is_switch_on(&s, "security_scan_network"));
    }
}
