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

/// 对原始文本（如流式 SSE 累积体）做同等高风险脱敏，返回脱敏后字符串。
/// 用于响应体日志脱敏——非 JSON 的 SSE 文本也能掩掉 sk-/AKIA/JWT 等明文。
pub fn redact_text(text: &str) -> String {
    let mut out = text.to_string();
    for re in high_risk_regexes() {
        out = re.replace_all(&out, MASK).into_owned();
    }
    out
}

#[cfg(test)]
mod tests {
    use super::redact;
    use serde_json::json;

    #[test]
    fn masks_secret_in_string() {
        let v = json!({"key":"sk-abcdefghijklmnopqrstuvwxyz"});
        let out = redact(&v);
        let s = out["key"].as_str().unwrap();
        assert!(s.contains("[REDACTED]"));
        assert!(!s.contains("sk-abcdefghijklmnopqrstuvwxyz"));
    }

    #[test]
    fn leaves_benign_intact() {
        let v = json!({"name":"alice","age":30,"ok":true});
        let out = redact(&v);
        assert_eq!(out["name"].as_str().unwrap(), "alice");
        assert_eq!(out["age"].as_i64().unwrap(), 30);
        assert_eq!(out["ok"].as_bool().unwrap(), true);
    }

    #[test]
    fn masks_nested_object_and_array() {
        let v = json!({
            "user": {"token": "AKIAABCDEFGHIJKLMNOP"},
            "list": ["plain", "ghp_abcdefghijklmnopqrstuvwxyz"]
        });
        let out = redact(&v);
        assert!(out["user"]["token"]
            .as_str()
            .unwrap()
            .contains("[REDACTED]"));
        assert_eq!(out["list"][0].as_str().unwrap(), "plain");
        assert!(out["list"][1].as_str().unwrap().contains("[REDACTED]"));
    }

    #[test]
    fn does_not_alter_structure() {
        let v = json!({"a":1,"b":[1,2,3],"c":{"d":"e"}});
        let out = redact(&v);
        // 无高风险命中，结构与值应保持完全一致。
        assert_eq!(out, v);
    }
}
