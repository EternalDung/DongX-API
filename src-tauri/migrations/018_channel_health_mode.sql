-- 熔断粒度从「渠道级」细化为「渠道 × 流式/非流式」两档（方案 A）。
-- SQLite 不支持 ALTER PRIMARY KEY，故新建表 + 拷贝历史行 + 改名。
CREATE TABLE channel_health_v2 (
    channel_id           TEXT    NOT NULL,
    mode                 TEXT    NOT NULL,
    consecutive_failures INTEGER NOT NULL DEFAULT 0,
    cooldown_until       TEXT,
    last_failure_at      TEXT,
    last_failure_reason  TEXT,
    PRIMARY KEY (channel_id, mode)
);

-- 历史渠道级健康行映射为 nonstream 档（历史上只有非流式路径），
-- stream 档从该渠道首次在流式请求中失败时从头累计。
INSERT INTO channel_health_v2 (
    channel_id, mode, consecutive_failures, cooldown_until,
    last_failure_at, last_failure_reason
)
SELECT
    channel_id, 'nonstream', consecutive_failures, cooldown_until,
    last_failure_at, last_failure_reason
FROM channel_health;

DROP TABLE channel_health;

ALTER TABLE channel_health_v2 RENAME TO channel_health;

CREATE INDEX IF NOT EXISTS idx_channel_health_mode
    ON channel_health (channel_id, mode, cooldown_until);
