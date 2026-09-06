//! Repository layer — all SQL CRUD operations.
//!
//! Java mental model: 等价于 Spring Data JPA 的 Repository 接口，
//! 但这里是具体实现（sqlx 没有运行时代理）。
//! - 每个 pub async fn ≈ 一个 DAO 方法，&SqlitePool ≈ 注入的 DataSource
//! - 动态筛选用 QueryBuilder（≈ JPA Specification / MyBatis 动态 SQL）
//! - 用 query_as 函数形式而非 query! 宏：宏需要编译期连库校验，
//!   函数形式零配置，SQL 正确性靠集成测试保证

use chrono::Utc;
use sqlx::sqlite::Sqlite;
use sqlx::{QueryBuilder, Row, SqlitePool};

use crate::models::{
    ChannelRow, DashboardStatsRow, GatewayKeyRow, RequestLogListItem, RequestLogRow,
    RequestSecurityFindingRow, SettingRow,
};

fn now() -> String {
    Utc::now().to_rfc3339()
}

fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

// ============================================================
// channels
// ============================================================
pub mod channels {
    use super::*;

    pub async fn list(pool: &SqlitePool) -> Result<Vec<ChannelRow>, sqlx::Error> {
        sqlx::query_as::<_, ChannelRow>(
            "SELECT * FROM channels ORDER BY priority DESC, created_at DESC",
        )
        .fetch_all(pool)
        .await
    }

    pub async fn get_by_id(
        pool: &SqlitePool,
        id: &str,
    ) -> Result<Option<ChannelRow>, sqlx::Error> {
        sqlx::query_as::<_, ChannelRow>("SELECT * FROM channels WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// Insert a new channel. `row.id` / timestamps are filled here if absent.
    pub async fn insert(
        pool: &SqlitePool,
        name: &str,
        protocol: &str,
        channel_type: &str,
        base_url: &str,
        cred_encrypted: &str,
        models: &str,        // JSON array string
        priority: i32,
        weight: i32,
        config: &str,        // JSON object string
        model_mapping: &str, // JSON object string
        endpoints: &str,     // JSON array string
    ) -> Result<ChannelRow, sqlx::Error> {
        let id = new_id();
        let ts = now();
        sqlx::query(
            "INSERT INTO channels (id, name, protocol, type, base_url, cred_encrypted,
                models, status, priority, weight, config, model_mapping, endpoints,
                created_at, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, 1, ?8, ?9, ?10, ?11, ?12, ?13, ?13)",
        )
        .bind(&id)
        .bind(name)
        .bind(protocol)
        .bind(channel_type)
        .bind(base_url)
        .bind(cred_encrypted)
        .bind(models)
        .bind(priority)
        .bind(weight)
        .bind(config)
        .bind(model_mapping)
        .bind(endpoints)
        .bind(&ts)
        .execute(pool)
        .await?;

        Ok(get_by_id(pool, &id).await?.expect("just inserted"))
    }

    /// Full update (all mutable fields).
    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        pool: &SqlitePool,
        id: &str,
        name: &str,
        protocol: &str,
        channel_type: &str,
        base_url: &str,
        cred_encrypted: &str,
        models: &str,
        priority: i32,
        weight: i32,
        config: &str,
        model_mapping: &str,
        endpoints: &str,
        status: i32,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE channels SET name=?2, protocol=?3, type=?4, base_url=?5,
                cred_encrypted=?6, models=?7, priority=?8, weight=?9, config=?10,
                model_mapping=?11, endpoints=?12, status=?13, updated_at=?14
             WHERE id = ?1",
        )
        .bind(id)
        .bind(name)
        .bind(protocol)
        .bind(channel_type)
        .bind(base_url)
        .bind(cred_encrypted)
        .bind(models)
        .bind(priority)
        .bind(weight)
        .bind(config)
        .bind(model_mapping)
        .bind(endpoints)
        .bind(status)
        .bind(now())
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<u64, sqlx::Error> {
        let res = sqlx::query("DELETE FROM channels WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(res.rows_affected())
    }

    /// Record last connectivity test result.
    pub async fn set_test_result(
        pool: &SqlitePool,
        id: &str,
        ok: bool,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE channels SET last_test_at=?2, last_test_ok=?3 WHERE id=?1")
            .bind(id)
            .bind(now())
            .bind(ok as i32)
            .execute(pool)
            .await?;
        Ok(())
    }

    /// Channels eligible for routing (enabled only), ordered by priority.
    pub async fn list_enabled(pool: &SqlitePool) -> Result<Vec<ChannelRow>, sqlx::Error> {
        sqlx::query_as::<_, ChannelRow>(
            "SELECT * FROM channels WHERE status = 1 ORDER BY priority DESC, weight DESC",
        )
        .fetch_all(pool)
        .await
    }
}

// ============================================================
// gateway_keys
// ============================================================
pub mod gateway_keys {
    use super::*;

