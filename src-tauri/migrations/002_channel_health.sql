-- DongX schema v2: per-channel circuit-breaker health
--
-- Mirrors waliapi's `channel_mode_health`: health state is persisted in the
-- DB (survives process restart) and lives in its OWN table — it is NOT mixed
-- into `request_logs` (which stays a pure audit trail).
--
-- The dispatcher skips any channel whose breaker is "open" (cooldown_until is
-- still in the future); `record_failure` trips the breaker after N consecutive
-- retryable failures and `record_success` resets it.

CREATE TABLE IF NOT EXISTS channel_health (
  channel_id           TEXT PRIMARY KEY REFERENCES channels(id) ON DELETE CASCADE,
  consecutive_failures INTEGER NOT NULL DEFAULT 0,
  cooldown_until       TEXT,               -- RFC-3339; if in the future the breaker is open
  last_failure_at      TEXT,
  last_failure_reason  TEXT
);

CREATE INDEX IF NOT EXISTS idx_channel_health_cooldown
    ON channel_health(cooldown_until);
