//! 请求体安全扫描器。
//!
//! 对下游原始 JSON 做全树遍历，对每个字符串叶子按「已启用 + toggle_key 开关开启」
//! 的内置规则跑正则；命中即产出 SecurityFinding（含 JSON 指针定位与脱敏证据）。
//! 自定义黑名单规则做子串匹配。
//!
//! 正则集中在 PATTERNS 静态表，由 OnceLock 编译一次，避免每次请求重编译。
//! 预算（MAX_SCAN_BYTES）保护扫描器免受畸形/超大输入拖累——超限则跳过剩余扫描。

use std::collections::HashMap;
use std::sync::OnceLock;

use regex::Regex;
use serde_json::Value;

use super::{is_switch_on, SecurityFinding, SecuritySettings};
use crate::security::rules::{BuiltinRule, CustomRule};

/// 单次扫描累计字节上限（1 MiB）。超过则跳过后续字符串，不阻断请求。
const MAX_SCAN_BYTES: usize = 1 * 1024 * 1024;

/// rule_id -> 正则。severity/title 等元数据来自 DB（可编辑），正则固定在代码（可测、无 ReDoS 风险）。
static PATTERNS: &[(&str, &str)] = &[
    ("cred.secret_token", r#"(?i)\b(sk-[A-Za-z0-9]{12,}|ghp_[A-Za-z0-9]{20,}|xox[baprs]-[A-Za-z0-9-]{10,}|AKIA[0-9A-Z]{16}|AIza[0-9A-Za-z_-]{35}|eyJ[A-Za-z0-9_-]{10,}\.eyJ[A-Za-z0-9_-]{10,}\.|Bearer\s+[A-Za-z0-9._~+/=-]{16,})"#),
    ("cred.private_key", r#"(?i)-----BEGIN (?:RSA |EC |OPENSSH |DSA |PGP )?PRIVATE KEY-----"#),
    ("cred.named_secret", r#"(?i)(?:password|passwd|secret|token|api[_-]?key|access[_-]?key|auth(?:orization)?|cookie|session)\s*[:=]\s*['"]?[A-Za-z0-9._\-/+=]{8,}"#),
    ("cred.database_url", r#"(?i)\b(?:mysql|postgres(?:ql)?|mongodb|redis|mongodb\+srv)://[^\s'"<>]+"#),
    ("cred.cloud_key", r#"(?i)(?:AKIA[0-9A-Z]{16}|SecretId|AccessKeyId|AIza[0-9A-Za-z_-]{35}|aws_secret_access_key)\b"#),
    ("pii.id_card", r#"(?<![0-9])([1-9][0-9]{5}(?:19|20)[0-9]{2}(?:0[1-9]|1[0-2])(?:0[1-9]|[12][0-9]|3[01])[0-9]{3}[0-9Xx])(?![0-9])"#),
    ("pii.email", r#"[A-Za-z0-9._%+-]+@[A-Za-z0-9.-]+\.[A-Za-z]{2,}"#),
    ("pii.phone", r#"(?<![0-9])1[3-9][0-9]{9}(?![0-9])"#),
    ("pay.credit_card", r#"(?<![0-9])(?:4[0-9]{12}(?:[0-9]{3})?|5[1-5][0-9]{14}|3[47][0-9]{13}|6(?:011|5[0-9]{2})[0-9]{12})(?![0-9])"#),
    // 银行卡：带「非身份证结构」负向预查，规避 62 开头等地区身份证误报。
    ("pay.bank_card", r#"(?<![0-9])(?!\d{6}(?:19|20)\d{2}(?:0[1-9]|1[0-2]))(?:62\d{13,16}|9\d{15,17}|4[0-9]{15}|5[1-5][0-9]{14})(?![0-9])"#),
    ("net.ip_probe", r#"(?i)\b(?:ifconfig\.me|ipinfo\.io|ipify\.org|ip\.co|whatismyip|api\.ipify)\b"#),
    ("net.suspicious_domain", r#"(?i)\b(?:webhook\.site|requestbin\.com|ngrok\.io|pastebin\.com|pipedream\.net|burpcollaborator\.net)\b"#),
    ("net.external_url", r#"https?://[^\s'"<>]+"#),
    ("net.tracking_pixel", r#"(?i)\b(?:1x1|tracking|pixel|beacon|telemetry)\b"#),
    ("exec.shell_command", r#"(?i)\b(?:curl|wget|nc|netcat|scp|ssh|bash\s+-c|sh\s+-c|python\s+-c|powershell\s+-c|cmd\s+/c)\b"#),
    ("exec.exfiltration", r#"(?i)(?:cat\s+.*\|.*curl|curl.*-d\s+@|tar\s+.*\|.*ssh|base64.*\|.*curl|cat\s+/etc/[^\s|]*\|)"#),
    ("exec.remote_script", r#"(?i)(?:curl[^\n]*\|\s*(?:ba)?sh|wget[^\n]*\|\s*(?:ba)?sh|curl.*>\s*/tmp|wget.*-O-)"#),
    ("exec.git_info", r#"(?i)\b(?:git\s+remote|gh\s+auth|git\s+config\s+--list|\.git/config)\b"#),
    ("exec.ssh_key", r#"(?i)\b(?:id_rsa|id_ed25519|id_ecdsa|id_dsa|\.ssh/)\b"#),
    ("prompt.injection", r#"(?i)(?:忽略(?:以上|前面|之前|所有|上述)的?指令|无视(?:系统|先前)提示|忽略(?:所有|上述)规则|disregard|ignore (?:the|all|previous) (?:instruction|prompt|rule)|system prompt|reveal your (?:instruction|prompt)|jailbreak|绕过(?:审计|安全|限制)|越权)"#),
    ("prompt.fingerprint", r#"(?i)(?:fingerprint|浏览器指纹|设备指纹|风控|代理池|user-agent 伪装|canvas 指纹)"#),
];

static COMPILED: OnceLock<HashMap<String, Regex>> = OnceLock::new();

fn compiled() -> &'static HashMap<String, Regex> {
    COMPILED.get_or_init(|| {
        let mut m = HashMap::new();
        for (id, pat) in PATTERNS {
            match Regex::new(pat) {
                Ok(re) => {
                    m.insert((*id).to_string(), re);
                }
                Err(e) => {
                    tracing::error!("安全规则正则编译失败 {}: {}", id, e);
                }
            }
        }
        m
    })
}

/// 高风险类别脱敏所需的正则集合（severity >= High 的规则）。
/// redact 模式在转发前对全树字符串应用这些模式，确保凭证/卡号/外传命令不出本机。
pub fn high_risk_regexes() -> &'static [Regex] {
    static REDACT: OnceLock<Vec<Regex>> = OnceLock::new();
    REDACT.get_or_init(|| {
        let comp = compiled();
        // 与迁移 003 中 severity >= high 的内置规则一一对应。
        let ids = [
            "cred.secret_token",
            "cred.private_key",
            "cred.named_secret",
            "cred.database_url",
            "cred.cloud_key",
            "pay.credit_card",
            "pay.bank_card",
            "net.ip_probe",
            "net.suspicious_domain",
            "net.tracking_pixel",
            "exec.exfiltration",
            "exec.remote_script",
            "exec.ssh_key",
            "prompt.injection",
        ];
        ids.iter()
            .filter_map(|id| comp.get(*id).cloned())
            .collect()
    })
}

/// 扫描结果。
#[derive(Debug, Clone, Default)]
pub struct SecurityScanResult {
    pub findings: Vec<SecurityFinding>,
    pub budget_exceeded: bool,
}

struct ScanCtx<'a> {
    settings: &'a SecuritySettings,
    builtin: &'a [BuiltinRule],
    custom: &'a [CustomRule],
    compiled: &'a HashMap<String, Regex>,
    findings: Vec<SecurityFinding>,
    scanned: usize,
    budget_exceeded: bool,
}

/// 对外入口：扫描整个请求 JSON，返回发现与预算状态。
pub fn scan(
    value: &Value,
    settings: &SecuritySettings,
    builtin: &[BuiltinRule],
    custom: &[CustomRule],
) -> SecurityScanResult {
    let mut ctx = ScanCtx {
        settings,
        builtin,
        custom,
        compiled: compiled(),
        findings: Vec::new(),
        scanned: 0,
        budget_exceeded: false,
    };
    let mut path = String::from("request");
    walk(value, &mut path, &mut ctx);
    SecurityScanResult {
        findings: ctx.findings,
        budget_exceeded: ctx.budget_exceeded,
    }
}

fn walk(value: &Value, path: &mut String, ctx: &mut ScanCtx) {
    if ctx.budget_exceeded {
        return;
    }
    match value {
        Value::String(s) => scan_string(s, path, ctx),
        Value::Array(arr) => {
            for (i, v) in arr.iter().enumerate() {
                let prev = path.len();
                if !path.is_empty() {
                    path.push('[');
                }
                path.push_str(&i.to_string());
                path.push(']');
                walk(v, path, ctx);
                path.truncate(prev);
            }
        }
        Value::Object(map) => {
            for (k, v) in map {
                let prev = path.len();
                if !path.is_empty() {
                    path.push('.');
                }
                path.push_str(k);
                walk(v, path, ctx);
                path.truncate(prev);
            }
        }
        _ => {}
    }
}

fn scan_string(text: &str, path: &str, ctx: &mut ScanCtx) {
    if ctx.scanned + text.len() > MAX_SCAN_BYTES {
        ctx.budget_exceeded = true;
        return;
    }
    ctx.scanned += text.len();

    // 内置规则（受 toggle_key 开关 + enabled 双控）。
    for rule in ctx.builtin {
        if !is_switch_on(ctx.settings, rule.toggle_key.as_deref().unwrap_or("")) {
            continue;
        }
        if let Some(re) = ctx.compiled.get(&rule.rule_id) {
            if let Some(m) = re.find(text) {
                ctx.findings.push(SecurityFinding {
                    rule_id: rule.rule_id.clone(),
                    category: rule.category.clone(),
                    severity: rule.severity.clone(),
                    title: rule.title.clone(),
                    description: rule.description.clone(),
                    location: Some(path.to_string()),
                    evidence_masked: Some(mask_evidence(m.as_str())),
                });
            }
        }
    }

    // 自定义黑名单（子串匹配）。
    let lower = text.to_ascii_lowercase();
    for cr in ctx.custom {
        if cr.rule_type != "blacklist" {
            continue;
        }
        if lower.contains(&cr.pattern.to_ascii_lowercase()) {
            ctx.findings.push(SecurityFinding {
                rule_id: format!("custom.{}", cr.category),
                category: cr.category.clone(),
                severity: cr.severity.clone(),
                title: format!("自定义黑名单: {}", cr.category),
                description: Some(cr.pattern.clone()),
                location: Some(path.to_string()),
                evidence_masked: Some(mask_evidence(&cr.pattern)),
            });
        }
    }
}

/// 脱敏证据：过短直接 ****；否则保留首尾各 2 字符 + ****，既可读又不出明文。
fn mask_evidence(s: &str) -> String {
    let chars: Vec<char> = s.chars().collect();
    if chars.len() <= 6 {
        "****".to_string()
    } else {
        let first: String = chars.iter().take(2).collect();
        let last: String = chars.iter().skip(chars.len() - 2).collect();
        format!("{}****{}", first, last)
    }
}
