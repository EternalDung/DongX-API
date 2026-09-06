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
// Knowledge Base (RAG)
// 知识库：RAG 检索的数据源。每个知识库绑定一个嵌入模型/渠道，
// 摄入文档后分块并向量化，问答时检索相关片段。
// 后端由 Phase 1（009_rag.sql + rag commands）落地；前端先接类型与 API。
// ============================================================

/** 知识库状态：0 禁用 / 1 启用 */
export type KnowledgeBaseStatus = 0 | 1;

export interface KnowledgeBase {
  id: string;
  name: string;
  description: string;
  /** 用于向量化的嵌入模型名（如 text-embedding-3-small） */
  embedding_model: string;
  /** 提供该嵌入模型的渠道 id */
  embedding_channel_id: string;
  doc_count: number;
  chunk_count: number;
  status: KnowledgeBaseStatus;
  /** 是否将本知识库暴露给 MCP 层（0=否 1=是） */
  mcp_exposed: number;
  /** 单次向量化批大小（null=取引擎默认） */
  embedding_batch_size: number | null;
  /** 摄入时排除的目录（逗号分隔，null=不排除） */
  exclude_dirs: string | null;
  /** 摄入时排除的文件（逗号分隔，null=不排除） */
  exclude_files: string | null;
  /** 摄入时仅包含的文件类型（逗号分隔，null=全部） */
  include_file_types: string | null;
  /** 分块大小（token 数，0=引擎默认 512） */
  chunk_size: number | null;
  /** 分块重叠 token 数（0=引擎默认 64） */
  chunk_overlap: number | null;
  created_at: string;
  updated_at: string;
}

/** 新建知识库参数 — 对应 Rust KnowledgeBaseInput */
export interface KnowledgeBaseInput {
  name: string;
  description: string;
  embedding_model: string;
}

/** 更新知识库参数（部分更新）— 对应 Rust KnowledgeBaseUpdate */
export interface KnowledgeBaseUpdate {
  name?: string;
  description?: string;
  /** 启用 RAG 开关：0=禁用 1=启用（复用 status 列） */
  status?: number;
  /** MCP 暴露开关：0=否 1=是 */
  mcp_exposed?: number;
  /**
   * 嵌入模型。不同模型的向量空间不兼容：改模型后旧分块立刻变 stale，
   * 需调用 reindex 重建索引，否则向量/混合检索会静默返回错误结果。
   */
  embedding_model?: string;
  /** 绑定的嵌入渠道（必填，须为已启用渠道）。换渠道通常也要换模型 */
  embedding_channel_id?: string;
  embedding_batch_size?: number | null;
  exclude_dirs?: string | null;
  exclude_files?: string | null;
  include_file_types?: string | null;
  /** 分块大小（token 数，0=引擎默认 512） */
  chunk_size?: number | null;
  /** 分块重叠 token 数（0=引擎默认 64） */
  chunk_overlap?: number | null;
}

/** 摄入文本结果 — 对应 Rust IngestResult */
export interface IngestResult {
  /** 新建文档 id */
  document_id: string;
  /** 分块数 */
  chunk_count: number;
  /** 命中重复上传去重，未重复摄入 */
  duplicate?: boolean;
}

/** 问答引用来源 — 对应 Rust Source */
export interface RagSource {
  kb_id: string;
  doc_title: string;
  content: string;
  score: number;
}

/** 问答结果 — 对应 Rust AskResult */
export interface AskResult {
  answer: string;
  sources: RagSource[];
}

/** 检索命中分块 — 对应 Rust RetrievalHit */
export interface RetrievalHit {
  doc_id: string;
  doc_title: string;
  content: string;
  /** 余弦相似度（0~1，越大越相关） */
  score: number;
}

/** 索引状态 — 对应 Rust IndexStatus */
export interface IndexStatus {
  /** 文档数 */
  doc_count: number;
  /** 分块总数 */
  chunk_count: number;
  /** 已向量化的分块数 */
  embedded_count: number;
  /** 嵌入模型与知识库当前模型不一致的分块数（需重建索引） */
  stale_count: number;
  /** 全部分块的 token 总数 */
  total_tokens: number;
  /** 知识库当前绑定的嵌入模型（判定 stale 的基准） */
  embedding_model: string;
  /** 全部分块都已向量化 */
  is_complete: boolean;
  /** 存在 stale 分块 */
  is_stale: boolean;
}

/** MCP 端点（运行态）— 对应 Rust McpListenAddr */
export interface McpListenAddr {
  host: string;
  port: number;
}

/** MCP 服务运行态 — 对应 Rust McpStatus */
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

/** 知识库文档 — 对应 Rust KbDocument */
export interface KbDocument {
  id: string;
  kb_id: string;
  title: string;
  /** text | file | url | git */
  source_type: string;
  source_ref: string;
  char_count: number;
  chunk_count: number;
  /** 0=处理中 1=已就绪 2=失败 */
  status: number;
  error_message: string | null;
  /** 原始文件字节数 */
  file_size: number;
  /** 该文档分块 token 总数 */
  token_count: number;
  created_at: string;
  updated_at: string;
}

/** 文档分片摘要 — 对应 Rust DocumentChunk（content 为完整文本，前端按需内联展开） */
export interface KbDocumentChunk {
  seq: number;
  token_count: number;
  symbol_name: string | null;
  symbol_kind: string | null;
  /** 语言标识（如 python / rust / javascript / markdown / text），用于前端 CodeBlock 预览的语言切换 */
  language: string | null;
  line_start: number | null;
  line_end: number | null;
  content: string;
}

