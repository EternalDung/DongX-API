-- 003_security_audit.sql
-- 安全审计：内置规则表、自定义规则表、每次请求的发现明细表。
-- 说明：request_logs 已在 001_init.sql 预留 risk_level/risk_score/risk_summary/
--       security_action/sanitized/blocked_reason 六个字段，无需再改。
-- 参考：同类桌面 LLM 网关的安全闸门库表设计（内置规则 + 自定义规则 + 发现明细）。

-- ============================================================
-- security_builtin_rules: 内置检测规则（应用首次启动时种子，用户可编辑 enabled）
-- ============================================================
CREATE TABLE IF NOT EXISTS security_builtin_rules (
    id          TEXT PRIMARY KEY,
    rule_id     TEXT NOT NULL UNIQUE,     -- 稳定标识，scanner 按此匹配正则
    category    TEXT NOT NULL,            -- credential | personal | payment | network | tool | prompt
    severity    TEXT NOT NULL DEFAULT 'medium',
    title       TEXT NOT NULL,
    description TEXT,
    toggle_key  TEXT,                     -- 对应设置页 6 个检测开关的 key；NULL 表示常开
    enabled     INTEGER NOT NULL DEFAULT 1,
    created_at  TEXT NOT NULL
);

-- ============================================================
-- security_custom_rules: 用户自定义规则（黑名单/白名单）
-- ============================================================
CREATE TABLE IF NOT EXISTS security_custom_rules (
    id          TEXT PRIMARY KEY,
    rule_type   TEXT NOT NULL,            -- 'blacklist' | 'whitelist'
    category    TEXT NOT NULL,            -- 'domain' | 'tool' | 'path' | 'keyword'
    pattern     TEXT NOT NULL,
    severity    TEXT NOT NULL DEFAULT 'medium',
    action      TEXT NOT NULL DEFAULT 'warn',
    enabled     INTEGER NOT NULL DEFAULT 1,
    description TEXT,
    created_at  TEXT NOT NULL
);

-- ============================================================
-- request_security_findings: 每次请求命中的风险明细
-- ============================================================
CREATE TABLE IF NOT EXISTS request_security_findings (
    id              TEXT PRIMARY KEY,
    log_id          TEXT NOT NULL,        -- -> request_logs.id
    phase           TEXT NOT NULL,        -- 扫描阶段，如 request/request_delta
    category        TEXT NOT NULL,
    rule_id         TEXT NOT NULL,
    severity        TEXT NOT NULL,
    title           TEXT NOT NULL,
    description     TEXT,
    location        TEXT,                 -- JSON 指针，定位命中字段
    evidence_masked TEXT,                 -- 脱敏后的证据片段（不存明文）
    evidence_hash   TEXT,                 -- 明文证据哈希（可选，用于去重/取证，NULL 即可）
    action          TEXT,                 -- 对该发现采取的动作
    created_at      TEXT NOT NULL
);

CREATE INDEX IF NOT EXISTS idx_builtin_rules_category ON security_builtin_rules(category);
CREATE INDEX IF NOT EXISTS idx_builtin_rules_enabled  ON security_builtin_rules(enabled);
CREATE INDEX IF NOT EXISTS idx_custom_rules_enabled   ON security_custom_rules(enabled);
CREATE INDEX IF NOT EXISTS idx_findings_log           ON request_security_findings(log_id);
CREATE INDEX IF NOT EXISTS idx_findings_rule         ON request_security_findings(rule_id);

