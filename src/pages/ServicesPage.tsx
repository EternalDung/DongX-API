import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Plus, Trash2, BookOpen, Globe, Zap, RefreshCw, AlertTriangle, Copy, Check, Terminal, Layers, Wifi, Server, Code2 } from "lucide-react";
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/tabs";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusBadge } from "@/components/ui/status-badge";
import { Switch } from "@/components/ui/switch";
import { useToast } from "@/components/ui/toast";
import { CodeBlock } from "@/components/CodeBlock";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { knowledgeApi, mcpApi } from "@/lib/api";
import type { KnowledgeBase, KnowledgeBaseInput, McpStatus } from "@/types";

/** 服务分类标签（服务页右上角切换）。desc 用于标题下方动态描述（随激活页签切换）。 */
const TABS = [
  { id: "rag", label: "RAG", desc: "以知识库为单元进行检索增强，摄入文档后可在问答中检索并引用。", icon: BookOpen },
  { id: "wiki", label: "Wiki", desc: "Wiki 知识沉淀模块，后续接入。", icon: Globe },
  { id: "mcp", label: "MCP", desc: "将知识库以 MCP 工具暴露，供外部 Agent 直接调用检索与问答。", icon: Server },
  { id: "skill", label: "Skill", desc: "Skill 扩展模块，后续接入。", icon: Zap },
] as const;

function fmtTime(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

/**
 * 头像配色：基于名称 hash 选 6 种之一，保证不同 KB 视觉差异大。
 *
 * 采用「低透明度底 + 同色系文字 + 同色系细边框」而非高饱和实底白字：
 * 实底白字色块视觉权重过大，列表里一屏 6+ 个头像时会把注意力从 KB 名称上抢走。
 * 文字色需分深浅两套（浅色模式 600 级、深色模式 400 级），否则深色下对比度不足。
 */
const KB_AVATAR_BG = [
  "bg-blue-500/15 text-blue-600 ring-blue-500/25 dark:text-blue-400",
  "bg-emerald-500/15 text-emerald-600 ring-emerald-500/25 dark:text-emerald-400",
  "bg-violet-500/15 text-violet-600 ring-violet-500/25 dark:text-violet-400",
  "bg-amber-500/15 text-amber-600 ring-amber-500/25 dark:text-amber-400",
  "bg-rose-500/15 text-rose-600 ring-rose-500/25 dark:text-rose-400",
  "bg-cyan-500/15 text-cyan-600 ring-cyan-500/25 dark:text-cyan-400",
];

function avatarColor(name: string): string {
  let h = 0;
  for (let i = 0; i < name.length; i++) {
    h = (h * 31 + name.charCodeAt(i)) >>> 0;
  }
  return KB_AVATAR_BG[h % KB_AVATAR_BG.length];
}

/** 取 KB 名称的首个非空白字符作为头像字；空名时回落到「?」。 */
function avatarLetter(name: string): string {
  const t = name.trim();
  if (!t) return "?";
  // 中文 / 表情都显示原字符；否则取大写首字母。
  return t.charAt(0).toUpperCase();
}

/** 新建知识库表单。 */
interface KbForm {
  name: string;
  description: string;
  embedding_model: string;
}

function emptyForm(): KbForm {
  return { name: "", description: "", embedding_model: "text-embedding-3-small" };
}

// ---------------------------------------------------------------------------
// 单个知识库行（点击进入详情页）
// 操作区中的开关与删除按钮阻止冒泡，避免被行点击带去详情页。
// ---------------------------------------------------------------------------

function KnowledgeBaseRow({
  kb,
  onOpen,
  onDelete,
  onToggleStatus,
  onToggleMcpExposed,
  busy = false,
}: {
  kb: KnowledgeBase;
  onOpen: (kb: KnowledgeBase) => void;
  onDelete: (kb: KnowledgeBase) => void;
  onToggleStatus: (kb: KnowledgeBase, next: 0 | 1) => void;
  onToggleMcpExposed: (kb: KnowledgeBase, next: 0 | 1) => void;
  busy?: boolean;
}) {
  const enabled = kb.status === 1;
  const tone = enabled ? "success" : "secondary";
  const label = enabled ? "就绪" : "禁用";
  const bg = avatarColor(kb.name);
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onOpen(kb)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") onOpen(kb);
      }}
      className="group flex cursor-pointer items-center gap-4 rounded-lg border bg-card/30 px-4 py-4 transition-colors hover:bg-accent/50"
    >
      {/* 头像：hash(name) → 6 色 + 首字符 */}
      <div
        aria-hidden
        // ring 取代 shadow：淡底配投影会显脏，细边框更贴合同色系配色。
        className={cn(
          "flex h-12 w-12 shrink-0 items-center justify-center rounded-lg text-lg font-semibold ring-1 ring-inset",
          bg,
        )}
      >
        {avatarLetter(kb.name)}
      </div>

      {/* 主信息：名称 / 描述 / 文档·分片 / 嵌入模型 / 更新 */}
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="truncate text-base font-semibold">{kb.name}</span>
          <StatusBadge tone={tone}>{label}</StatusBadge>
        </div>
        {kb.description && (
          <p className="mt-1 line-clamp-1 text-sm text-muted-foreground">
            {kb.description}
          </p>
        )}
        <p className="mt-1.5 text-xs text-muted-foreground">
          {kb.doc_count} 文档 · {kb.chunk_count} 片段
          {kb.embedding_model ? ` · ${kb.embedding_model}` : ""}
          {kb.updated_at ? ` · 更新 ${fmtTime(kb.updated_at)}` : ""}
        </p>
      </div>

      {/* 操作区：MCP 暴露 / 启用 / 删除 —— 阻断行点击 */}
      <div
        className="flex shrink-0 items-end gap-4 pl-2"
        onClick={(e) => e.stopPropagation()}
      >
        <div className="flex flex-col items-center gap-1">
          <Switch
            checked={kb.mcp_exposed === 1}
            disabled={busy || !enabled}
            title={
              enabled
                ? kb.mcp_exposed === 1
                  ? "已暴露给 MCP，点击关闭"
                  : "未暴露给 MCP，点击开启"
                : "请先启用知识库"
            }
            onCheckedChange={(v) => onToggleMcpExposed(kb, v ? 1 : 0)}
          />
          <span className="text-[10px] text-muted-foreground">MCP</span>
        </div>
        <div className="flex flex-col items-center gap-1">
          <Switch
            checked={enabled}
            disabled={busy}
            title={enabled ? "已启用，点击禁用" : "已禁用，点击启用"}
            onCheckedChange={(v) => onToggleStatus(kb, v ? 1 : 0)}
          />
          <span className="text-[10px] text-muted-foreground">启用</span>
        </div>
        <Button
          variant="ghost"
          size="icon"
          title="删除知识库"
          className="text-destructive hover:bg-destructive/10 hover:text-destructive"
          disabled={busy}
          onClick={() => onDelete(kb)}
        >
          <Trash2 />
        </Button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 未实现分类的占位
