-- 021: request_logs 增加 cached_tokens 列，用于模型调用统计「缓存命中」维度
-- 注意：已应用的迁移（001~020）禁止再编辑（sqlx SHA-384 校验）。
-- 缓存命中数来自上游响应 usage.cached_tokens / cache_read_input_tokens。
ALTER TABLE request_logs ADD COLUMN cached_tokens INTEGER NOT NULL DEFAULT 0;