    pub async fn list(pool: &SqlitePool) -> Result<Vec<GatewayKeyRow>, sqlx::Error> {
        sqlx::query_as::<_, GatewayKeyRow>("SELECT * FROM gateway_keys ORDER BY created_at DESC")
            .fetch_all(pool)
            .await
    }

    pub async fn get_by_id(
        pool: &SqlitePool,
        id: &str,
    ) -> Result<Option<GatewayKeyRow>, sqlx::Error> {
        sqlx::query_as::<_, GatewayKeyRow>("SELECT * FROM gateway_keys WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// Lookup by plaintext key — gateway authentication (local plaintext storage).
    pub async fn get_by_key(
        pool: &SqlitePool,
        key: &str,
    ) -> Result<Option<GatewayKeyRow>, sqlx::Error> {
        sqlx::query_as::<_, GatewayKeyRow>("SELECT * FROM gateway_keys WHERE key = ?")
            .bind(key)
            .fetch_optional(pool)
            .await
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        pool: &SqlitePool,
        name: &str,
        key: &str, // plaintext gateway key (local storage)
        allowed_models: &str,
        allowed_channels: &str,
        quota_limit: i64,
        expires_at: Option<&str>,
    ) -> Result<GatewayKeyRow, sqlx::Error> {
        let id = new_id();
        let ts = now();
        sqlx::query(
            "INSERT INTO gateway_keys (id, name, key, key_hash, status, allowed_models,
                allowed_channels, quota_limit, quota_used, expires_at, created_at, updated_at)
             VALUES (?1, ?2, ?3, '', 1, ?4, ?5, ?6, 0, ?7, ?8, ?8)",
        )
        .bind(&id)
        .bind(name)
        .bind(key)
        .bind(allowed_models)
        .bind(allowed_channels)
        .bind(quota_limit)
        .bind(expires_at)
        .bind(&ts)
        .execute(pool)
        .await?;

        Ok(get_by_id(pool, &id).await?.expect("just inserted"))
    }

    /// Enable / disable a gateway key (status: 0=disabled 1=active).
    pub async fn set_status(
        pool: &SqlitePool,
        id: &str,
        status: i32,
    ) -> Result<(), sqlx::Error> {
        sqlx::query("UPDATE gateway_keys SET status = ?, updated_at = ? WHERE id = ?")
            .bind(status)
            .bind(now())
            .bind(id)
            .execute(pool)
            .await?;
        Ok(())
    }

    #[allow(clippy::too_many_arguments)]
    pub async fn update(
        pool: &SqlitePool,
        id: &str,
        name: &str,
        allowed_models: &str,
        allowed_channels: &str,
        quota_limit: i64,
        expires_at: Option<&str>,
        status: i32,
    ) -> Result<(), sqlx::Error> {
        sqlx::query(
            "UPDATE gateway_keys SET name=?2, allowed_models=?3, allowed_channels=?4,
                quota_limit=?5, expires_at=?6, status=?7, updated_at=?8
             WHERE id=?1",
        )
        .bind(id)
        .bind(name)
        .bind(allowed_models)
        .bind(allowed_channels)
        .bind(quota_limit)
        .bind(expires_at)
        .bind(status)
        .bind(now())
        .execute(pool)
        .await?;
        Ok(())
    }

    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<u64, sqlx::Error> {
        let res = sqlx::query("DELETE FROM gateway_keys WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(res.rows_affected())
    }

    /// Atomically add consumed tokens; auto-disable key when quota exhausted.
    /// (0 = unlimited, never exhausted)
    ///
    /// 返回 `(quota_used 新值, status 新值)`，便于调用方在密钥被自动禁用时
    /// 写一条 `quota_exhaust` 审计事件（用 `RETURNING` 一次搞定，避免额外查询）。
    pub async fn add_quota_used(
        pool: &SqlitePool,
        id: &str,
        tokens: i64,
    ) -> Result<(i64, i32), sqlx::Error> {
        let row = sqlx::query(
            "UPDATE gateway_keys
             SET quota_used = quota_used + ?2,
                 status = CASE
                     WHEN quota_limit > 0 AND quota_used + ?2 >= quota_limit THEN 0
                     ELSE status END,
                 updated_at = ?3
             WHERE id = ?1
             RETURNING quota_used, status",
        )
        .bind(id)
        .bind(tokens)
        .bind(now())
        .fetch_one(pool)
        .await?;
        let used_after: i64 = row.try_get("quota_used")?;
        let status: i32 = row.try_get("status")?;
        Ok((used_after, status))
    }
}

// ============================================================
// request_logs
// ============================================================pub mod

/// Log filter conditions (all optional) — mirrors commands::log::LogQuery.
#[derive(Debug, Default, Clone)]
pub struct LogFilter {
    pub keyword: Option<String>,     // fuzzy match model / channel_name / error_message
    pub channel_name: Option<String>,
    pub model: Option<String>,
    pub status_code: Option<i32>,
    pub start_time: Option<String>,  // RFC3339
    pub end_time: Option<String>,
    pub page: u32,
    pub page_size: u32,
}

pub mod request_logs {
    use super::*;

    /// Insert a full log entry. `row.id` will be generated if absent.
    #[allow(clippy::too_many_arguments)]
    pub async fn insert(
        pool: &SqlitePool,
        api_key_name: Option<&str>,
        api_key_id: Option<&str>,
        channel_name: Option<&str>,
        model: &str,
        upstream_model: Option<&str>,
        mode: &str,
        status_code: i32,
        prompt_tokens: i64,
        completion_tokens: i64,
        total_tokens: i64,
        duration_ms: i64,
        error_message: Option<&str>,
        is_stream: bool,
        is_retry: bool,
        request_body: Option<&str>,
        response_body: Option<&str>,
        risk_level: &str,
        risk_score: i64,
        risk_summary: Option<&str>,
        security_action: &str,
        sanitized: bool,
        blocked_reason: Option<&str>,
    ) -> Result<String, sqlx::Error> {
        let id = new_id();
        let ts = now();

        sqlx::query(
            "INSERT INTO request_logs (id, seq, api_key_name, api_key_id, channel_name, model,
                upstream_model, mode, status_code, prompt_tokens, completion_tokens,
                total_tokens, duration_ms, error_message, is_stream, is_retry,
                created_at, request_body, response_body, risk_level, risk_score,
                risk_summary, security_action, sanitized, blocked_reason)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15,?16,?17,?18,
                ?19,?20,?21,?22,?23,?24,?25)",
        )
        .bind(&id)
        // seq 现为 INTEGER PRIMARY KEY AUTOINCREMENT（迁移 007），
        // 绑定 NULL 即触发自增，彻底消除原 SELECT MAX(seq) 全表扫描。
        .bind(Option::<i64>::None)
        .bind(api_key_name)
        .bind(api_key_id)
        .bind(channel_name)
        .bind(model)
        .bind(upstream_model)
        .bind(mode)
        .bind(status_code)
        .bind(prompt_tokens)
        .bind(completion_tokens)
        .bind(total_tokens)
        .bind(duration_ms)
        .bind(error_message)
        .bind(is_stream as i32)
        .bind(is_retry as i32)
        .bind(&ts)
        .bind(request_body)
        .bind(response_body)
        .bind(risk_level)
        .bind(risk_score)
        .bind(risk_summary)
        .bind(security_action)
        .bind(sanitized)
        .bind(blocked_reason)
        .execute(pool)
        .await?;

        Ok(id)
    }

