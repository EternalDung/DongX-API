/**
 * DongX shared types (frontend <-> backend wire format)
 * Convention: snake_case to align with Rust Serde field names and SQLite columns.
 */

// ============================================================
// Channel
// ============================================================

export type ChannelProtocol = "openai" | "anthropic" | "ollama";
export type ChannelType =
  | "openai"
  | "deepseek"
  | "claude"
  | "gemini"
  | "zhipu"
  | "ollama"
  | "custom";
export type ChannelStatus = 0 | 1 | 2; // disabled | enabled | error

export interface Channel {
  id: string;
  name: string;
  protocol: ChannelProtocol;
  type: ChannelType;
  base_url: string;
  api_key: string; // masked display
  keys?: { key: string; weight: number }[]; // decrypted upstream keys (local gateway)
  models: string[];
  status: ChannelStatus;
  priority: number;
  weight: number;
  config: Record<string, unknown>;
  model_mapping: Record<string, string>;
  endpoints: string[];
  created_at: string;
  updated_at: string;
  last_test_at: string | null;
  last_test_ok: number | null; // 0 | 1 | null
}

// ---- Channel preset registry (mirrors backend channel_presets) ----
export type ChannelProvider =
  | "openai"
  | "google"
  | "deepseek"
  | "qwen"
  | "zhipu"
  | "doubao"
  | "doubao_coding_plan"
  | "moonshot"
  | "anthropic"
  | "ollama"
  | "custom";

export type ChannelRegionGroup = "custom" | "international" | "domestic" | "local";

export type ChannelEndpoint =
  | "chat_completions"
  | "responses"
  | "messages"
  | "count_tokens"
  | "embeddings"
  | "api_chat";

export type ChannelAuthScheme =
  | "bearer"
  | "x_api_key"
  | "query_key"
  | "optional_bearer";

export type ChannelModelEnumStrategy = "static_only" | "static_plus_sync" | "sync_only";
export type ChannelEndpointTestStrategy = "probe_first_model" | "list_models";

export interface ModelSuggestion {
  id: string;
  verified_at: string;
  source_url: string;
}

export interface ChannelPreset {
  id: string;
  protocol: ChannelProtocol;
  provider: ChannelProvider;
  display_name: string;
  region: ChannelRegionGroup;
  description: string;
  icon_key: string;
  native_base_url: string;
  legacy_base_url: string;
  legacy_type: string;
  native_endpoints: ChannelEndpoint[];
  default_checked_endpoints: ChannelEndpoint[];
  auth_scheme: ChannelAuthScheme;
  model_suggestions: ModelSuggestion[];
  model_enum_strategy: ChannelModelEnumStrategy;
  endpoint_test_strategy: ChannelEndpointTestStrategy;
  preset_revision: string;
}

export interface ChannelProtocolPresetGroup {
  protocol: ChannelProtocol;
  presets: ChannelPreset[];
}

// ============================================================
// Gateway Key (ApiKey)
// ============================================================

export type ApiKeyStatus = 0 | 1 | 2; // disabled | active | expired

export interface ApiKey {
  id: string;
  name: string;
  key: string; // plaintext gateway key（本地明文存储）
  status: ApiKeyStatus;
  allowed_models: string[];
  allowed_channels: string[];
  quota_limit: number;
  quota_used: number;
  expires_at: string | null;
  created_at: string;
  updated_at: string;
}

// ============================================================
// Request Log
// ============================================================

export type LogMode = "chat" | "completion" | "embedding" | "other";
export type RiskLevel = "none" | "low" | "medium" | "high" | "critical";
/** 安全闸门对一次请求采取的动作（与后端 SecurityAction::as_str 一一对应） */
export type SecurityAction = "allow" | "warn" | "redact" | "block";

/**
 * 单条安全审计发现（对应 request_security_findings 一行）。
 * 与 RequestLog 上的 risk_level/risk_score 汇总字段的区别：
 * 汇总只看最高等级，findings 是**全部**命中明细，每条自带自己的 severity。
 */
export interface SecurityFinding {
  id: string;
  log_id: string;
  /** 扫描阶段：request = 入站请求体 / response = 出站响应体 */
  phase: "request" | "response";
  category: string;
  rule_id: string;
  severity: RiskLevel;
  title: string;
  description: string | null;
  /** JSON 指针，定位命中字段 */
  location: string | null;
  /** 脱敏后的证据片段（不存明文） */
  evidence_masked: string | null;
  action: SecurityAction | null;
  created_at: string;
}

