-- 004_security_alignment.sql
-- 对齐 waliapi 的安全审计设置模型。
-- 背景：waliapi 实有「4 模式(audit/warn/redact/block) + 6 开关(3 检测 + 响应 + 2 行为)」，
--   且脱敏/阻断是独立于模式的开关（security_redact_secrets / security_block_on_critical）。
-- 此前 DongX 后端 gate 误读了一套不相关的类目键(scan_credentials 等)，导致 UI 上
--   拨的 6 个开关是死键。本迁移完成规则 toggle 重映射、补 unicode 规则、模式值映射与孤儿键清理。
-- 003 已应用（sqlx SHA-384 校验），不可编辑，故全部变更放此新迁移。

-- ============================================================
-- 1) 重映射内置规则 toggle_key
--    waliapi 仅 unicode/tools/network 三类可被独立开关控制；
--   凭证/PII/支付/命令/提示注入始终扫描（toggle_key=NULL，常开）。
-- ============================================================
UPDATE security_builtin_rules SET toggle_key = NULL
  WHERE rule_id IN ('cred.secret_token','cred.private_key','cred.named_secret','cred.database_url','cred.cloud_key');

UPDATE security_builtin_rules SET toggle_key = NULL
  WHERE rule_id IN ('pii.id_card','pii.email','pii.phone');

UPDATE security_builtin_rules SET toggle_key = NULL
  WHERE rule_id IN ('pay.credit_card','pay.bank_card');

UPDATE security_builtin_rules SET toggle_key = 'security_scan_network'
  WHERE rule_id IN ('net.ip_probe','net.suspicious_domain','net.external_url','net.tracking_pixel');

UPDATE security_builtin_rules SET toggle_key = 'security_scan_tools'
  WHERE rule_id IN ('exec.shell_command','exec.exfiltration','exec.remote_script','exec.git_info','exec.ssh_key');

UPDATE security_builtin_rules SET toggle_key = NULL
  WHERE rule_id IN ('prompt.injection','prompt.fingerprint');

-- ============================================================
-- 2) 补 4 条 Unicode 隐写规则（对齐 waliapi b012-b015），受 security_scan_unicode 控制。
--    正则固定在 scanner.rs PATTERNS（按 rule_id 匹配），此处仅存元数据。
-- ============================================================
INSERT OR IGNORE INTO security_builtin_rules (id, rule_id, category, severity, title, description, toggle_key, enabled, created_at) VALUES
  ('b022','unicode.zero_width','unicode','medium','零宽 Unicode 字符','检测 U+200B/200C/200D/2060/FEFF 等不可见字符','security_scan_unicode',1,datetime('now')),
  ('b023','unicode.bidi_control','unicode','high','方向控制 Unicode 字符','检测 U+202A-202E、U+2066-2069 等 Bidi 控制字符','security_scan_unicode',1,datetime('now')),
  ('b024','unicode.variation_selector','unicode','medium','变体选择符','检测 U+FE00-FE0F、U+E0100-E01EF 等 variation selector','security_scan_unicode',1,datetime('now')),
  ('b025','unicode.homograph','unicode','medium','同形异义字符','检测西里尔、希腊等与拉丁字母同形的字符，可能用于域名混淆','security_scan_unicode',1,datetime('now'));

-- ============================================================
-- 3) 旧 4 级模式值映射（permissive/warning/strict → audit/warn/block；redact 不变）
-- ============================================================
UPDATE settings SET value = '"audit"' WHERE key = 'security_mode' AND value = '"permissive"';
UPDATE settings SET value = '"warn"'  WHERE key = 'security_mode' AND value = '"warning"';
UPDATE settings SET value = '"block"' WHERE key = 'security_mode' AND value = '"strict"';

-- ============================================================
-- 4) 清理 003 种下的孤儿类目键（gate 已改读 security_scan_* 等，旧键无人读取）
-- ============================================================
DELETE FROM settings WHERE key IN (
  'scan_credentials','scan_pii','scan_payment','scan_network','scan_code_exec','scan_prompt_injection'
);