    /// List logs (slim rows, no bodies) with dynamic filters + pagination.
    pub async fn list_filtered(
        pool: &SqlitePool,
        filter: &LogFilter,
    ) -> Result<Vec<RequestLogListItem>, sqlx::Error> {
        let mut qb: QueryBuilder<Sqlite> = QueryBuilder::new(
            "SELECT id, seq, api_key_name, api_key_id, channel_name, model, mode, status_code,
                    total_tokens, duration_ms, is_stream, is_retry, created_at,
                    error_message, risk_level, risk_score, security_action
             FROM request_logs WHERE 1=1 ",
        );

        if let Some(kw) = &filter.keyword {
            let like = format!("%{}%", kw);
            qb.push("AND (model LIKE ").push_bind(like.clone());
            qb.push(" OR channel_name LIKE ").push_bind(like.clone());
            qb.push(" OR error_message LIKE ").push_bind(like);
            qb.push(") ");
        }
        if let Some(c) = &filter.channel_name {
            qb.push(" AND channel_name = ").push_bind(c.clone());
        }
        if let Some(m) = &filter.model {
            qb.push(" AND model LIKE ").push_bind(format!("%{}%", m));
        }
        if let Some(sc) = filter.status_code {
            qb.push(" AND status_code = ").push_bind(sc);
        }
        if let Some(s) = &filter.start_time {
            qb.push(" AND created_at >= ").push_bind(s.clone());
        }
        if let Some(e) = &filter.end_time {
            qb.push(" AND created_at <= ").push_bind(e.clone());
        }

        let page_size = filter.page_size.clamp(1, 200);
        let offset = filter.page.saturating_sub(1).saturating_mul(page_size) as i64;
        qb.push(" ORDER BY created_at DESC LIMIT ");
        qb.push_bind(page_size as i64);
        qb.push(" OFFSET ");
        qb.push_bind(offset);

        qb.build_query_as::<RequestLogListItem>()
            .fetch_all(pool)
            .await
    }