export interface RequestLog {
  id: string;
  seq: number;
  api_key_name: string | null;
  channel_name: string | null;
  model: string;
  upstream_model: string | null;
  mode: LogMode;
  status_code: number;
  prompt_tokens: number;
  completion_tokens: number;
  total_tokens: number;
  duration_ms: number;
  error_message: string | null;
  is_stream: boolean;
  is_retry: boolean;
  created_at: string;
  request_body: string | null;
  response_body: string | null;
  risk_level: RiskLevel;
  risk_score: number;
  risk_summary: string | null;
  security_action: SecurityAction;
  sanitized: boolean;
  blocked_reason: string | null;
}

// ============================================================
// Dashboard Stats
// ============================================================

export interface DashboardStats {
  today_requests: number;
  today_total_tokens: number;
  active_channels: number;
  avg_latency_ms: number;
  total_channels: number;
  total_api_keys: number;
  total_requests: number;
  total_tokens: number;
}

// ============================================================
// Settings
// ============================================================

export type ThemeMode = "light" | "dark" | "system";
/**
 * 安全审计模式 — 4 级风险响应策略
 *  - audit:  只审计（记录风险，不影响请求）
 *  - warn:   中高风险标记告警
 *  - redact: 高风险脱敏转发（敏感值替换后转发）
 *  - block:  高风险直接阻断（不离开本机）
 */
export type SecurityMode = "audit" | "warn" | "redact" | "block";

export interface Settings {
  server_port: number;
  server_host: string;
  ui_theme: ThemeMode;
  ui_language: string;
  minimize_to_tray: boolean;
  close_to_tray: boolean;
  auto_start: boolean;
  retry_enabled: boolean;
  retry_times: number;
  enable_rate_limit: boolean;
  rate_limit_rpm: number;
  log_retention_days: number;
  log_raw_body: boolean;
  security_enabled: boolean;
  security_mode: SecurityMode;
  /** 安全审计检测项配置（仅当 security_enabled=true 时生效） */
  security_scan_unicode: boolean;
  security_scan_tools: boolean;
  security_scan_network: boolean;
  security_scan_response: boolean;
  security_redact_secrets: boolean;
  security_block_on_critical: boolean;
}

/** 自定义安全规则（对应后端 security_custom_rules + CustomRule） */
export interface CustomRule {
  id: string;
  rule_type: "blacklist" | "whitelist";
  category: "domain" | "tool" | "path" | "keyword";
  pattern: string;
  severity: "low" | "medium" | "high" | "critical";
  action: "warn" | "block";
  enabled: boolean;
  description: string | null;
  created_at: string;
}

/** 自定义规则创建/更新参数（对应 Rust CustomRuleInput） */
export interface CustomRuleInput {
  rule_type: "blacklist" | "whitelist";
  category: "domain" | "tool" | "path" | "keyword";
  pattern: string;
  severity: "low" | "medium" | "high" | "critical";
  action: "warn" | "block";
  enabled: boolean;
  description: string | null;
}

/** 内置安全规则（对应后端 security_builtin_rules + BuiltinRule） */
export interface BuiltinRule {
  rule_id: string;
  category:
    | "credential"
    | "personal"
    | "payment"
    | "network"
    | "tool"
    | "prompt"
    | "unicode";
  severity: "info" | "low" | "medium" | "high" | "critical";
  title: string;
  description: string | null;
  /** 控制该规则所属类目的全局开关 key；NULL 表示常开（不可单独关闭） */
  toggle_key: string | null;
  enabled: boolean;
}

/** 内置规则更新参数（对应 Rust BuiltinRuleUpdate） */
export interface BuiltinRuleUpdate {
  enabled: boolean;
  severity: "info" | "low" | "medium" | "high" | "critical";
}

// ============================================================
// 网关服务状态
// ============================================================

/** 网关服务运行态快照（对应 Rust ServerStatus） */
export interface ServerStatus {
  /** 服务当前是否在运行 */
  running: boolean;
  /** 实际监听地址（未运行时为 null） */
  host: string | null;
  port: number | null;
  /** 完整端点 URL（未运行时为 null） */
  url: string | null;
  /** settings 中配置的值（重启服务后才会成为运行态值） */
  configured_host: string;
  configured_port: number;
  /** 配置与运行态不一致 → 需重启服务才生效 */
  restart_required: boolean;
}
