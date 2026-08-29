//! 高风险脱敏转发体。
//!
//! 在「脱敏」模式下，对请求 JSON 全树的字符串叶子应用高风险类别正则
//! （凭证/卡号/外传命令/可疑域名/提示注入），命中的明文替换为占位符，
//! 返回用于转发上游的脱敏副本。日志体始终走脱敏（见 handler 的 sanitized 逻辑）。

use serde_json::Value;

use crate::security::scanner::high_risk_regexes;

const MASK: &str = "[REDACTED]";

/// 返回脱敏后的 JSON 副本：所有命中高风险模式的字符串被替换为 [REDACTED]。
pub fn redact(value: &Value) -> Value {
    match value {
        Value::String(s) => {
            let mut out = s.clone();
            for re in high_risk_regexes() {
                out = re.replace_all(&out, MASK).into_owned();
            }
            Value::String(out)
        }
        Value::Array(arr) => Value::Array(arr.iter().map(redact).collect()),
        Value::Object(map) => {
            Value::Object(map.iter().map(|(k, v)| (k.clone(), redact(v))).collect())
        }
        other => other.clone(),
    }
}
