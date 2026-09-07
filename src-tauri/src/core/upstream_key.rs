//! 上游 API key 的「解密 + 加权挑选」**单一实现**。
//!
//! 此前 `rag::ask` / `wiki::ask` 的 `decrypt_pick_upstream_key` 与
//! `core::dispatcher` 的 `pick_upstream_key` 是三份逐行相同的拷贝，
//! 且共享同一个缺陷（见下）。现收敛到此处，三处调用点一律改用本函数，
//! 禁止再复制。
//!
//! ## 存储形态
//! 渠道 `cred_encrypted` 解密后的明文有两种形态：
//! - **多 key（当前形态）**：`[{"key":"sk-...","weight":7}, ...]`
//! - **legacy 单 key**：明文即密钥本身，如 `sk-legacy-123`
//!
//! ## 既定语义（已固化进单测）
//! - 明文能解析成 JSON **数组**时，就**只**按数组语义处理，**不再回退 legacy**。
//!   旧实现在「数组为空」或「数组内所有 key 均为空串」时 pairs 为空，会掉到
//!   legacy 分支把 `[]` / `[{"key":"","weight":1}]` 这段 JSON 原文当成密钥返回，
//!   最终拼出非法的 `Authorization` 头，且上游报错极难定位。现在直接报错。
//!   API key 不可能以 `[` 开头且被解析成合法 JSON 数组，因此该判据不会误伤
//!   legacy 单 key。
//! - key 先 `trim`，trim 后为空的条目直接跳过（空白 key 没有任何意义）。
//! - `weight` 缺失或非数字按 1；超出 i32 范围做**饱和截断**——旧实现直接
//!   `as i32` 会静默回绕，超大权重可能变成负数。
//! - 明文为空（解密后空串）→ 报错，不返回空密钥。
//! - 解密失败 → 原样上抛 `AppError::Crypto`，不吞掉原因。

use serde_json::Value;

use crate::core::weighted::weighted_pick;
use crate::crypto;
use crate::error::{AppError, AppResult};

/// 解密渠道凭据并加权随机挑选一条上游 key。
pub fn pick_upstream_key(cred_encrypted: &str) -> AppResult<String> {
    let plaintext = crypto::decrypt(cred_encrypted)?;

    // 多 key 数组形态：命中即只走数组语义，pairs 为空直接报错（不再回退）。
    if let Ok(keys) = serde_json::from_str::<Vec<Value>>(&plaintext) {
        let pairs: Vec<(String, i32)> = keys
            .iter()
            .filter_map(|k| {
                let key = k.get("key")?.as_str()?.trim().to_string();
                if key.is_empty() {
                    return None;
                }
                let weight = k
                    .get("weight")
                    .and_then(|v| v.as_i64())
                    .unwrap_or(1)
                    .clamp(i32::MIN as i64, i32::MAX as i64) as i32;
                Some((key, weight))
            })
            .collect();
        return weighted_pick(&pairs)
            .ok_or_else(|| AppError::Crypto("渠道未配置任何可用的上游密钥".into()));
    }

    // legacy 单 key 形态：原样返回（不 trim，与历史行为一致）。
    if plaintext.is_empty() {
        return Err(AppError::Crypto("上游密钥为空".into()));
    }
    Ok(plaintext)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    /// 走真实加解密往返：依赖 crypto 的硬编码占位密钥，无 DB / 无网络。
    fn enc(s: &str) -> String {
        crypto::encrypt(s).expect("encrypt 不应失败")
    }

    #[test]
    fn legacy_single_key_is_returned_as_is() {
        assert_eq!(
            pick_upstream_key(&enc("sk-legacy-123")).unwrap(),
            "sk-legacy-123"
        );
    }

    #[test]
    fn multi_key_picks_a_member() {
        let payload = r#"[{"key":"sk-a","weight":1},{"key":"sk-b","weight":1}]"#;
        let got = pick_upstream_key(&enc(payload)).unwrap();
        assert!(
            got == "sk-a" || got == "sk-b",
            "应从数组内挑一条，实际: {got}"
        );
    }

    #[test]
    fn multi_key_respects_weight() {
        let payload = r#"[{"key":"sk-a","weight":1},{"key":"sk-b","weight":1000}]"#;
        let mut a = 0usize;
        for _ in 0..500 {
            if pick_upstream_key(&enc(payload)).unwrap() == "sk-a" {
                a += 1;
            }
        }
        // 期望约 0.5 次，阈值 10 极其宽松，确保不会 flaky
        assert!(a < 10, "sk-a 命中 {a} 次，权重 1:1000 下不应如此频繁");
    }

    #[test]
    fn missing_weight_defaults_to_one() {
        let payload = r#"[{"key":"sk-a"},{"key":"sk-b"}]"#;
        let mut seen = HashSet::new();
        for _ in 0..500 {
            seen.insert(pick_upstream_key(&enc(payload)).unwrap());
        }
        assert_eq!(
            seen,
            HashSet::from(["sk-a".to_string(), "sk-b".to_string()])
        );
    }

    #[test]
    fn blank_keys_are_skipped_when_others_exist() {
        let payload = r#"[{"key":"  ","weight":9},{"key":"sk-real","weight":1}]"#;
        for _ in 0..50 {
            assert_eq!(pick_upstream_key(&enc(payload)).unwrap(), "sk-real");
        }
    }

    #[test]
    fn empty_plaintext_is_error() {
        assert!(pick_upstream_key(&enc("")).is_err());
    }

    #[test]
    fn invalid_ciphertext_is_error() {
        assert!(pick_upstream_key("not-valid-base64-ciphertext").is_err());
    }

    #[test]
    fn all_keys_empty_is_error_not_raw_json() {
        // 回归：旧实现会把 JSON 原文当密钥返回，拼出非法 Authorization 头。
        let err = pick_upstream_key(&enc(r#"[{"key":"","weight":1}]"#)).unwrap_err();
        assert!(
            !err.to_string().contains('['),
            "不应再返回原始 JSON 文本，实际: {err}"
        );
    }

    #[test]
    fn empty_array_is_error_not_raw_json() {
        let err = pick_upstream_key(&enc("[]")).unwrap_err();
        assert!(
            !err.to_string().contains("[]"),
            "不应再返回原始 JSON 文本，实际: {err}"
        );
    }

    #[test]
    fn array_of_non_objects_is_error() {
        // `["sk-a"]` 不是合法的存储形态：元素不是 {key, weight} 对象。
        assert!(pick_upstream_key(&enc(r#"["sk-a"]"#)).is_err());
    }

    #[test]
    fn huge_weight_does_not_wrap_negative() {
        // 回归：旧实现 `as i32` 会静默回绕，超大权重可能变负数。
        let payload = r#"[{"key":"sk-huge","weight":99999999999},{"key":"sk-other","weight":1}]"#;
        let mut huge = 0usize;
        for _ in 0..200 {
            if pick_upstream_key(&enc(payload)).unwrap() == "sk-huge" {
                huge += 1;
            }
        }
        // 饱和截断后权重为 i32::MAX，应几乎每次都命中 huge
        assert!(huge > 190, "huge 命中 {huge}/200 次，不应被回绕成负数");
    }
}
