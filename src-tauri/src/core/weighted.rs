//! 加权随机挑选的**单一实现**。
//!
//! 此前 `rag::ask`、`core::dispatcher`、`wiki::ask` 各维护一份 `weighted_pick`，
//! 且语义互不一致：
//! - `rag::ask` / `core::dispatcher`：clamp 到 ≥1，但 dispatcher 版对空列表会
//!   `pairs[0]` 越界 panic；
//! - `wiki::ask`：clamp 到 ≥0（0 权重被完全排除），且用
//!   `rand::random::<u32>() % total` 取模，存在取模偏置。
//!
//! 统一收敛到此处，调用方只保留「业务类型 -> (String, i32)」的转换。
//!
//! ## 既定语义（已固化进单测）
//! - **空列表 -> `None`**：调用方负责转成各自的业务错误，函数本身不 panic。
//! - **权重 clamp 到 ≥1**：0 与负数按 1 处理。前端权重输入为
//!   `min={1}` 且 `Number(..) || 1`，0 只可能来自历史数据或手工改库，
//!   不视为「禁用该条」。
//! - **用 `gen_range` 而非取模**：避免 `2^32 % total != 0` 带来的取模偏置。
//! - **求和用 i64**：旧实现用 i32 求和，多条大权重相加会溢出（debug 下 panic）。

use rand::Rng;

/// 按（id, weight）列表做加权随机挑选，weight clamp 到 ≥1；空列表返回 `None`。
pub fn weighted_pick(pairs: &[(String, i32)]) -> Option<String> {
    if pairs.is_empty() {
        return None;
    }
    let total: i64 = pairs.iter().map(|(_, w)| (*w).max(1) as i64).sum();
    if total <= 0 {
        return None;
    }
    let mut r = rand::thread_rng().gen_range(0..total);
    for (id, w) in pairs {
        r -= (*w).max(1) as i64;
        if r < 0 {
            return Some(id.clone());
        }
    }
    // 理论上不可达：r ∈ [0, total) 且权重之和 == total，循环内必然提前返回。
    pairs.last().map(|(id, _)| id.clone())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn pair(id: &str, w: i32) -> (String, i32) {
        (id.to_string(), w)
    }

    #[test]
    fn empty_returns_none_without_panic() {
        // 回归：dispatcher 的旧实现在这里会 pairs[0] 越界 panic。
        assert_eq!(weighted_pick(&[]), None);
    }

    #[test]
    fn single_pair_is_always_returned() {
        for _ in 0..20 {
            assert_eq!(weighted_pick(&[pair("only", 1)]).as_deref(), Some("only"));
        }
    }

    #[test]
    fn zero_and_negative_weights_clamp_to_one() {
        let pairs = vec![pair("a", 0), pair("b", -5), pair("c", 1)];
        let mut counts: HashMap<String, usize> = HashMap::new();
        for _ in 0..3000 {
            *counts.entry(weighted_pick(&pairs).unwrap()).or_insert(0) += 1;
        }
        // 理论各 1000，sigma 约 26，正负 250 约 9 sigma
        for id in ["a", "b", "c"] {
            let n = *counts.get(id).unwrap_or(&0);
            assert!((750..=1250).contains(&n), "{id} 命中 {n} 次，应在 750~1250");
        }
    }

    #[test]
    fn zero_weight_is_selectable_not_excluded() {
        // 语义统一点：wiki 旧实现用 max(0) 会把 0 权重完全排除；
        // 统一后 0 按 1 处理，与 rag / dispatcher 一致。
        let pairs = vec![pair("zero", 0), pair("one", 1)];
        let mut zero = 0usize;
        for _ in 0..2000 {
            if weighted_pick(&pairs).unwrap() == "zero" {
                zero += 1;
            }
        }
        // 期望 1000，sigma 约 22，正负 200 约 9 sigma
        assert!(
            (800..=1200).contains(&zero),
            "0 权重应按 1 处理（各占一半），实际命中 {zero} 次"
        );
    }

    #[test]
    fn respects_weight_ratio() {
        let pairs = vec![pair("light", 1), pair("heavy", 3)];
        let mut light = 0usize;
        for _ in 0..4000 {
            if weighted_pick(&pairs).unwrap() == "light" {
                light += 1;
            }
        }
        // 期望 1000，sigma 约 27，正负 300 约 11 sigma
        assert!(
            (700..=1300).contains(&light),
            "light 命中 {light} 次，应在 700~1300"
        );
    }

    #[test]
    fn extreme_weight_ratio_holds() {
        let pairs = vec![pair("rare", 1), pair("common", 1000)];
        let mut rare = 0usize;
        for _ in 0..2000 {
            if weighted_pick(&pairs).unwrap() == "rare" {
                rare += 1;
            }
        }
        // 期望约 2 次，阈值 25 足够宽松，确保不 flaky
        assert!(rare < 25, "rare 命中 {rare} 次，权重 1:1000 下不应如此频繁");
    }

    #[test]
    fn never_returns_none_or_unknown_id() {
        let pairs = vec![pair("a", 2), pair("b", 5), pair("c", 1)];
        let valid = ["a", "b", "c"];
        for _ in 0..500 {
            let got = weighted_pick(&pairs).expect("非空输入不应返回 None");
            assert!(valid.contains(&got.as_str()), "返回了未知 id: {got}");
        }
    }

    #[test]
    fn large_weights_do_not_overflow() {
        // 回归：旧实现用 i32 求和，三条 i32::MAX/2 相加即溢出（debug 下 panic）。
        let pairs = vec![
            pair("a", i32::MAX / 2),
            pair("b", i32::MAX / 2),
            pair("c", i32::MAX / 2),
        ];
        let valid = ["a", "b", "c"];
        for _ in 0..50 {
            let got = weighted_pick(&pairs).expect("大权重下不应返回 None");
            assert!(valid.contains(&got.as_str()), "返回了未知 id: {got}");
        }
    }

    #[test]
    fn all_zero_weights_still_select_one() {
        // 全 0（或全负）时 clamp 后 total > 0，应能正常选出一个，而不是返回 None。
        let pairs = vec![pair("x", 0), pair("y", -3)];
        let got = weighted_pick(&pairs);
        assert!(
            matches!(got.as_deref(), Some("x") | Some("y")),
            "实际: {got:?}"
        );
    }
}
