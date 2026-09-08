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

export type LogMode = "chat" | "responses" | "messages" | "completion" | "embedding" | "rag" | "deep-research" | "other";
export type RiskLevel = "none" | "low" | "medium" | "high" | "critical" | "skipped";
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
  /** 网关侧强制生成的链路追踪 ID：一次请求内所有日志行共享 */
  trace_id: string | null;
  /** 上游返回的请求 ID（x-request-id / request-id），用于向提供商排查 */
  provider_request_id: string | null;
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

/** 按模型聚合的调用统计（GROUP BY model），供 Dashboard「模型调用明细」表使用。 */
export interface ModelStat {
  model: string;
  request_count: number;
  prompt_tokens: number;
  completion_tokens: number;
  cached_tokens: number;
  total_tokens: number;
  success_count: number;
  total_count: number;
  avg_latency_ms: number;
  /** 该模型各场景（chat/responses/messages/rag/wiki）的调用次数分布 */
  mode_breakdown: Record<string, number>;
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

// ============================================================
// 客户端接入配置（使用页 CodeX/Claude Code/OpenCode 等切换）
// ============================================================

/** 单个 AI 客户端接入信息（对应 Rust ClientInfo） */
export interface ClientInfo {
  /** 客户端标识：codex / claude-code / opencode / openclaw / hermes */
  name: string;
  label: string;
  icon: string;
  description: string;
  config_path: string;
  config_format: string;
  /** 可配置：CLI 已装 或 配置文件已存在（可写入） */
  available: boolean;
  /** 已安装：探测到 CLI 可执行文件（强信号；不再仅凭配置目录存在判断） */
  installed: boolean;
  /** 是否已接入本网关（配置中含 _dongx 标记 / DongX provider） */
  applied: boolean;
  download_url: string;
}

/** 一键写入结果（对应 Rust ApplyResult） */
export interface ApplyResult {
  success: boolean;
  message: string;
}

/** 配置文件内容读取结果（对应 Rust ConfigContent） */
export interface ConfigContent {
  exists: boolean;
  content: string;
  error: string | null;
}

// ============================================================
// 业务服务（服务页）
// ============================================================

/** 单个业务服务的运行态快照（对应 Rust ServiceStatus） */
export interface ServiceStatus {
  id: string;
  name: string;
  description: string;
  enabled: boolean;
  running: boolean;
  /** 服务自定义统计（RAG 为 kb_count / doc_count / chunk_count 等） */
  stats: Record<string, number>;
}

// ============================================================
// RAG / Wiki
// ------------------------------------------------------------
// 以下类型由 ts-rs 从 Rust 结构体生成，**不要手改**：
//   src/types/generated/rag.ts   （RAG：知识库 / 文档 / 分片 / 检索 / 问答）
//   src/types/generated/wiki.ts  （Wiki：项目 / 来源 / 页面 / 问答）
// 重新生成（Rust 类型变更后执行）：
//   cd src-tauri && cargo test --features ts-export --lib export_ts_bindings
// 本文件只做少量「收窄」：Rust 侧用 i64 / String 表达的枚举，
// 前端需要更严格的字面量联合类型。
// ============================================================

import type { KnowledgeBase as KnowledgeBaseGenerated } from "./generated/rag";
import type {
  WikiPage as WikiPageGenerated,
  WikiProject as WikiProjectGenerated,
  WikiSource as WikiSourceGenerated,
} from "./generated/wiki";

export type {
  AskResult,
  ImportSourceInput,
  IngestResult,
  IndexStatus,
  KbDocument,
  KbDocumentChunk,
  KbDocumentChunksPage,
  KbSource,
  KnowledgeBaseInput,
  KnowledgeBaseUpdate,
  RagSource,
  RetrievalHit,
} from "./generated/rag";

export type {
  WikiAskResult,
  WikiCitation,
  WikiProjectInput,
  WikiProjectUpdate,
  WikiSourceInput,
} from "./generated/wiki";

/** 知识库状态：0 禁用 / 1 启用 */
export type KnowledgeBaseStatus = 0 | 1;

/** Wiki 项目状态：0 禁用 / 1 就绪 */
export type WikiProjectStatus = 0 | 1;

/** 来源类型：git 仓库 / 网页 URL / 本地目录 */
export type WikiSourceKind = "git" | "url" | "local_dir";

/** 来源状态。注意：摄入中状态只存在于「源」粒度，不会冒泡成项目状态。 */
export type WikiSourceStatus = "pending" | "ingesting" | "ready" | "failed";

/** 页面分类。LLM 摄入时强制带 kind，用于页面 Tab 的分类过滤。 */
export type WikiPageKind = "概念" | "实体" | "日志" | "索引" | "摘要";

// MCP 运行态（Rust McpStatus 在 mcp/ 模块，本轮未纳入 ts-rs 生成，
// 仍为手写；后续要接生成时把对应结构体挂上 derive(TS) 即可）
/** MCP 端点（运行态） */
export interface McpListenAddr {
  host: string;
  port: number;
}

/** MCP 服务运行态 */
export interface McpStatus {
  /** server 进程是否在监听端口 */
  running: boolean;
  /** 当前实际监听的地址；服务未起时为 null */
  bindAddr: McpListenAddr | null;
  /** 给 MCP client 用的端点 URL（= bindAddr.url()）；服务未起时为 null */
  endpoint: string | null;
  /** 当前可用的 MCP tool 数量（与后端 tool_specs().len() 一致） */
  toolsCount: number;
}

/** 知识库（Rust KnowledgeBase，status 收窄为 0|1） */
export type KnowledgeBase = Omit<KnowledgeBaseGenerated, "status"> & {
  status: KnowledgeBaseStatus;
};

/** Wiki 项目（Rust WikiProject，status 收窄为 0|1） */
export type WikiProject = Omit<WikiProjectGenerated, "status"> & {
  status: WikiProjectStatus;
};

/** Wiki 来源（Rust WikiSource，kind / status 收窄） */
export type WikiSource = Omit<WikiSourceGenerated, "kind" | "status"> & {
  kind: WikiSourceKind;
  status: WikiSourceStatus;
};

/** Wiki 页面（Rust WikiPage，kind 收窄） */
export type WikiPage = Omit<WikiPageGenerated, "kind"> & {
  kind: WikiPageKind;
};