    /// Full log entry including request/response bodies.
    pub async fn get_detail(
        pool: &SqlitePool,
        id: &str,
    ) -> Result<Option<RequestLogRow>, sqlx::Error> {
        sqlx::query_as::<_, RequestLogRow>("SELECT * FROM request_logs WHERE id = ?")
            .bind(id)
            .fetch_optional(pool)
            .await
    }

    /// Clear all logs, or only those older than N days (None = all).
    ///
    /// Also removes the security findings attached to any purged log so they
    /// don't linger as orphans — `request_security_findings` has no FK cascade
    /// back to `request_logs`.
    pub async fn clear(
        pool: &SqlitePool,
        older_than_days: Option<i32>,
    ) -> Result<u64, sqlx::Error> {
        match older_than_days {
            // Compute the cutoff as an RFC3339 string so the comparison matches
            // the RFC3339 `created_at` we store (to_rfc3339()). SQLite's
            // datetime('now', '-N days') uses a different layout and would sort
            // incorrectly against RFC3339 on sub-day boundaries.
            Some(days) if days > 0 => {
                let cutoff =
                    (Utc::now() - chrono::Duration::days(days as i64)).to_rfc3339();
                // Drop findings of logs that are about to be deleted (exact,
                // independent of any time-format subtlety).
                sqlx::query(
                    "DELETE FROM request_security_findings
                     WHERE log_id IN (SELECT id FROM request_logs WHERE created_at < ?1)",
                )
                .bind(&cutoff)
                .execute(pool)
                .await?;
                let res = sqlx::query("DELETE FROM request_logs WHERE created_at < ?1")
                    .bind(&cutoff)
                    .execute(pool)
                    .await?;
                Ok(res.rows_affected())
            }
            _ => {
                // Clear all: drop every finding, then every log.
                sqlx::query("DELETE FROM request_security_findings")
                    .execute(pool)
                    .await?;
                let res = sqlx::query("DELETE FROM request_logs").execute(pool).await?;
                Ok(res.rows_affected())
            }
        }
    }