// ---------------------------------------------------------------------------

function Placeholder({ name }: { name: string }) {
  return (
    <div className="flex flex-col items-center justify-center rounded-xl border border-dashed bg-card/50 py-20 text-center">
      <BookOpen className="h-8 w-8 text-muted-foreground/50" />
      <p className="mt-3 text-sm font-medium">{name} 服务</p>
      <p className="mt-1 text-xs text-muted-foreground">暂未实现，敬请期待</p>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 页面
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// MCP 服务 tab：运行态 + 端点 + 调用示例 + 工具清单
// 信息架构参考 waliapi 服务页 MCP tab，但端点与工具以 DongX 真实实现为准：
// 端点与网关同源（默认 http://127.0.0.1:9842/mcp），仅有 POST /mcp 与
// GET /mcp/tools（调试），无独立 SSE 端口；工具为 5 个。
// ---------------------------------------------------------------------------

function McpTab({ kbs }: { kbs: KnowledgeBase[] }) {
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [copiedKey, setCopiedKey] = useState<string | null>(null);
  const [conn, setConn] = useState<{ state: "idle" | "testing" | "ok" | "err"; msg?: string; n?: number }>({ state: "idle" });

  useEffect(() => {
    let alive = true;
    mcpApi
      .status()
      .then((s) => alive && setStatus(s))
      .catch(() => alive && setStatus(null))
      .finally(() => alive && setLoading(false));
    return () => {
      alive = false;
    };
  }, []);

  const endpoint = status?.endpoint ?? "http://127.0.0.1:9842/mcp";
  const running = status?.running ?? false;
  const exposedCount = kbs.filter((k) => k.mcp_exposed === 1 && k.status === 1).length;
  const toolCount = status?.toolsCount ?? 0;

  // 统一复制：用 key 区分多个复制源，各自短暂显示 ✓。
  const copy = (key: string, text: string) => {
    navigator.clipboard?.writeText(text);
    setCopiedKey(key);
    setTimeout(() => setCopiedKey((k) => (k === key ? null : k)), 2000);
  };

  // 向 MCP 端点 POST tools/list，验证端点真实可达且能返回工具清单。
  // 网关已开 CorsLayer::permissive()，前端跨域 fetch 不会被拦。
  const handleTest = async () => {
    setConn({ state: "testing" });
    try {
      const resp = await fetch(endpoint, {
        method: "POST",
        headers: { "Content-Type": "application/json" },
        body: JSON.stringify({ jsonrpc: "2.0", id: 1, method: "tools/list", params: {} }),
      });
      if (!resp.ok) {
        setConn({ state: "err", msg: `HTTP ${resp.status}` });
        return;
      }
      const data = await resp.json();
      if (Array.isArray(data?.result?.tools)) {
        setConn({ state: "ok", n: data.result.tools.length });
      } else if (data?.error) {
        setConn({ state: "err", msg: data.error.message ?? "未知错误" });
      } else {
        setConn({ state: "err", msg: "响应格式异常" });
      }
    } catch (e) {
      setConn({ state: "err", msg: e instanceof Error ? e.message : String(e) });
    }
  };

  const tools: { name: string; desc: string; required: string[] }[] = [
    { name: "search_knowledge_base", desc: "语义检索 RAG，返回匹配文本片段和相似度评分", required: ["kb_id", "query"] },
    { name: "list_knowledge_bases", desc: "列出所有已暴露的 RAG（ID / 名称 / 文档数）", required: [] },
    { name: "ask_knowledge_base", desc: "RAG 问答，基于检索内容生成回答并返回来源引用", required: ["kb_id", "question"] },
    { name: "read_document", desc: "读取指定文档的完整内容（含分片正文）", required: ["kb_id", "doc_id"] },
    { name: "get_knowledge_base_stats", desc: "获取 RAG 统计信息（文档数 / 切片数 / token 数）", required: ["kb_id"] },
  ];

  // 调用示例的三段 curl（同一份字符串既用于展示也用于复制）。
  const exInit = `curl -X POST ${endpoint} \\
  -H "Content-Type: application/json" \\
  -d '{"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2024-11-05","capabilities":{},"clientInfo":{"name":"demo","version":"1.0"}}}'`;
  const exList = `curl -X POST ${endpoint} \\
  -H "Content-Type: application/json" \\
  -d '{"jsonrpc":"2.0","id":2,"method":"tools/list","params":{}}'`;
  const exSearch = `curl -X POST ${endpoint} \\
  -H "Content-Type: application/json" \\
  -d '{"jsonrpc":"2.0","id":3,"method":"tools/call","params":{"name":"search_knowledge_base","arguments":{"kb_id":"<KB_ID>","query":"你的问题"}}}'`;

  return (
    <div className="space-y-6">
      {/* 头部说明 */}
      <div>
        <h2 className="text-lg font-semibold tracking-tight">MCP 服务</h2>
        <p className="mt-1 text-sm text-muted-foreground">
          通过 Model Context Protocol 把本地 RAG 暴露给 AI Agent / MCP 客户端（如 Claude Desktop、Cursor）。
          端点与网关同源，无需单独配置端口，开启知识库的「MCP 暴露」后即可被检索。
        </p>
      </div>

      {/* 状态 + 概览 */}
      <div className="grid gap-4 sm:grid-cols-2">
        <Card>
          <CardContent className="space-y-3 p-4">
            <div className="flex items-center justify-between">
              <div className="flex items-center gap-3">
                <span
                  className={cn(
                    "flex h-9 w-9 items-center justify-center rounded-lg",
                    running ? "bg-emerald-500/15 text-emerald-600" : "bg-rose-500/15 text-rose-600",
                  )}
                >
                  <Wifi size={18} />
                </span>
                <div>
                  <p className="text-sm font-medium">MCP 端点</p>
                  <p className={cn("text-xs", running ? "text-emerald-600" : "text-rose-500")}>
                    {loading ? "检测中…" : running ? "运行中" : "已停止"}
                  </p>
                </div>
              </div>
              <div className="flex flex-col items-end gap-2">
                <span
                  className={cn(
                    "rounded-full px-2 py-0.5 text-xs font-medium",
                    running ? "bg-emerald-500/15 text-emerald-600" : "bg-rose-500/15 text-rose-500",
                  )}
                >
                  {running ? "就绪" : "离线"}
                </span>
                <Button variant="outline" size="sm" onClick={handleTest} disabled={conn.state === "testing"}>
                  {conn.state === "testing" ? "测试中…" : "测试连接"}
                </Button>
              </div>
            </div>
            {conn.state !== "idle" && (
              <p
                className={cn(
                  "rounded-lg px-3 py-2 text-xs",
                  conn.state === "ok"
                    ? "bg-emerald-50 text-emerald-700 dark:bg-emerald-500/10 dark:text-emerald-400"
                    : conn.state === "err"
                      ? "bg-rose-50 text-rose-700 dark:bg-rose-500/10 dark:text-rose-400"
                      : "bg-muted text-muted-foreground",
                )}
              >
                {conn.state === "ok"
                  ? `连接成功，返回 ${conn.n ?? 0} 个工具`
                  : conn.state === "err"
                    ? `连接失败：${conn.msg}`
                    : "正在测试连接…"}
              </p>
            )}
          </CardContent>
        </Card>

        <Card>
          <CardContent className="flex items-center justify-between p-4">
            <div className="flex items-center gap-3">
              <span className="flex h-9 w-9 items-center justify-center rounded-lg bg-violet-500/15 text-violet-600">
                <Layers size={18} />
              </span>
              <div>
                <p className="text-sm font-medium">已暴露知识库</p>
                <p className="text-xs text-muted-foreground">可在 MCP 中被检索的启用知识库</p>
              </div>
            </div>
            <span className="text-2xl font-semibold tabular-nums">{exposedCount}</span>
          </CardContent>
        </Card>
      </div>

      {/* 端点 */}
      <Card>
        <CardContent className="space-y-3 p-5">
          <div className="flex items-center gap-2">
            <Terminal size={18} className="text-foreground" />
            <h3 className="text-sm font-semibold">MCP 端点</h3>
          </div>
          <div>
            <label className="mb-1 block text-xs font-medium text-muted-foreground">
              JSON-RPC over HTTP（仅 POST；浏览器直接访问会返回 405）
            </label>
            <div className="flex items-center gap-2">
              <code className="flex-1 truncate rounded-lg border bg-muted px-3 py-2 font-mono text-xs text-foreground">
                {endpoint}
              </code>
              <Button variant="outline" size="icon" onClick={() => copy("endpoint", endpoint)} title="复制端点">
                {copiedKey === "endpoint" ? <Check size={14} className="text-emerald-500" /> : <Copy size={14} />}
              </Button>
            </div>
          </div>
          <p className="rounded-lg bg-amber-50 px-3 py-2 text-xs text-amber-700 dark:bg-amber-500/10 dark:text-amber-400">
            ⚠️ 该端点仅接受 JSON-RPC POST 请求。可用{" "}
            <code className="rounded bg-amber-100 px-1 py-0.5 font-mono dark:bg-amber-500/20">
              GET {endpoint.replace(/\/$/, "")}/tools
            </code>{" "}
            调试查看工具清单。
          </p>
        </CardContent>
      </Card>

      {/* 调用示例 */}
      <Card>
        <CardContent className="space-y-4 p-5">
          <div className="flex items-center gap-2">
            <Code2 size={18} className="text-foreground" />
            <h3 className="text-sm font-semibold">调用示例（curl）</h3>
          </div>

          <div>
            <div className="mb-1 flex items-center justify-between">
              <label className="text-xs font-medium text-muted-foreground">1 · 初始化握手</label>
              <Button variant="outline" size="sm" className="h-7 gap-1 px-2 text-xs" onClick={() => copy("ex1", exInit)}>
                {copiedKey === "ex1" ? <Check size={13} className="text-emerald-500" /> : <Copy size={13} />}
                <span>复制</span>
              </Button>
            </div>
            <CodeBlock code={exInit} lang="bash" />
          </div>

          <div>
            <div className="mb-1 flex items-center justify-between">
              <label className="text-xs font-medium text-muted-foreground">2 · 列出工具</label>
              <Button variant="outline" size="sm" className="h-7 gap-1 px-2 text-xs" onClick={() => copy("ex2", exList)}>
                {copiedKey === "ex2" ? <Check size={13} className="text-emerald-500" /> : <Copy size={13} />}
                <span>复制</span>
              </Button>
            </div>
            <CodeBlock code={exList} lang="bash" />
          </div>

          <div>
            <div className="mb-1 flex items-center justify-between">
              <label className="text-xs font-medium text-muted-foreground">3 · 语义检索</label>
              <Button variant="outline" size="sm" className="h-7 gap-1 px-2 text-xs" onClick={() => copy("ex3", exSearch)}>
                {copiedKey === "ex3" ? <Check size={13} className="text-emerald-500" /> : <Copy size={13} />}
                <span>复制</span>
              </Button>
            </div>
            <CodeBlock code={exSearch} lang="bash" />
          </div>

          <p className="text-xs text-muted-foreground">
            所有工具遵循 MCP JSON-RPC 2.0 规范，仅接受 POST。跨知识库操作会强制校验「MCP 暴露」开关——未暴露的 KB 不会被检索命中。
          </p>
        </CardContent>
      </Card>

      {/* 工具清单 */}
      <Card>
        <CardContent className="space-y-2 p-5">
          <div className="mb-2 flex items-center gap-2">
            <Server size={18} className="text-foreground" />
            <h3 className="text-sm font-semibold">可用工具</h3>
            <span className="ml-auto text-xs text-muted-foreground">
              {conn.state === "ok" ? (conn.n ?? toolCount) : toolCount} 个工具
            </span>
          </div>
          {tools.map((t) => (
            <div key={t.name} className="flex items-start gap-3 rounded-lg bg-muted px-3 py-2.5">
              <code className="shrink-0 rounded bg-background px-1.5 py-0.5 font-mono text-[11px] font-medium text-foreground">
                {t.name}
              </code>
              <div className="min-w-0">
                <p className="text-xs text-muted-foreground">{t.desc}</p>
                {t.required.length > 0 && (
                  <p className="mt-0.5 text-[11px] text-muted-foreground/70">必填：{t.required.join("、")}</p>
                )}
              </div>
            </div>
          ))}
        </CardContent>
      </Card>
    </div>
  );
}

export function ServicesPage() {
  const navigate = useNavigate();
  const toast = useToast();

  // 激活页签（受控，用于标题区动态展示该页签的标题 + 描述）
  const [activeTab, setActiveTab] = useState<string>("rag");

  // 键盘 TAB 切换服务分类：Tab=下一个，Shift+Tab=上一个，到达末尾循环回开头。
  // 输入框/文本域/可编辑区内不劫持，保留浏览器正常的焦点移动。
  useEffect(() => {
    const onKey = (e: KeyboardEvent) => {
      if (e.key !== "Tab") return;
      const el = e.target as HTMLElement | null;
      const tag = el?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA" || el?.isContentEditable) return;
      e.preventDefault();
      const ids = TABS.map((t) => t.id);
      const idx = ids.indexOf(activeTab as (typeof ids)[number]);
      if (idx < 0) return;
      const delta = e.shiftKey ? -1 : 1;
      const next = ids[(idx + delta + ids.length) % ids.length];
      setActiveTab(next);
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [activeTab]);

  // RAG 知识库列表
  const [kbs, setKbs] = useState<KnowledgeBase[]>([]);
  const [loading, setLoading] = useState(true);
  const [deleteTarget, setDeleteTarget] = useState<KnowledgeBase | null>(null);
  const [deleting, setDeleting] = useState(false);

  // 新建知识库对话框
  const [dialogOpen, setDialogOpen] = useState(false);
  const [form, setForm] = useState<KbForm>(emptyForm());
  const [saving, setSaving] = useState(false);

  const load = async () => {
    setLoading(true);
    try {
      const list = await knowledgeApi.list();
      setKbs(list);
    } catch (e) {
      // 后端 RAG 模块（Phase 1）尚未接入时优雅降级为空状态，不刷错误提示。
      console.warn("知识库列表加载失败（RAG 后端可能未接入）：", e);
      setKbs([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const openCreate = () => {
    setForm(emptyForm());
    setDialogOpen(true);
  };

  const handleCreate = async () => {
    if (!form.name.trim()) return;
    setSaving(true);
    try {
      const input: KnowledgeBaseInput = {
        name: form.name.trim(),
        description: form.description.trim(),
        embedding_model: form.embedding_model.trim() || "text-embedding-3-small",
      };
      await knowledgeApi.create(input);
      toast.success("知识库已创建");
      setDialogOpen(false);
      await load();
    } catch (e) {
      const msg =
        e && typeof e === "object" && "message" in e
          ? String((e as { message: unknown }).message)
          : String(e);
      console.error("创建知识库失败：", e);
      // 直接展示后端返回的真实错误（中文，描述具体原因，如缺可用嵌入渠道），
      // 仅在拿不到任何消息时回退到通用提示。
      toast.error(msg || "创建失败，请重试");
    } finally {
      setSaving(false);
    }
  };

  const handleConfirmDelete = async () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setDeleting(true);
    try {
      await knowledgeApi.remove(target.id);
      toast.success(`已删除知识库「${target.name}」`);
      setDeleteTarget(null);
      await load();
    } catch (e) {
      console.error("删除知识库失败：", e);
      toast.error("删除失败，请重试");
    } finally {
      setDeleting(false);
    }
  };

  // 行内开关的乐观更新 + 失败回滚。仅对单个 KB 标记 in-flight。
  const [togglingId, setTogglingId] = useState<string | null>(null);

  const patchKb = useCallback(
    async (kb: KnowledgeBase, patch: Partial<Pick<KnowledgeBase, "status" | "mcp_exposed">>, successMsg: string) => {
      setTogglingId(kb.id);
      // 乐观更新：立刻翻转 UI 反映"将要变到"的状态。
      const previous = { status: kb.status, mcp_exposed: kb.mcp_exposed };
      setKbs((prev) =>
        prev.map((k) =>
          k.id === kb.id ? { ...k, ...(patch.status !== undefined ? { status: patch.status } : {}), ...(patch.mcp_exposed !== undefined ? { mcp_exposed: patch.mcp_exposed } : {}) } : k,
        ),
      );
      try {
        await knowledgeApi.update(kb.id, patch as Parameters<typeof knowledgeApi.update>[1]);
        toast.success(successMsg);
      } catch (e) {
        // 失败回滚
        setKbs((prev) =>
          prev.map((k) =>
            k.id === kb.id ? { ...k, status: previous.status, mcp_exposed: previous.mcp_exposed } : k,
          ),
        );
        const msg =
          e && typeof e === "object" && "message" in e
            ? String((e as { message: unknown }).message)
            : String(e);
        console.error("更新知识库失败：", e);
        toast.error(msg || "更新失败，请重试");
      } finally {
        setTogglingId(null);
      }
    },
    [toast],
  );

  const handleToggleStatus = (kb: KnowledgeBase, next: 0 | 1) =>
    patchKb(kb, { status: next }, next === 1 ? "知识库已启用" : "知识库已禁用");

  const handleToggleMcpExposed = (kb: KnowledgeBase, next: 0 | 1) =>
    patchKb(kb, { mcp_exposed: next }, next === 1 ? "已暴露给 MCP" : "已关闭 MCP 暴露");

  return (
    <div>
      <Tabs value={activeTab} onValueChange={setActiveTab} className="w-full">
        {/* 标题 + 右上角分类标签：标题区随激活页签动态切换 */}
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="text-2xl font-semibold tracking-tight">
              {TABS.find((t) => t.id === activeTab)?.label}
            </h1>
            <p className="mt-1 text-sm text-muted-foreground">
              {TABS.find((t) => t.id === activeTab)?.desc}
            </p>
          </div>
          <TabsList>
            {TABS.map((t) => (
              <TabsTrigger
                key={t.id}
                value={t.id}
                className="gap-1.5 data-[state=active]:!bg-primary data-[state=active]:!text-primary-foreground data-[state=active]:!shadow-none"
              >
                <t.icon className="size-3.5" />
                {t.label}
              </TabsTrigger>
            ))}
          </TabsList>
        </div>

        {/* RAG：知识库列表 */}
        <TabsContent value="rag" className="mt-6">
          <div className="flex items-center justify-between">
            <h2 className="text-sm font-medium text-muted-foreground">知识库</h2>
            <div className="flex gap-2">
              <Button variant="outline" size="sm" onClick={load} disabled={loading}>
                <RefreshCw className={loading ? "animate-spin" : ""} />
                刷新
              </Button>
              <Button
                size="sm"
                onClick={openCreate}
                className="border border-primary/30 font-semibold shadow-sm"
              >
                <Plus />
                新建知识库
              </Button>
            </div>
          </div>

          <Card className="mt-3">
            <CardContent className="pt-2">
              {loading ? (
                <div className="space-y-2 py-4">
                  {Array.from({ length: 3 }).map((_, i) => (
                    <Skeleton key={i} className="h-14 w-full" />
                  ))}
                </div>
              ) : kbs.length === 0 ? (
                <EmptyState
                  icon={BookOpen}
                  title="暂无知识库"
                  description="知识库是 RAG 检索的数据源。新建一个知识库并摄入文档后，即可在问答中检索引用。"
                  action={
                    <Button size="sm" variant="outline" onClick={openCreate}>
                      <Plus />
                      新建第一个知识库
                    </Button>
                  }
                />
              ) : (
                <div className="space-y-3 py-2">
                  {kbs.map((kb) => (
                    <KnowledgeBaseRow
                      key={kb.id}
                      kb={kb}
                      busy={togglingId === kb.id || deleting}
                      onOpen={(k) => navigate(`/services/rag/${k.id}`)}
                      onDelete={setDeleteTarget}
                      onToggleStatus={handleToggleStatus}
                      onToggleMcpExposed={handleToggleMcpExposed}
                    />
                  ))}
                </div>
              )}
            </CardContent>
          </Card>
        </TabsContent>

        {/* 其余分类：暂未实现 */}
        <TabsContent value="wiki" className="mt-6">
          <Placeholder name="Wiki" />
        </TabsContent>
        <TabsContent value="mcp" className="mt-6">
          <McpTab kbs={kbs} />
        </TabsContent>
        <TabsContent value="skill" className="mt-6">
          <Placeholder name="Skill" />
        </TabsContent>
      </Tabs>

      {/* 新建知识库 Dialog */}
      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>新建知识库</DialogTitle>
            <DialogDescription>
              知识库是 RAG 检索的数据源。命名后摄入文档即可在问答中检索引用。
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-4">
            <div className="grid gap-2">
              <Label htmlFor="kb-name">名称</Label>
              <Input
                id="kb-name"
                placeholder="如：产品文档"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="kb-desc">描述</Label>
              <Input
                id="kb-desc"
                placeholder="可选，简述该知识库的用途"
                value={form.description}
                onChange={(e) => setForm({ ...form, description: e.target.value })}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="kb-embed">嵌入模型</Label>
              <Input
                id="kb-embed"
                placeholder="如 text-embedding-3-small"
                value={form.embedding_model}
                onChange={(e) => setForm({ ...form, embedding_model: e.target.value })}
                className="font-mono"
              />
              <p className="text-xs text-muted-foreground">
                用于把文档分块向量化的嵌入模型，需由某个已配置的渠道提供。
              </p>
            </div>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDialogOpen(false)}>
              取消
            </Button>
            <Button
              onClick={handleCreate}
              disabled={saving || !form.name.trim()}
            >
              {saving ? "创建中..." : "创建"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 删除确认 Dialog */}
      <Dialog
        open={deleteTarget !== null}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              删除知识库
            </DialogTitle>
            <DialogDescription>
              确认删除知识库「{deleteTarget?.name}」？其下全部文档与向量分块将一并移除，操作不可恢复。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>
              取消
            </Button>
            <Button
              variant="destructive"
              onClick={handleConfirmDelete}
              disabled={deleting}
            >
              {deleting ? "删除中..." : "确认删除"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
