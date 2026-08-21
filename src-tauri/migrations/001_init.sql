-- DongX database schema v1
-- Created: 2026-08-21

-- ============================================================
-- channels: 上游渠道
-- ============================================================
CREATE TABLE IF NOT EXISTS channels (
  id              TEXT PRIMARY KEY,
  name            TEXT NOT NULL,
  protocol        TEXT NOT NULL,             -- openai | anthropic | ollama
  type            TEXT NOT NULL,             -- openai | deepseek | claude | gemini | zhipu | ollama | custom
  base_url        TEXT NOT NULL,
  cred_encrypted  TEXT NOT NULL DEFAULT '',  -- AES-GCM encrypted upstream API key
  models          TEXT NOT NULL DEFAULT '[]', -- JSON array of model names
  status          INTEGER NOT NULL DEFAULT 1, -- 0=disabled 1=enabled 2=error
  priority        INTEGER NOT NULL DEFAULT 0,
  weight          INTEGER NOT NULL DEFAULT 1,
  config          TEXT NOT NULL DEFAULT '{}', -- JSON: timeout, custom headers, proxy
  model_mapping   TEXT NOT NULL DEFAULT '{}', -- JSON: external_model -> upstream_model
  endpoints       TEXT NOT NULL DEFAULT '[]', -- JSON array of enabled endpoint paths
  created_at      TEXT NOT NULL,
  updated_at      TEXT NOT NULL,
  last_test_at    TEXT,
  last_test_ok    INTEGER
);

CREATE INDEX IF NOT EXISTS idx_channels_status ON channels(status);
CREATE INDEX IF NOT EXISTS idx_channels_type ON channels(type);

-- ============================================================
-- gateway_keys: 网关密钥 (downstream client credentials)
-- ============================================================
CREATE TABLE IF NOT EXISTS gateway_keys (
  id               TEXT PRIMARY KEY,
  name             TEXT NOT NULL,
  key              TEXT NOT NULL,             -- masked display: sk-dong-****a1b2
  key_hash         TEXT NOT NULL,             -- bcrypt/argon2 hash
  status           INTEGER NOT NULL DEFAULT 1, -- 0=disabled 1=active 2=expired
  allowed_models   TEXT NOT NULL DEFAULT '[]',
  allowed_channels TEXT NOT NULL DEFAULT '[]',
  quota_limit      INTEGER NOT NULL DEFAULT 0, -- 0=unlimited
  quota_used       INTEGER NOT NULL DEFAULT 0,
  expires_at       TEXT,
  created_at       TEXT NOT NULL,
  updated_at       TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_gateway_keys_status ON gateway_keys(status);

-- ============================================================
-- request_logs: 请求日志
-- ============================================================
CREATE TABLE IF NOT EXISTS request_logs (
  id                TEXT PRIMARY KEY,
  seq               INTEGER,
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

CREATE INDEX IF NOT EXISTS idx_logs_created ON request_logs(created_at);
CREATE INDEX IF NOT EXISTS idx_logs_channel ON request_logs(channel_name);
CREATE INDEX IF NOT EXISTS idx_logs_model ON request_logs(model);

-- ============================================================
-- audit_events: 安全审计
-- ============================================================
CREATE TABLE IF NOT EXISTS audit_events (
  id          TEXT PRIMARY KEY,
  timestamp   TEXT NOT NULL,
  type        TEXT NOT NULL,                 -- rate_limit | invalid_key | quota_exhaust | suspicious | config_change
  severity    TEXT NOT NULL,                 -- info | warning | critical
  actor       TEXT,
  message     TEXT NOT NULL,
  meta        TEXT                            -- JSON
);

CREATE INDEX IF NOT EXISTS idx_audit_timestamp ON audit_events(timestamp);
CREATE INDEX IF NOT EXISTS idx_audit_severity ON audit_events(severity);

-- ============================================================
-- settings: 系统设置 (KV)
-- ============================================================
CREATE TABLE IF NOT EXISTS settings (
  key   TEXT PRIMARY KEY,
  value TEXT NOT NULL                         -- JSON encoded value
);

-- Default settings
INSERT OR IGNORE INTO settings (key, value) VALUES
  ('server_port', '9842'),
  ('server_host', '"127.0.0.1"'),
  ('ui_theme', '"system"'),
  ('ui_language', '"zh-CN"'),
  ('minimize_to_tray', 'true'),
  ('close_to_tray', 'true'),
  ('auto_start', 'false'),
  ('retry_enabled', 'true'),
  ('retry_times', '3'),
  ('log_retention_days', '30'),
  ('log_raw_body', 'false'),
  ('security_enabled', 'true'),
  ('security_mode', '"balanced"');
