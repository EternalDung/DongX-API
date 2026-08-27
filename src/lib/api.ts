/**
 * DongX 前端 API 封装层
 *
 * 统一封装所有 Tauri invoke 调用（管理面）。
 * 数据面（Axum HTTP 代理）由外部客户端直接请求 http://127.0.0.1:9842，不经过此层。
 *
 * 设计约定：
 * - 按业务模块分组为对象，调用方式：channelApi.list() / keyApi.create(input)
 * - 每个 invoke 对应一个后端 #[tauri::command]
 * - 参数命名使用 snake_case，与 Rust serde 字段对齐
 * - 返回值带 TypeScript 类型，和 src/types/index.ts 保持一致
 * - 错误由 invoke 本身抛出，调用方用 try/catch 捕获
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  Channel,
  ChannelProtocolPresetGroup,
  ApiKey,
  RequestLog,
  AuditEvent,
  DashboardStats,
  Settings,
} from "@/types";

// ============================================================
// 输入类型定义（与 Rust command struct 一一对应）
// ============================================================

/** 单个上游 key + 权重（负载均衡） */
export interface KeyEntry {
  key: string;
  weight: number;
}

/** 渠道创建/更新参数 — 对应 Rust ChannelInput */
export interface ChannelInput {
  name: string;
  protocol: string;
  type: string;
  base_url: string;
  keys: KeyEntry[];
  models: string[];
  priority: number;
  weight: number;
  config: Record<string, unknown>;
  model_mapping: Record<string, string>;
  endpoints: string[];
  timeout_secs: number;
}

/** 密钥创建/更新参数 — 对应 Rust ApiKeyInput */
export interface ApiKeyInput {
  name: string;
  allowed_models: string[];
  allowed_channels: string[];
  quota_limit: number;
  expires_at: string | null;
}

/** 日志查询过滤 — 对应 Rust LogQuery */
export interface LogQuery {
  keyword?: string;
  channel_name?: string;
  model?: string;
  status_code?: number;
  start_time?: string;
  end_time?: string;
  page?: number;
  page_size?: number;
}

/** 审计事件查询过滤 — 对应 Rust AuditQuery */
export interface AuditQuery {
  severity?: string;
  event_type?: string;
  start_time?: string;
  end_time?: string;
  page?: number;
  page_size?: number;
}

/** 设置部分更新参数 — 对应 Rust SettingsUpdate（所有字段可选） */
export interface SettingsUpdate {
  server_port?: number;
  server_host?: string;
  ui_theme?: string;
  ui_language?: string;
  minimize_to_tray?: boolean;
  close_to_tray?: boolean;
  auto_start?: boolean;
  retry_enabled?: boolean;
  retry_times?: number;
  log_retention_days?: number;
  log_raw_body?: boolean;
  security_enabled?: boolean;
  security_mode?: string;
}

// ============================================================
// 渠道管理 API
// 管理上游 LLM 供应商渠道（OpenAI / DeepSeek / Claude / Ollama 等）
// ============================================================

export const channelApi = {
  /** 获取所有渠道列表 */
  list: (): Promise<Channel[]> => invoke<Channel[]>("list_channels"),

  /** 创建新渠道 */
  create: (input: ChannelInput): Promise<Channel> =>
    invoke<Channel>("create_channel", { input }),

  /** 更新已有渠道 */
  update: (id: string, input: ChannelInput): Promise<void> =>
    invoke<void>("update_channel", { id, input }),

  /** 删除渠道 */
  remove: (id: string): Promise<void> =>
    invoke<void>("delete_channel", { id }),

  /** 测试渠道连通性，返回是否成功 */
  test: (id: string): Promise<boolean> =>
    invoke<boolean>("test_channel", { id }),

  /** 获取按协议分组的供应商预设（渠道类型选择器的唯一数据来源） */
  presets: (): Promise<ChannelProtocolPresetGroup[]> =>
    invoke<ChannelProtocolPresetGroup[]>("list_provider_presets"),

  /** 获取某供应商适配器的默认模型列表（用于"拉取模型"按钮） */
  fetchModels: (type: string): Promise<string[]> =>
    invoke<string[]>("list_provider_models", { type }),
};

// ============================================================
// 密钥管理 API
// 管理网关对外发放的 sk-dong-* 密钥，控制访问权限与配额
// ============================================================

export const keyApi = {
  /** 获取所有网关密钥列表 */
  list: (): Promise<ApiKey[]> => invoke<ApiKey[]>("list_api_keys"),

  /** 创建新密钥，返回包含明文 key 的对象（仅此一次展示明文） */
  create: (input: ApiKeyInput): Promise<{ id: string; key: string; status: string }> =>
    invoke<{ id: string; key: string; status: string }>("create_api_key", { input }),

  /** 更新密钥配置 */
  update: (id: string, input: ApiKeyInput): Promise<void> =>
    invoke<void>("update_api_key", { id, input }),

  /** 删除密钥 */
  remove: (id: string): Promise<void> =>
    invoke<void>("delete_api_key", { id }),

  /** 启用 / 禁用密钥（status: 0=禁用 1=启用） */
  setStatus: (id: string, status: number): Promise<void> =>
    invoke<void>("set_api_key_status", { id, status }),
};

// ============================================================
// 请求日志 API
// 查询经过网关代理的 LLM 请求记录，含 token 用量与耗时
// ============================================================

export const logApi = {
  /** 查询请求日志（支持按渠道、模型、状态码、时间范围过滤） */
  list: (query?: LogQuery): Promise<RequestLog[]> =>
    invoke<RequestLog[]>("list_logs", { query }),

  /** 获取单条日志详情（含完整 request/response body） */
  detail: (id: string): Promise<RequestLog> =>
    invoke<RequestLog>("get_log_detail", { id }),

  /** 清空日志，可选只清理 N 天前的记录 */
  clear: (olderThanDays?: number): Promise<void> =>
    invoke<void>("clear_logs", { olderThanDays }),

  /** 删除单条日志 */
  delete: (id: string): Promise<number> =>
    invoke<number>("delete_log", { id }),
};

// ============================================================
// 安全审计 API
// 查询安全事件：限流触发、无效密钥、配额耗尽、可疑请求等
// ============================================================

export const auditApi = {
  /** 查询审计事件（支持按严重级别、事件类型、时间范围过滤） */
  list: (query?: AuditQuery): Promise<AuditEvent[]> =>
    invoke<AuditEvent[]>("list_audit_events", { query }),
};

// ============================================================
// 系统设置 API
// 全局配置读写
// ============================================================

export const settingsApi = {
  /** 获取完整系统设置 */
  get: (): Promise<Settings> => invoke<Settings>("get_settings"),

  /** 部分更新系统设置（只需传变更字段） */
  update: (update: SettingsUpdate): Promise<Settings> =>
    invoke<Settings>("update_settings", { update }),
};

// ============================================================
// 仪表盘 API
// 首页统计数据聚合
// ============================================================

export const statsApi = {
  /** 获取仪表盘统计数据（今日请求量、token 用量、活跃渠道等） */
  getDashboard: (): Promise<DashboardStats> =>
    invoke<DashboardStats>("get_dashboard_stats"),
};
