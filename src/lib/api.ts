/**
 * DongX 前端 API 封装层
 *
 * 统一封装所有 Tauri invoke 调用（管理面）。
 * 数据面（Axum HTTP 代理）由外部客户端直接请求 http://127.0.0.1:9842，不经过此层。
 *
 * 设计约定：
 * - 每个 invoke 调用对应一个后端 #[tauri::command]
 * - 参数命名使用 snake_case，与 Rust serde 字段对齐
 * - 返回值带 TypeScript 类型，和 src/types/index.ts 保持一致
 * - 错误由 invoke 本身抛出，调用方用 try/catch 捕获
 */

import { invoke } from "@tauri-apps/api/core";
import type {
  Channel,
  ProviderPreset,
  ApiKey,
  RequestLog,
  AuditEvent,
  DashboardStats,
  Settings,
} from "@/types";

// ============================================================
// 输入类型定义（与 Rust command struct 一一对应）
// ============================================================

/** 渠道创建/更新参数 — 对应 Rust ChannelInput */
export interface ChannelInput {
  name: string;
  protocol: string;
  type: string;
  base_url: string;
  api_key: string;
  models: string[];
  priority: number;
  weight: number;
  config: Record<string, unknown>;
  model_mapping: Record<string, string>;
  endpoints: string[];
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

/** 获取所有渠道列表 */
export function listChannels(): Promise<Channel[]> {
  return invoke<Channel[]>("list_channels");
}

/** 创建新渠道 */
export function createChannel(input: ChannelInput): Promise<Channel> {
  return invoke<Channel>("create_channel", { input });
}

/** 更新已有渠道 */
export function updateChannel(id: string, input: ChannelInput): Promise<void> {
  return invoke<void>("update_channel", { id, input });
}

/** 删除渠道 */
export function deleteChannel(id: string): Promise<void> {
  return invoke<void>("delete_channel", { id });
}

/** 测试渠道连通性，返回是否成功 */
export function testChannel(id: string): Promise<boolean> {
  return invoke<boolean>("test_channel", { id });
}

/** 获取供应商预设列表（用于新建渠道时选择类型） */
export function listProviderPresets(): Promise<ProviderPreset[]> {
  return invoke<ProviderPreset[]>("list_provider_presets");
}

// ============================================================
// 密钥管理 API
// 管理网关对外发放的 sk-dong-* 密钥，控制访问权限与配额
// ============================================================

/** 获取所有网关密钥列表 */
export function listApiKeys(): Promise<ApiKey[]> {
  return invoke<ApiKey[]>("list_api_keys");
}

/** 创建新密钥，返回包含明文 key 的对象（仅此一次展示明文） */
export function createApiKey(input: ApiKeyInput): Promise<{ id: string; key: string; status: string }> {
  return invoke<{ id: string; key: string; status: string }>("create_api_key", { input });
}

/** 更新密钥配置 */
export function updateApiKey(id: string, input: ApiKeyInput): Promise<void> {
  return invoke<void>("update_api_key", { id, input });
}

/** 删除密钥 */
export function deleteApiKey(id: string): Promise<void> {
  return invoke<void>("delete_api_key", { id });
}

// ============================================================
// 请求日志 API
// 查询经过网关代理的 LLM 请求记录，含 token 用量与耗时
// ============================================================

/** 查询请求日志（支持按渠道、模型、状态码、时间范围过滤） */
export function listLogs(query?: LogQuery): Promise<RequestLog[]> {
  return invoke<RequestLog[]>("list_logs", { query });
}

/** 获取单条日志详情（含完整 request/response body） */
export function getLogDetail(id: string): Promise<RequestLog> {
  return invoke<RequestLog>("get_log_detail", { id });
}

/** 清空日志，可选只清理 N 天前的记录 */
export function clearLogs(olderThanDays?: number): Promise<void> {
  return invoke<void>("clear_logs", { olderThanDays });
}

// ============================================================
// 安全审计 API
// 查询安全事件：限流触发、无效密钥、配额耗尽、可疑请求等
// ============================================================

/** 查询审计事件（支持按严重级别、事件类型、时间范围过滤） */
export function listAuditEvents(query?: AuditQuery): Promise<AuditEvent[]> {
  return invoke<AuditEvent[]>("list_audit_events", { query });
}

// ============================================================
// 系统设置 & 仪表盘 API
// 全局配置读写 + 首页统计数据聚合
// ============================================================

/** 获取完整系统设置 */
export function getSettings(): Promise<Settings> {
  return invoke<Settings>("get_settings");
}

/** 部分更新系统设置（只需传变更字段） */
export function updateSettings(update: SettingsUpdate): Promise<Settings> {
  return invoke<Settings>("update_settings", { update });
}

/** 获取仪表盘统计数据（今日请求量、token 用量、活跃渠道等） */
export function getDashboardStats(): Promise<DashboardStats> {
  return invoke<DashboardStats>("get_dashboard_stats");
}
