use std::collections::HashSet;

use sqlx::SqlitePool;

use crate::core::dispatcher::{self, DispatchContext, SelectedChannel};
use crate::error::{AppError, AppResult};

/// 单次请求内的「故障转移」状态机，参照 waliapi 的 `AttemptFlow`。
///
/// 状态流转（每次 `next()` 推进一环）：
///
/// ```text
///   Select ──候选渠道池──▶ Attempt(forward 上游)
///                           │
///               ┌───────────┼──────────────────┐
///           Success      Retryable        Terminal(400/422/401/403)
///              │             │                  │
///           Done      record+回到 Select    Done(原样返回上游错误，不重试)
///                      (consume 一次重试)
///   候选耗尽 / 重试次数耗尽 ──▶ Exhausted
/// ```
///
/// 与 waliapi 的差异：DongX 只做「渠道级」故障转移（waliapi 也是渠道级），
/// 不跨协议组降级（DongX 当前没有协议优先级概念）。熔断判定复用持久化的
/// `channel_health` 表（与 waliapi 的 `channel_mode_health` 同思路）。
pub struct Failover {
    pool: SqlitePool,
    ctx: DispatchContext,
    /// 本次请求中已经尝试过的渠道 id（避免重复选中同一个渠道）。
    tried: HashSet<String>,
    /// 最大尝试次数（含首次）。`retry_enabled=false` 时为 1。
    max_attempts: usize,
    /// 已发起的尝试次数。
    attempts: usize,
    /// 最近一次上游失败结果，供耗尽时回退给客户端。
    last_outcome: Option<Outcome>,
}

/// 一次上游尝试的结果（用于错误响应与审计标记）。
#[derive(Debug, Clone)]
pub struct Outcome {
    pub status: u16,
    pub code: String,
    pub message: String,
    /// 是否「可重试」故障（连接/超时/5xx/429/408/409 → true；4xx 客户端/鉴权错误 → false）。
    pub retryable: bool,
}

impl Outcome {
    /// 上游返回了 HTTP 状态（无论成功与否，调用方按 `retryable` 判断）。
    pub fn upstream(status: u16, message: String, retryable: bool) -> Self {
        Outcome {
            status,
            code: "upstream_error".into(),
            message,
            retryable,
        }
    }
    /// 连接失败 / 超时（无 HTTP 状态）→ 一律视为可重试。
    pub fn connection(message: String) -> Self {
        Outcome {
            status: 502,
            code: "upstream_error".into(),
            message,
            retryable: true,
        }
    }
    /// 一开始就没有可用渠道（模型无人服务 / 全部冷却）。
    pub fn no_channel(message: String) -> Self {
        Outcome {
            status: 503,
            code: "no_channel".into(),
            message,
            retryable: false,
        }
    }
}

/// `next()` 的返回：本次应尝试的渠道，或已无候选。
pub enum Step {
    /// 选出下一个候选渠道（已排除本次已尝试 + 熔断冷却中的渠道）。
    Try(SelectedChannel),
    /// 全部候选已耗尽（都试过 / 全在冷却中）。
    Exhausted,
    /// 一开始就没有可用渠道（模型无人服务）。
    NoChannel(String),
}

impl Failover {
    pub fn new(pool: SqlitePool, ctx: DispatchContext, max_attempts: usize) -> Self {
        Failover {
            pool,
            ctx,
            tried: HashSet::new(),
            max_attempts: max_attempts.max(1),
            attempts: 0,
            last_outcome: None,
        }
    }

    /// 取出下一个要尝试的渠道。每调用一次推进一次：
    /// 重新查询候选池 → 剔除本次已尝试 + 熔断冷却中的渠道 →
    /// 取最高优先级组加权随机选 1 个 → 记入 `tried`。
    ///
    /// 返回 `Err` 仅当 `pick_one` 内部失败（如解密 key 出错），此时交由
    /// 调用方按 500 处理；「无候选」走 `Ok(Step::NoChannel/Exhausted)`。
    pub async fn next(&mut self) -> AppResult<Step> {
        self.attempts += 1;
        match dispatcher::candidate_channels(&self.pool, &self.ctx, &self.tried).await {
            Ok(cands) => {
                let selected = dispatcher::pick_one(&cands)?;
                self.tried.insert(selected.id.clone());
                Ok(Step::Try(selected))
            }
            Err(e) => {
                if self.tried.is_empty() {
                    Ok(Step::NoChannel(e.to_string()))
                } else {
                    Ok(Step::Exhausted)
                }
            }
        }
    }

    /// 反馈一次尝试的结果（成功或失败），更新 `last_outcome`。
    pub fn observe(&mut self, outcome: Outcome) {
        self.last_outcome = Some(outcome);
    }

    /// 该失败是否还应继续换渠道重试：尚未达到最大尝试次数。
    /// （是否「可重试」由调用方在 `observe` 前判断：不可重试的错误不会走到这里。）
    pub fn should_retry(&self) -> bool {
        self.attempts < self.max_attempts
    }

    /// 当前尝试是否处于「重试」（第 2 次及以后），用于审计标记 `is_retry`。
    pub fn is_retry(&self) -> bool {
        self.attempts > 1
    }

    /// 最近一次上游结果（耗尽时回退给客户端）。
    pub fn last_outcome(&self) -> Option<&Outcome> {
        self.last_outcome.as_ref()
    }
}

/// 把 `AppError` 转成人类可读文本（供 `NoChannel` 消息使用）。
impl From<AppError> for Outcome {
    fn from(e: AppError) -> Self {
        Outcome::no_channel(e.to_string())
    }
}
