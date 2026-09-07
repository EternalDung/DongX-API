-- 链路追踪：网关侧强制生成的 trace_id + 上游返回的 provider_request_id。
--
-- trace_id：每次请求进入数据面时由服务端 Uuid::new_v4() 强制生成（恒非空），
--   贯穿所有故障转移重试尝试，是「同一次用户请求」的聚合主键。
--   优于 waliapi 的「客户端可选头 Wali-Trace-Id」——后者客户端不发就为 NULL，
--   无法保证每次请求都有可追溯的链路。
--
-- provider_request_id：上游大模型响应头里带回的本次请求标识
--   （OpenAI 兼容 X-Request-Id / Anthropic request-id），与网关 trace_id 不同，
--   用于跨系统（网关 ↔ 提供商）定位单次调用。
--
-- 历史行这两列恒为 NULL；新请求开始写入，旧数据不影响聚合。
ALTER TABLE request_logs ADD COLUMN trace_id TEXT;
ALTER TABLE request_logs ADD COLUMN provider_request_id TEXT;
CREATE INDEX IF NOT EXISTS idx_request_logs_trace_id ON request_logs(trace_id);
