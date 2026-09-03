import { useCallback, useEffect, useState } from "react";
import { useNavigate } from "react-router-dom";
import { Plus, Trash2, BookOpen, RefreshCw, AlertTriangle } from "lucide-react";
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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { cn } from "@/lib/utils";
import { knowledgeApi } from "@/lib/api";
import type { KnowledgeBase, KnowledgeBaseInput } from "@/types";

/** 服务分类标签（服务页右上角切换）。 */
const TABS = [
  { id: "rag", label: "RAG" },
  { id: "wiki", label: "Wiki" },
  { id: "mcp", label: "MCP" },
  { id: "skill", label: "Skill" },
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

export function ServicesPage() {
  const navigate = useNavigate();
  const toast = useToast();

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
      <Tabs defaultValue="rag" className="w-full">
        {/* 标题 + 右上角分类标签 */}
        <div className="flex items-start justify-between gap-4">
          <div>
            <h1 className="text-2xl font-semibold tracking-tight">服务</h1>
            <p className="mt-1 text-sm text-muted-foreground">
              管理网关的业务能力模块。RAG 以知识库为单元进行检索增强；Wiki / MCP / Skill 后续接入。
            </p>
          </div>
          <TabsList>
            {TABS.map((t) => (
              <TabsTrigger key={t.id} value={t.id}>
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
          <Placeholder name="MCP" />
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