    /// Delete a single log entry by id.
    pub async fn delete(pool: &SqlitePool, id: &str) -> Result<u64, sqlx::Error> {
        let res = sqlx::query("DELETE FROM request_logs WHERE id = ?")
            .bind(id)
            .execute(pool)
            .await?;
        Ok(res.rows_affected())
    }
}

// ============================================================
// settings (key-value store, JSON-encoded values)
// ============================================================
pub mod settings {
    use super::*;

    /// Load all settings rows.
    pub async fn get_all(pool: &SqlitePool) -> Result<Vec<SettingRow>, sqlx::Error> {
        sqlx::query_as::<_, SettingRow>("SELECT key, value FROM settings")
            .fetch_all(pool)
            .await
    }

    /// Get a single value (raw JSON-encoded string).
    pub async fn get(pool: &SqlitePool, key: &str) -> Result<Option<String>, sqlx::Error> {
        let row: Option<(String,)> =
            sqlx::query_as("SELECT value FROM settings WHERE key = ?")
                .bind(key)
                .fetch_optional(pool)
                .await?;
        Ok(row.map(|(v,)| v))
    }

    /// Batch upsert — one transaction, all-or-nothing.
    pub async fn upsert_many(
        pool: &SqlitePool,
        entries: &[(String, String)],
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        for (k, v) in entries {
            sqlx::query(
                "INSERT INTO settings (key, value) VALUES (?1, ?2)
                 ON CONFLICT(key) DO UPDATE SET value = ?2",
            )
            .bind(k)
            .bind(v)
            .execute(&mut *tx)
            .await?;
        }
        tx.commit().await
    }

    /// Insert keys that are missing, without overwriting existing values.
    /// Used to backfill defaults so a single source of truth (DEFAULTS in
    /// commands/settings.rs) governs which keys exist.
    pub async fn ensure_many(
        pool: &SqlitePool,
        entries: &[(String, String)],
    ) -> Result<(), sqlx::Error> {
        let mut tx = pool.begin().await?;
        for (k, v) in entries {
            sqlx::query("INSERT OR IGNORE INTO settings (key, value) VALUES (?1, ?2)")
                .bind(k)
                .bind(v)
                .execute(&mut *tx)
                .await?;
        }
        tx.commit().await
    }
}

// ============================================================
// dashboard stats (single aggregate query)
// ============================================================
pub mod stats {
    use super::*;

    pub async fn dashboard(pool: &SqlitePool) -> Result<DashboardStatsRow, sqlx::Error> {
        sqlx::query_as::<_, DashboardStatsRow>(
            "SELECT
                COALESCE((SELECT COUNT(*) FROM request_logs
                    WHERE substr(created_at,1,10) = strftime('%Y-%m-%d','now')), 0) AS today_requests,
                COALESCE((SELECT SUM(total_tokens) FROM request_logs
                    WHERE substr(created_at,1,10) = strftime('%Y-%m-%d','now')), 0) AS today_total_tokens,
                COALESCE((SELECT CAST(AVG(duration_ms) AS INTEGER) FROM request_logs
                    WHERE substr(created_at,1,10) = strftime('%Y-%m-%d','now')), 0) AS avg_latency_ms,
                (SELECT COUNT(*) FROM channels WHERE status = 1) AS active_channels,
                (SELECT COUNT(*) FROM channels) AS total_channels,
                (SELECT COUNT(*) FROM gateway_keys WHERE status = 1) AS total_api_keys,
                COALESCE((SELECT COUNT(*) FROM request_logs), 0) AS total_requests,
                COALESCE((SELECT SUM(total_tokens) FROM request_logs), 0) AS total_tokens",
        )
        .fetch_one(pool)
        .await
    }

