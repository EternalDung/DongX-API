pub mod scanner;
pub mod rate_limit;

use serde::{Deserialize, Serialize};

/// Severity levels for security findings
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
#[serde(rename_all = "lowercase")]
pub enum Severity {
    Info,
    Warning,
    Critical,
}

/// A security finding from content scanning
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct SecurityFinding {
    pub rule_id: String,
    pub severity: Severity,
    pub message: String,
    pub matched_text: Option<String>,
}
