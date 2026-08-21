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

export interface ProviderPreset {
  type: ChannelType;
  label: string;
  default_base_url: string;
  requires_api_key: boolean;
}

// ============================================================
// Gateway Key (ApiKey)
// ============================================================

export type ApiKeyStatus = 0 | 1 | 2; // disabled | active | expired

export interface ApiKey {
  id: string;
  name: string;
  key: string; // masked: sk-dong-****a1b2
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
export type SecurityMode = "strict" | "balanced" | "permissive";

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
}