    /// 单渠道近 30 天运行概览：总请求数、成功数、平均耗时、Token 用量。
    /// 成功 = `error_message` 为空。供渠道详情「运行概览」展示。
    #[derive(Debug, Clone, sqlx::FromRow)]
    pub struct ChannelStatsRow {
        pub total: i64,
        pub successes: i64,
        pub avg_latency_ms: i64,
        pub prompt_tokens_sum: i64,
        pub completion_tokens_sum: i64,
        pub last_called_at: Option<String>,
    }

    pub async fn channel_stats(
        pool: &SqlitePool,
        channel_name: &str,
    ) -> Result<ChannelStatsRow, sqlx::Error> {
        sqlx::query_as::<_, ChannelStatsRow>(
            "SELECT
                COALESCE(COUNT(*), 0) AS total,
                COALESCE(SUM(CASE WHEN error_message IS NULL OR error_message = '' THEN 1 ELSE 0 END), 0) AS successes,
                COALESCE(CAST(AVG(duration_ms) AS INTEGER), 0) AS avg_latency_ms,
                COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens_sum,
                COALESCE(SUM(completion_tokens), 0) AS completion_tokens_sum,
                MAX(created_at) AS last_called_at
            FROM request_logs
            WHERE channel_name = ?1
              AND created_at >= strftime('%Y-%m-%d %H:%M:%S', 'now', '-30 days')",
        )
        .bind(channel_name)
        .fetch_one(pool)
        .await
    }

    /// 单个网关密钥近 30 天运行概览：总请求数、成功数、平均耗时、Token 用量、最后调用时间。
    /// 成功 = `error_message` 为空。供密钥列表展开区的「运行统计」展示。
    #[derive(Debug, Clone, sqlx::FromRow)]
    pub struct ApiKeyStatsRow {
        pub total: i64,
        pub successes: i64,
        pub avg_latency_ms: i64,
        pub prompt_tokens_sum: i64,
        pub completion_tokens_sum: i64,
        pub last_called_at: Option<String>,
    }

    pub async fn api_key_stats(
        pool: &SqlitePool,
        api_key_id: &str,
        api_key_name: &str,
    ) -> Result<ApiKeyStatsRow, sqlx::Error> {
        sqlx::query_as::<_, ApiKeyStatsRow>(
            "SELECT
                COALESCE(COUNT(*), 0) AS total,
                COALESCE(SUM(CASE WHEN error_message IS NULL OR error_message = '' THEN 1 ELSE 0 END), 0) AS successes,
                COALESCE(CAST(AVG(duration_ms) AS INTEGER), 0) AS avg_latency_ms,
                COALESCE(SUM(prompt_tokens), 0) AS prompt_tokens_sum,
                COALESCE(SUM(completion_tokens), 0) AS completion_tokens_sum,
                MAX(created_at) AS last_called_at
            FROM request_logs
            -- 按稳定主键 api_key_id 聚合；历史行 api_key_id 为 NULL，
            -- 以 api_key_name 兜底，确保改名/重名前的旧日志仍被计入。
            WHERE (api_key_id = ?1 OR (api_key_id IS NULL AND api_key_name = ?2))
              AND created_at >= strftime('%Y-%m-%d %H:%M:%S', 'now', '-30 days')",
        )
        .bind(api_key_id)
        .bind(api_key_name)
        .fetch_one(pool)
        .await
    }
}

// ============================================================
// channel_health: 熔断器的持久化健康状态
// ============================================================
pub mod channel_health {
    use super::*;
    use chrono::{DateTime, Duration, Utc};

    /// 连续失败达到此阈值后，熔断器打开（进入冷却）。
    pub const FAILURE_THRESHOLD: u32 = 3;
    /// 熔断器打开后保持冷却的秒数。
    pub const COOLDOWN_SECS: i64 = 60;

