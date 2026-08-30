-- 006_builtin_defaults_off.sql
-- 部分内置规则默认禁用：LLM 网关场景下误报率高的低价值规则。
-- - net.external_url（b013, info）：请求/响应几乎必含 http(s) 外链，纯噪音。
-- - exec.git_info（b018, low）：Git 信息泄露场景较窄。
-- 仅置 enabled=0，不改其它字段（已应用迁移不可编辑，故新建本迁移）。
UPDATE security_builtin_rules SET enabled = 0
  WHERE rule_id IN ('net.external_url', 'exec.git_info');