-- ============================================================
-- 种子内置规则：toggle_key 与设置页 6 个开关一一对应
-- ============================================================
INSERT OR IGNORE INTO security_builtin_rules (id, rule_id, category, severity, title, description, toggle_key, enabled, created_at) VALUES
  ('b001','cred.secret_token','credential','high','疑似密钥/Token','检测 sk-/ghp_/AKIA/AIza/JWT/Bearer/xoxb 等凭证格式','scan_credentials',1,datetime('now')),
  ('b002','cred.private_key','credential','critical','私钥内容','检测 PEM/OpenSSH 私钥头部','scan_credentials',1,datetime('now')),
  ('b003','cred.named_secret','credential','high','敏感凭证字段','检测 Authorization/Cookie/Session/Secret/Password/Token 等字段名赋值','scan_credentials',1,datetime('now')),
  ('b004','cred.database_url','credential','high','数据库连接串','检测 mysql:// postgres:// mongodb:// redis://','scan_credentials',1,datetime('now')),
  ('b005','cred.cloud_key','credential','high','云厂商密钥','检测 AWS AKIA/腾讯云 SecretId/阿里云 AccessKey/GCP AIza','scan_credentials',1,datetime('now')),
  ('b006','pii.id_card','personal','medium','中国居民身份证号','检测 18 位居民身份证号码','scan_pii',1,datetime('now')),
  ('b007','pii.email','personal','low','邮箱地址','检测邮箱格式字符串','scan_pii',1,datetime('now')),
  ('b008','pii.phone','personal','low','手机号码','检测中国大陆手机号','scan_pii',1,datetime('now')),
  ('b009','pay.credit_card','payment','high','信用卡号','检测主流卡组织卡号（4/5/3/6 开头分组）','scan_payment',1,datetime('now')),
  ('b010','pay.bank_card','payment','high','银行卡号','检测 16-19 位银行卡号（含银联 62 开头）','scan_payment',1,datetime('now')),
  ('b011','net.ip_probe','network','high','公网 IP 探测','检测 ifconfig.me/ipinfo.io/ipify.org 等','scan_network',1,datetime('now')),
  ('b012','net.suspicious_domain','network','high','可疑外联域名','检测 webhook.site/ngrok/pastebin/requestbin','scan_network',1,datetime('now')),
  ('b013','net.external_url','network','info','外部 URL','检测请求中的 http(s) 外链','scan_network',1,datetime('now')),
  ('b014','net.tracking_pixel','network','high','追踪像素','检测 1x1 图片/track/pixel/beacon','scan_network',1,datetime('now')),
  ('b015','exec.shell_command','tool','medium','高风险命令片段','检测 curl/wget/nc/scp/bash -c/python -c','scan_code_exec',1,datetime('now')),
  ('b016','exec.exfiltration','tool','critical','疑似数据外传命令','检测敏感文件读取与网络外传的组合','scan_code_exec',1,datetime('now')),
  ('b017','exec.remote_script','tool','critical','远程脚本执行','检测 curl|wget 下载脚本管道到 sh/bash','scan_code_exec',1,datetime('now')),
  ('b018','exec.git_info','tool','low','Git 信息泄露','检测 git remote/gh auth token','scan_code_exec',1,datetime('now')),
  ('b019','exec.ssh_key','tool','critical','SSH 密钥文件','检测 id_rsa/id_ed25519/id_ecdsa','scan_code_exec',1,datetime('now')),
  ('b020','prompt.injection','prompt','high','提示注入/越权','检测要求忽略指令/隐藏行为/绕过审计','scan_prompt_injection',1,datetime('now')),
  ('b021','prompt.fingerprint','prompt','medium','账号画像/风控上下文','检测指纹/代理/风控相关词同时出现','scan_prompt_injection',1,datetime('now'));

-- ============================================================
-- 设置补全：6 个检测开关默认开启；旧 3 级 security_mode 迁移为 4 级默认值
-- ============================================================
INSERT OR IGNORE INTO settings (key, value) VALUES
  ('scan_credentials','true'),
  ('scan_pii','true'),
  ('scan_payment','true'),
  ('scan_network','true'),
  ('scan_code_exec','true'),
  ('scan_prompt_injection','true');

-- 旧值 'balanced' 在新 4 级 select（permissive/warning/redact/strict）中非法，统一迁移为 'warning'。
UPDATE settings SET value = '"warning"' WHERE key = 'security_mode' AND value = '"balanced"';
