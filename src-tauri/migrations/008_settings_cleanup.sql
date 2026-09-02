-- 008_settings_cleanup.sql
-- 清理历史遗留的设置数据问题（不改表结构、不触碰已应用迁移）。

-- 1) 修复 security_mode 潜在 bug：003 迁移把值写成非法 "warning"
--    （合法模式仅 audit/warn/redact/block），security/mod.rs 的 match 落到
--    `_ => Allow`，静默降级成「只审计」。修正为合法 "warn"。
--    仅改非法值；用户已显式设定的 audit/warn/redact/block 不受影响。
UPDATE settings SET value = '"warn"'
WHERE key = 'security_mode' AND value = '"warning"';

-- 2) 删除 003 种下的死键：前端/后端只认 security_scan_*，
--    以下 6 个 scan_* 键无任何代码读取，属历史遗留脏数据。
DELETE FROM settings WHERE key IN (
  'scan_credentials',
  'scan_pii',
  'scan_payment',
  'scan_network',
  'scan_code_exec',
  'scan_prompt_injection'
);