/** 分页分片结果 — 对应 Rust DocumentChunksPage */
export interface KbDocumentChunksPage {
  total: number;
  chunks: KbDocumentChunk[];
}

/** 摄入来源 — 对应 Rust KbSource（不含 token 字段） */
export interface KbSource {
  id: string;
  kb_id: string;
  /** git | url | local_dir */
  source_type: string;
  repo_url: string | null;
  branch: string | null;
  url: string | null;
  dir_path: string | null;
  subpath: string | null;
  /** fetching | done | error */
  status: string;
  file_count: number;
  error_message: string | null;
  created_at: string;
  updated_at: string;
}

/** 导入来源参数 — 对应 Rust ImportSourceInput */
export interface ImportSourceInput {
  /** git | url | local_dir */
  source_type: string;
  repo_url?: string;
  branch?: string;
  token?: string;
  url?: string;
  dir_path?: string;
  subpath?: string;
  /** 逗号分隔 */
  excluded_dirs?: string;
  /** 逗号分隔 */
  included_files?: string;
  /** MB */
  max_file_size_mb?: number;
}

// ============================================================
// Wiki
// Wiki 项目：以「源」为输入，由 LLM 阅读消化后生成结构化的「页面」，
// 页面之间通过 [[wikilink]] 交叉引用，并在后续摄入中增量更新。
// 与 RAG 的区别：RAG 每次检索原文片段（不积累），Wiki 沉淀为页面（会积累）。
// ============================================================

/** Wiki 项目状态：0 禁用 / 1 就绪 */
export type WikiProjectStatus = 0 | 1;

export interface WikiProject {
  id: string;
  name: string;
  description: string;
  /** 生成页面所用的渠道 id */
  channel_id: string;
  /** 生成页面所用的模型名 */
  model: string;
  /** 维护规则：约束页面生成与增量更新风格的 system 提示片段 */
  maintenance_prompt: string;
  /** 对话（搜索/问答）所用渠道 id；默认与生成渠道一致，可单独指定 */
  chat_channel_id: string;
  /** 对话（搜索/问答）所用模型名 */
  chat_model: string;
  /** MCP 暴露开关（预留：后端接入后启用，将 Wiki 以 MCP 工具暴露给外部 Agent） */
  mcp_exposed: number;
  status: WikiProjectStatus;
  /** 已配置来源数 */
  source_count: number;
  /** 已生成页面数（含目录页 index.md） */
  page_count: number;
  /** 页面间 [[wikilink]] 引用关系数 */
  link_count: number;
  /** 全部页面正文的 token 估算 */
  token_estimate: number;
  /** 最近一次摄入完成时间 */
  last_ingest_at: string | null;
  created_at: string;
  updated_at: string;
}

/** 新建 Wiki 项目参数（空白项目，源留到详情页添加） */
export interface WikiProjectInput {
  name: string;
  description: string;
  channel_id: string;
  model: string;
  /** MCP 暴露开关（预留） */
  mcp_exposed?: number;
}

/** 更新 Wiki 项目参数（部分更新） */
export interface WikiProjectUpdate {
  name?: string;
  description?: string;
  /** 0=禁用 1=就绪 */
  status?: number;
  channel_id?: string;
  model?: string;
  maintenance_prompt?: string;
  /** 对话（搜索/问答）渠道 id（可选更新） */
  chat_channel_id?: string;
  /** 对话（搜索/问答）模型名（可选更新） */
  chat_model?: string;
  /** MCP 暴露开关（预留，可选更新） */
  mcp_exposed?: number;
}

/** 来源类型：git 仓库 / 网页 URL / 本地目录 */
export type WikiSourceKind = "git" | "url" | "local_dir";

/** 来源状态。注意：摄入中状态只存在于「源」粒度，不会冒泡成项目状态。 */
export type WikiSourceStatus = "pending" | "ingesting" | "ready" | "failed";

/** 页面分类。LLM 摄入时强制带 kind，用于页面 Tab 的分类过滤。 */
export type WikiPageKind = "概念" | "实体" | "日志" | "索引" | "摘要";

export interface WikiSource {
  id: string;
  project_id: string;
  kind: WikiSourceKind;
  /** git 仓库 URL / 网页 URL / 本地目录绝对路径 */
  locator: string;
  branch: string | null;
  status: WikiSourceStatus;
  /** 摄入进度：已处理文档数 */
  ingested: number;
  /** 摄入进度：文档总数 */
  total: number;
  /** 最近一次摄入的错误信息 */
  error: string | null;
  last_ingest_at: string | null;
  created_at: string;
}

export interface WikiSourceInput {
  kind: WikiSourceKind;
  locator: string;
  branch?: string;
}

export interface WikiPage {
  id: string;
  project_id: string;
  title: string;
  /** URL 友好标识，用于 [[wikilink]] 定位 */
  slug: string;
  /** Markdown 正文 */
  content: string;
  /** 是否为目录页 index.md（查询引擎的导航入口，列表中置顶） */
  is_index: boolean;
  /** 页面分类：概念/实体/日志/索引/摘要 */
  kind: WikiPageKind;
  /** 正文中 [[wikilink]] 指向的页面标题 */
  links: string[];
  tokens: number;
  updated_at: string;
  created_at: string;
}

/** Wiki 问答引用到的页面片段 */
export interface WikiCitation {
  title: string;
  slug: string;
  /** 命中片段节选 */
  excerpt: string;
}

export interface WikiAskResult {
  answer: string;
  citations: WikiCitation[];
  prompt_tokens: number;
  completion_tokens: number;
  duration_ms: number;
}