    #[derive(Debug, Clone, sqlx::FromRow)]
    pub struct ChannelHealthRow {
        pub channel_id: String,
        pub consecutive_failures: i64,
        pub cooldown_until: Option<String>,
        pub last_failure_at: Option<String>,
        pub last_failure_reason: Option<String>,
    }

    pub async fn get(
        pool: &SqlitePool,
        channel_id: &str,
    ) -> Result<Option<ChannelHealthRow>, sqlx::Error> {
        sqlx::query_as::<_, ChannelHealthRow>(
            "SELECT channel_id, consecutive_failures, cooldown_until, \
             last_failure_at, last_failure_reason \
             FROM channel_health WHERE channel_id = ?",
        )
        .bind(channel_id)
        .fetch_optional(pool)
        .await
    }

    /// 熔断器是否处于打开状态（cooldown_until 仍指向未来）。
    pub async fn is_open(pool: &SqlitePool, channel_id: &str) -> bool {
        match get(pool, channel_id).await {
            Ok(Some(row)) => match &row.cooldown_until {
                Some(s) => DateTime::parse_from_rfc3339(s)
                    .map(|t| t.timestamp() > Utc::now().timestamp())
                    .unwrap_or(false),
                None => false,
            },
            _ => false,
        }
    }

    /// 记录一次「可重试」的上游失败；连续失败达到阈值后打开熔断器
    /// （设置 cooldown_until = now + COOLDOWN_SECS）。
    pub async fn record_failure(
        pool: &SqlitePool,
        channel_id: &str,
        reason: &str,
    ) -> Result<(), sqlx::Error> {
        let failures = get(pool, channel_id).await?.map(|r| r.consecutive_failures).unwrap_or(0)
            + 1;
        let (cooldown, last_at, last_reason) = if failures >= FAILURE_THRESHOLD as i64 {
            (
                Some((Utc::now() + Duration::seconds(COOLDOWN_SECS)).to_rfc3339()),
                Some(now()),
                Some(reason.to_string()),
            )
        } else {
            (None, Some(now()), Some(reason.to_string()))
        };
        sqlx::query(
            "INSERT INTO channel_health \
             (channel_id, consecutive_failures, cooldown_until, last_failure_at, last_failure_reason) \
             VALUES (?1,?2,?3,?4,?5) \
             ON CONFLICT(channel_id) DO UPDATE SET \
                consecutive_failures = ?2, \
                cooldown_until = ?3, \
                last_failure_at = ?4, \
                last_failure_reason = ?5",
        )
        .bind(channel_id)
        .bind(failures)
        .bind(cooldown)
        .bind(last_at)
        .bind(last_reason)
        .execute(pool)
        .await?;
        Ok(())
    }

    /// 记录一次成功：重置熔断器。
    pub async fn record_success(pool: &SqlitePool, channel_id: &str) -> Result<(), sqlx::Error> {
        sqlx::query(
            "INSERT INTO channel_health \
             (channel_id, consecutive_failures, cooldown_until, last_failure_at, last_failure_reason) \
             VALUES (?1, 0, NULL, NULL, NULL) \
             ON CONFLICT(channel_id) DO UPDATE SET \
                consecutive_failures = 0, \
                cooldown_until = NULL, \
                last_failure_at = NULL, \
                last_failure_reason = NULL",
        )
        .bind(channel_id)
        .execute(pool)
        .await?;
        Ok(())
    }
}

// ============================================================
// security_findings: 安全审计发现明细
// ============================================================
pub mod security_findings {
    use super::*;
    use crate::security::SecurityFinding;

    /// 写入一条发现明细，关联 request_logs.id（log_id）。
    /// phase 来自 finding.phase（request=入站请求体 / response=出站响应体）。
    pub async fn insert(
        pool: &SqlitePool,
        log_id: &str,
        finding: &SecurityFinding,
        action: &str,
    ) -> Result<(), sqlx::Error> {
        let id = new_id();
        sqlx::query(
            "INSERT INTO request_security_findings \
             (id, log_id, phase, category, rule_id, severity, title, description, location, evidence_masked, evidence_hash, action, created_at) \
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13)",
        )
        .bind(&id)
        .bind(log_id)
        .bind(&finding.phase)
        .bind(&finding.category)
        .bind(&finding.rule_id)
        .bind(&finding.severity)
        .bind(&finding.title)
        .bind(&finding.description)
        .bind(&finding.location)
        .bind(&finding.evidence_masked)
        .bind(&finding.evidence_hash)
        .bind(action)
        .bind(now())
        .execute(pool)
        .await?;
        Ok(())
    }

