-- 007: request_logs.seq 改为 INTEGER PRIMARY KEY AUTOINCREMENT。
-- 消除每次插入时的「SELECT MAX(seq)」全表扫描；同时保留 id 为 UNIQUE
-- （仍被 request_security_findings.log_id 引用，不可丢失唯一性）。
-- 注意：迁移文件一经应用不得再编辑（sqlx 按字节校验 checksum）。

CREATE TABLE request_logs_new (
  seq               INTEGER PRIMARY KEY AUTOINCREMENT,
  id                TEXT NOT NULL UNIQUE,
  api_key_name      TEXT,
  channel_name      TEXT,
  model             TEXT NOT NULL,
  upstream_model    TEXT,
  mode              TEXT NOT NULL,             -- chat | completion | embedding | other
  status_code       INTEGER NOT NULL,
  prompt_tokens     INTEGER NOT NULL DEFAULT 0,
  completion_tokens INTEGER NOT NULL DEFAULT 0,
  total_tokens      INTEGER NOT NULL DEFAULT 0,
  duration_ms       INTEGER NOT NULL,
  error_message     TEXT,
  is_stream         INTEGER NOT NULL DEFAULT 0,
  is_retry          INTEGER NOT NULL DEFAULT 0,
  created_at        TEXT NOT NULL,
  request_body      TEXT,
  response_body     TEXT,
  -- security audit fields
  risk_level        TEXT NOT NULL DEFAULT 'none',
  risk_score        INTEGER NOT NULL DEFAULT 0,
  risk_summary      TEXT,
  security_action   TEXT NOT NULL DEFAULT 'allow',
  sanitized         INTEGER NOT NULL DEFAULT 0,
  blocked_reason    TEXT
);

-- 拷贝历史数据：seq 不拷贝，由 AUTOINCREMENT 按原顺序重新编号。
-- 理由：旧 seq 由「SELECT MAX(seq)+1」生成，并发插入下可能重复；直接拷进主键列
-- 会因唯一约束冲突导致迁移失败。重新编号既保证唯一单调，又保留原有先后顺序。
INSERT INTO request_logs_new (
    id, api_key_name, channel_name, model, upstream_model, mode,
    status_code, prompt_tokens, completion_tokens, total_tokens, duration_ms,
    error_message, is_stream, is_retry, created_at, request_body, response_body,
    risk_level, risk_score, risk_summary, security_action, sanitized, blocked_reason
)
SELECT
    id, api_key_name, channel_name, model, upstream_model, mode,
    status_code, prompt_tokens, completion_tokens, total_tokens, duration_ms,
    error_message, is_stream, is_retry, created_at, request_body, response_body,
    risk_level, risk_score, risk_summary, security_action, sanitized, blocked_reason
FROM request_logs
ORDER BY COALESCE(seq, 0), created_at;

DROP TABLE request_logs;
ALTER TABLE request_logs_new RENAME TO request_logs;

CREATE INDEX IF NOT EXISTS idx_logs_created ON request_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_logs_channel ON request_logs(channel_name);
CREATE INDEX IF NOT EXISTS idx_logs_model ON request_logs(model);
