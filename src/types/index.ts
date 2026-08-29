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
export type SecurityAction = "allow" | "sanitize" | "flag" | "block";

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
// Audit Event
// ============================================================

export type AuditEventType =
  | "rate_limit"
  | "invalid_key"
  | "quota_exhaust"
  | "suspicious"
  | "config_change";
export type AuditSeverity = "info" | "warning" | "critical";

export interface AuditEvent {
  id: string;
  timestamp: string;
  type: AuditEventType;
  severity: AuditSeverity;
  actor: string | null;
  message: string;
  meta: Record<string, unknown> | null;
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
/** 安全审计模式 — 4 级风险响应策略
 *  - permissive: 只记录（高风险也不阻断，仅落审计）
 *  - warning:    中高风险标记告警（最低默认）
 *  - redact:     高风险脱敏转发（敏感值替换后转发）
 *  - strict:     高风险直接阻断（不离开本机）
 */
export type SecurityMode = "permissive" | "warning" | "redact" | "strict";

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
  log_retention_days: number;
  log_raw_body: boolean;
  security_enabled: boolean;
  security_mode: SecurityMode;
  /** 安全审计检测项配置（仅当 security_enabled=true 时生效） */
  security_detect_unicode_stego: boolean;
  security_detect_tool_risk: boolean;
  security_detect_outbound_tracking: boolean;
  security_scan_response: boolean;
  security_redact_request: boolean;
  security_block_critical: boolean;
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