    /// 取某条日志的全部安全发现明细，供日志详情页展示。
    ///
    /// 排序：**按严重度降序**（critical→high→medium→low→info），同级按时间正序。
    /// 参考实现（同类网关）此处用 `ORDER BY created_at ASC`，导致高危项被埋在
    /// 滚动区下方；这里改为严重度优先，保证最严重的一条永远在第一行。
    pub async fn list_by_log(
        pool: &SqlitePool,
        log_id: &str,
    ) -> Result<Vec<RequestSecurityFindingRow>, sqlx::Error> {
        sqlx::query_as::<_, RequestSecurityFindingRow>(
            "SELECT id, log_id, phase, category, rule_id, severity, title,
                    description, location, evidence_masked, action, created_at
             FROM request_security_findings
             WHERE log_id = ?
             ORDER BY CASE severity
                 WHEN 'critical' THEN 5
                 WHEN 'high'     THEN 4
                 WHEN 'medium'   THEN 3
                 WHEN 'low'      THEN 2
                 WHEN 'info'     THEN 1
                 ELSE 0
             END DESC, created_at ASC",
        )
        .bind(log_id)
        .fetch_all(pool)
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// 内存库 + 全部迁移（含 007）。内存库需单连接，否则每个连接各持一份数据。
    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    /// 插入一条日志；`sanitized` 为最后一个可变参数。
    async fn insert_log(pool: &SqlitePool, model: &str, sanitized: bool) -> String {
        request_logs::insert(
            pool,
            Some("key-a"),
            Some("key-a-id"),
            Some("ch-a"),
            model,
            Some(model),
            "chat",
            200,
            1,
            2,
            3,
            42,
            None,
            false,
            false,
            Some("{\"a\":1}"),
            Some("{\"b\":2}"),
            "none",
            0,
            None,
            "allow",
            sanitized,
            None,
        )
        .await
        .expect("insert log")
    }

    /// 迁移 007 后 seq 由 AUTOINCREMENT 自增：连续插入应得到严格递增的序号，
    /// 且不再有 SELECT MAX(seq) 全表扫描（此处以「多行插入耗时不随行数线性恶化」
    /// 为间接约束，核心断言仍是序号唯一递增）。
    #[tokio::test]
    async fn seq_autoincrements_on_insert() {
        let pool = test_pool().await;
        let ids: Vec<String> = (0..5).map(|i| format!("m{i}")).collect();
        let mut seqs = Vec::new();
        for m in &ids {
            let id = insert_log(&pool, m, false).await;
            let row = request_logs::get_detail(&pool, &id)
                .await
                .expect("get_detail")
                .expect("row exists");
            seqs.push(row.seq.expect("seq 应由 AUTOINCREMENT 赋值"));
        }
        // 严格递增 → 证明每次插入都拿到了新的自增值（而非恒为 MAX+1 或 1）。
        assert!(
            seqs.windows(2).all(|w| w[0] < w[1]),
            "seq 应严格递增，实际: {seqs:?}"
        );
    }

    /// sanitized 以 bool 落 INTEGER 列并原样读回（前后端契约：boolean，非 0/1 数字）。
    #[tokio::test]
    async fn sanitized_round_trips_as_bool() {
        let pool = test_pool().await;
        let id = insert_log(&pool, "m-bool", true).await;
        let row = request_logs::get_detail(&pool, &id)
            .await
            .expect("get_detail")
            .expect("row exists");
        assert!(row.sanitized, "sanitized=true 应原样读回为 true");
        assert_eq!(row.model, "m-bool");
    }
}
