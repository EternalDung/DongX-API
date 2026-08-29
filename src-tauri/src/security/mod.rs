// scanner + rate_limit 已实现但当前请求路径尚未接线（限流/内容扫描属 backlog 搁置项）。
// 保留实现、用 dead_code 静音，待后续接线时移除本属性即可。
#![allow(dead_code)]

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
