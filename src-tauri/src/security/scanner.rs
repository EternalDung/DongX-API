use regex::Regex;
use super::{SecurityFinding, Severity};

/// Scan request/response content for sensitive information leakage.
///
/// This runs on every proxied request body and response body to detect
/// patterns like SSNs, credit card numbers, internal IP addresses, etc.
///
/// Java comparison: like a Servlet Filter that inspects request/response,
/// but here it's a pure function returning findings rather than mutating.
pub fn scan_content(text: &str) -> Vec<SecurityFinding> {
    let mut findings = Vec::new();

    // Check for API key patterns
    if let Some(finding) = check_api_keys(text) {
        findings.push(finding);
    }

    // Check for credit card numbers (basic Luhn-pattern check)
    if let Some(finding) = check_credit_cards(text) {
        findings.push(finding);
    }

    // Check for SSN-like patterns (US format XXX-XX-XXXX)
    if let Some(finding) = check_ssn(text) {
        findings.push(finding);
    }

    // Check for internal IP addresses
    if let Some(finding) = check_internal_ips(text) {
        findings.push(finding);
    }

    findings
}

/// Redact sensitive information from a string.
/// Replaces API keys, tokens, passwords with masked values.
pub fn redact_secrets(text: &str) -> String {
    let mut result = text.to_string();

    // Redact sk-* patterns (OpenAI-style API keys)
    redact_pattern(&mut result, r"sk-[a-zA-Z0-9]{20,}", "sk-****REDACTED****");

    // Redact sk-dong-* patterns (gateway keys)
    redact_pattern(&mut result, r"sk-dong-[a-zA-Z0-9_\-]+", "sk-dong-****REDACTED****");

    // Redact Bearer tokens
    redact_pattern(&mut result, r"Bearer\s+[a-zA-Z0-9\-_]+", "Bearer ****REDACTED****");

    // Redact password fields in JSON
    redact_pattern(&mut result, r#""password"\s*:\s*"[^"]*""#, "\"password\":\"****REDACTED****\"");

    // Redact api_key fields in JSON
    redact_pattern(&mut result, r#""api_key"\s*:\s*"[^"]*""#, "\"api_key\":\"****REDACTED****\"");

    result
}

fn redact_pattern(text: &mut String, pattern: &str, replacement: &str) {
    if let Ok(re) = Regex::new(pattern) {
        *text = re.replace_all(text, replacement).to_string();
    }
}

fn check_api_keys(text: &str) -> Option<SecurityFinding> {
    if let Ok(re) = Regex::new(r"sk-[a-zA-Z0-9]{20,}") {
        if re.is_match(text) {
            return Some(SecurityFinding {
                rule_id: "API_KEY_LEAK".into(),
                severity: Severity::Critical,
                message: "Potential API key detected in content".into(),
                matched_text: None,
            });
        }
    }
    None
}

fn check_credit_cards(text: &str) -> Option<SecurityFinding> {
    if let Ok(re) = Regex::new(r"\b\d{4}[\s-]?\d{4}[\s-]?\d{4}[\s-]?\d{4}\b") {
        if re.is_match(text) {
            return Some(SecurityFinding {
                rule_id: "CREDIT_CARD".into(),
                severity: Severity::Critical,
                message: "Potential credit card number detected".into(),
                matched_text: None,
            });
        }
    }
    None
}

fn check_ssn(text: &str) -> Option<SecurityFinding> {
    if let Ok(re) = Regex::new(r"\b\d{3}-\d{2}-\d{4}\b") {
        if re.is_match(text) {
            return Some(SecurityFinding {
                rule_id: "SSN_DETECTED".into(),
                severity: Severity::Critical,
                message: "Potential SSN detected in content".into(),
                matched_text: None,
            });
        }
    }
    None
}

fn check_internal_ips(text: &str) -> Option<SecurityFinding> {
    if let Ok(re) = Regex::new(r"\b(?:10\.|172\.(?:1[6-9]|2\d|3[01])\.|192\.168\.)\d{1,3}\.\d{1,3}\b") {
        if re.is_match(text) {
            return Some(SecurityFinding {
                rule_id: "INTERNAL_IP".into(),
                severity: Severity::Warning,
                message: "Internal IP address detected in content".into(),
                matched_text: None,
            });
        }
    }
    None
}
