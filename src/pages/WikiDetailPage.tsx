import { useCallback, useEffect, useMemo, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import {
  ArrowLeft,
  LayoutDashboard,
  FileText,
  Database,
  Network,
  Settings,
  Search,
  Trash2,
  RefreshCw,
  AlertTriangle,
  Loader2,
  GitBranch,
  Link2,
  FolderOpen,
  Sparkles,
  Play,
  CheckCircle2,
  XCircle,
  UploadCloud,
  Clock,
} from "lucide-react";
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/tabs";
import { Card, CardContent, CardHeader, CardTitle } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusBadge } from "@/components/ui/status-badge";
import { useToast } from "@/components/ui/toast";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { wikiApi, channelApi } from "@/lib/api";
import { sleep } from "@/lib/async";
import { useTabKeyNavigation } from "@/hooks/useTabKeyNavigation";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import WikiGraphPrototype from "@/components/wiki/WikiGraphPrototype";
import type {
  Channel,
  WikiAskResult,
  WikiPage,
  WikiPageKind,
  WikiProject,
  WikiSource,
  WikiSourceKind,
  WikiSourceStatus,
} from "@/types";

/** 顶部水平 Tabs：概览置前，图谱 v1 不实现故禁用占位。 */
const WIKI_TABS = [
  { id: "overview", label: "概览", icon: LayoutDashboard, disabled: false },
  { id: "pages", label: "页面", icon: FileText, disabled: false },
  { id: "sources", label: "源", icon: Database, disabled: false },
  { id: "ask", label: "搜索", icon: Search, disabled: false },
  { id: "graph", label: "图谱", icon: Network, disabled: false },
  { id: "settings", label: "设置", icon: Settings, disabled: false },
] as const;

function fmtTime(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function errMsg(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) return String((e as { message: unknown }).message);
  return String(e);
}

function fmtTokens(n: number): string {
  if (n >= 1000) return `${(n / 1000).toFixed(1)}k`;
  return String(n);
}

/** 来源类型的展示元数据。 */
const SOURCE_KIND: Record<WikiSourceKind, { label: string; icon: typeof GitBranch }> = {
  git: { label: "Git 仓库", icon: GitBranch },
  url: { label: "网页", icon: Link2 },
  local_dir: { label: "本地目录", icon: FolderOpen },
};

const SOURCE_STATUS: Record<
  WikiSourceStatus,
  { label: string; tone: "success" | "warning" | "destructive" | "secondary" }
> = {
  pending: { label: "待摄入", tone: "secondary" },
  ingesting: { label: "摄入中", tone: "warning" },
  ready: { label: "就绪", tone: "success" },
  failed: { label: "失败", tone: "destructive" },
};

function SourceStatusIcon({ status }: { status: WikiSourceStatus }) {
  if (status === "ready") return <CheckCircle2 size={14} className="text-emerald-500" />;
  if (status === "failed") return <XCircle size={14} className="text-rose-500" />;
  if (status === "ingesting") return <Loader2 size={14} className="animate-spin text-amber-500" />;
  return <Clock size={14} className="text-muted-foreground" />;
}

// ---------------------------------------------------------------------------
// 概览 Tab：项目统计 + 描述 + 最近更新页面 + 来源状态
// ---------------------------------------------------------------------------

function OverviewTab({
  project,
  pages,
  sources,
  onOpenPage,
}: {
  project: WikiProject;
  pages: WikiPage[];
  sources: WikiSource[];
  onOpenPage: (p: WikiPage) => void;
}) {
  const recent = useMemo(
    () => pages.filter((p) => !p.is_index).slice().sort((a, b) => b.updated_at.localeCompare(a.updated_at)).slice(0, 5),
    [pages],
  );

  const byStatus = useMemo(() => {
    const acc: Record<WikiSourceStatus, number> = { pending: 0, ingesting: 0, ready: 0, failed: 0 };
    sources.forEach((s) => {
      acc[s.status] += 1;
    });
    return acc;
  }, [sources]);

  const stats = [
    { label: "来源", value: project.source_count, icon: Database },
    { label: "页面", value: project.page_count, icon: FileText },
    { label: "引用关系", value: project.link_count, icon: Link2 },
    { label: "Token 估算", value: fmtTokens(project.token_estimate), icon: Sparkles },
  ];

  return (
    <div className="space-y-5">
      <Card>
        <CardHeader>
          <CardTitle className="text-sm">项目描述</CardTitle>
        </CardHeader>
        <CardContent className="pt-0">
          <p className="text-sm leading-relaxed text-muted-foreground">
            {project.description || "（未填写描述）"}
          </p>
        </CardContent>
      </Card>

      <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-4">
        {stats.map((s) => {
          const Icon = s.icon;
          return (
            <Card key={s.label}>
              <CardContent className="flex items-center gap-3 p-4">
                <span className="flex h-9 w-9 items-center justify-center rounded-lg bg-muted text-muted-foreground">
                  <Icon size={16} />
                </span>
                <div className="min-w-0">
                  <p className="text-lg font-semibold tabular-nums leading-tight">{s.value}</p>
                  <p className="text-xs text-muted-foreground">{s.label}</p>
                </div>
              </CardContent>
            </Card>
          );
        })}
      </div>

      <div className="grid gap-4 lg:grid-cols-2">
        <Card>
          <CardHeader>
            <CardTitle className="text-sm">来源状态</CardTitle>
          </CardHeader>
          <CardContent className="space-y-2 pt-0">
            {(Object.keys(SOURCE_STATUS) as WikiSourceStatus[]).map((k) => (
              <div key={k} className="flex items-center justify-between text-sm">
                <span className="flex items-center gap-2 text-muted-foreground">
                  <SourceStatusIcon status={k} />
                  {SOURCE_STATUS[k].label}
                </span>
                <span className="tabular-nums">{byStatus[k]}</span>
              </div>
            ))}
            <p className="pt-2 text-xs text-muted-foreground">
              最近摄入：{project.last_ingest_at ? fmtTime(project.last_ingest_at) : "尚未摄入"}
            </p>
          </CardContent>
        </Card>

        <Card>
          <CardHeader>
            <CardTitle className="text-sm">最近更新的页面</CardTitle>
          </CardHeader>
          <CardContent className="pt-0">
            {recent.length === 0 ? (
              <p className="py-4 text-center text-xs text-muted-foreground">还没有页面，去「源」Tab 添加来源并摄入</p>
            ) : (
              <div className="space-y-1">
                {recent.map((p) => (
                  <button
                    key={p.id}
                    type="button"
                    onClick={() => onOpenPage(p)}
                    className="flex w-full items-center justify-between gap-2 rounded-lg px-2 py-1.5 text-left transition-colors hover:bg-accent/60"
                  >
                    <span className="truncate text-sm">{p.title}</span>
                    <span className="shrink-0 text-xs text-muted-foreground">{fmtTime(p.updated_at)}</span>
                  </button>
                ))}
              </div>
            )}
          </CardContent>
        </Card>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 页面 Tab：列表（index.md 置顶）+ 关键词过滤 + 预览弹窗
// ---------------------------------------------------------------------------

const PAGE_KIND_STYLE: Record<WikiPageKind, string> = {
  概念: "bg-sky-500/10 text-sky-600 dark:text-sky-400",
  实体: "bg-emerald-500/10 text-emerald-600 dark:text-emerald-400",
  日志: "bg-amber-500/10 text-amber-600 dark:text-amber-400",
  索引: "bg-violet-500/10 text-violet-600 dark:text-violet-400",
  摘要: "bg-slate-500/10 text-slate-600 dark:text-slate-400",
};

function PagesTab({
  pages,
  loading,
  onOpenPage,
}: {
  pages: WikiPage[];
  loading: boolean;
  onOpenPage: (p: WikiPage) => void;
}) {
  const [q, setQ] = useState("");
  const [kindFilter, setKindFilter] = useState("");

  const filtered = useMemo(() => {
    const kw = q.trim().toLowerCase();
    return pages.filter((p) => {
      if (kindFilter && p.kind !== kindFilter) return false;
      if (!kw) return true;
      return (
        p.title.toLowerCase().includes(kw) || p.content.toLowerCase().includes(kw)
      );
    });
  }, [pages, q, kindFilter]);

  if (loading) {
    return (
      <div className="space-y-2">
        {Array.from({ length: 4 }).map((_, i) => (
          <Skeleton key={i} className="h-16 w-full" />
        ))}
      </div>
    );
  }

  return (
    <div className="space-y-3">
      <div className="rounded-xl border bg-card p-3 shadow-sm">
        <div className="flex gap-2">
          <div className="relative flex-1">
            <Search size={14} className="absolute left-3 top-1/2 -translate-y-1/2 text-muted-foreground" />
            <Input
              value={q}
              onChange={(e) => setQ(e.target.value)}
              placeholder="按标题或正文过滤页面"
              className="pl-8"
            />
          </div>
          <Select
            value={kindFilter || undefined}
            onValueChange={(v) => setKindFilter(v === "__all__" ? "" : v)}
          >
            <SelectTrigger className="w-32">
              <SelectValue placeholder="全部分类" />
            </SelectTrigger>
            <SelectContent>
              <SelectItem value="__all__">全部分类</SelectItem>
              <SelectItem value="概念">概念</SelectItem>
              <SelectItem value="实体">实体</SelectItem>
              <SelectItem value="日志">日志</SelectItem>
              <SelectItem value="索引">索引</SelectItem>
              <SelectItem value="摘要">摘要</SelectItem>
            </SelectContent>
          </Select>
        </div>
      </div>

      <Card>
        <CardContent className="space-y-2 pt-2">
          {filtered.length === 0 ? (
        <EmptyState
          icon={FileText}
          title={q ? "没有匹配的页面" : "还没有页面"}
          description={q ? "换个关键词试试" : "添加来源并触发摄入后，模型会把资料消化成结构化页面"}
        />
      ) : (
        <div className="space-y-2">
          {filtered.map((p) => (
            <div
              key={p.id}
              role="button"
              tabIndex={0}
              onClick={() => onOpenPage(p)}
              onKeyDown={(e) => {
                if (e.key === "Enter" || e.key === " ") onOpenPage(p);
              }}
              className="cursor-pointer rounded-lg border px-4 py-3 transition-colors hover:bg-muted/50"
            >
              <div className="flex flex-wrap items-center gap-2">
                <span className="font-medium">{p.title}</span>
                <span className={`rounded px-1.5 py-0.5 text-[11px] ${PAGE_KIND_STYLE[p.kind]}`}>
                  {p.kind}
                </span>
                <span className="ml-auto text-xs text-muted-foreground">
                  {fmtTokens(p.tokens)} tokens · {fmtTime(p.updated_at)}
                </span>
              </div>
              <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
                {p.content.replace(/^#.*$/m, "").trim().slice(0, 140)}
              </p>
              {p.links.length > 0 && (
                <div className="mt-2 flex flex-wrap items-center gap-x-2 gap-y-1 text-[11px] text-muted-foreground">
                  <span>引用：</span>
                  {p.links.map((l) => {
                    const target = pages.find((x) => x.title === l);
                    return target ? (
                      <button
                        key={l}
                        type="button"
                        onClick={() => onOpenPage(target)}
                        className="rounded text-primary hover:underline"
                      >
                        [{l}]
                      </button>
                    ) : (
                      <span key={l} className="rounded text-muted-foreground/60 line-through">
                        [{l}]
                      </span>
                    );
                  })}
                </div>
              )}
            </div>
          ))}
        </div>
      )}
        </CardContent>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 源 Tab：来源管理 + 摄入（进度只在这一层，不冒泡成项目状态）
// ---------------------------------------------------------------------------

function SourcesTab({
  projectId,
  sources,
  loading,
  onChanged,
}: {
  projectId: string;
  sources: WikiSource[];
  /** 仅首屏为 true（loadChildren(true)），用于一次性骨架闸门；刷新时不置位，故不会闪 */
  loading: boolean;
  onChanged: () => void;
}) {
  const toast = useToast();
  const [uploading, setUploading] = useState<{ id: string; name: string }[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const [busyId, setBusyId] = useState<string | null>(null);
  const [deleteTarget, setDeleteTarget] = useState<WikiSource | null>(null);
  const [deleting, setDeleting] = useState(false);

  const pickFiles = async () => {
    try {
      const picked = await openDialog({
        multiple: true,
        filters: [
          {
            name: "资料文件",
            extensions: [
              "md", "markdown", "txt", "mdx", "json", "yaml", "yml", "csv",
              "log", "ts", "tsx", "js", "jsx", "py", "rs", "go", "java", "kt",
              "c", "cpp", "h", "sh", "toml", "xml", "html", "css",
            ],
          },
        ],
      });
      if (!picked) return;
      const paths = Array.isArray(picked) ? picked : [picked];
      await uploadPaths(paths);
    } catch {
      /* 非桌面环境或用户取消：忽略 */
    }
  };

  const uploadPaths = async (paths: string[]) => {
    for (const p of paths) {
      const uid = crypto.randomUUID();
      const name = p.split(/[\\/]/).pop() || p;
      setUploading((prev) => [...prev, { id: uid, name }]);
      try {
        const created = await wikiApi.addSource(projectId, {
          kind: "local_dir",
          locator: p,
        });
        await wikiApi.ingestSource(created.id);
        toast.success(`已开始摄入「${name}」`);
        onChanged();
      } catch (e) {
        console.error("添加来源失败：", e);
        toast.error(`添加「${name}」失败：${errMsg(e) || "请重试"}`);
      } finally {
        setUploading((prev) => prev.filter((u) => u.id !== uid));
      }
    }
  };

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    pickFiles();
  };

  const handleIngest = async (s: WikiSource) => {
    setBusyId(s.id);
    try {
      await wikiApi.ingestSource(s.id);
      toast.success("已开始摄入");
      onChanged();
    } catch (e) {
      console.error("触发摄入失败：", e);
      toast.error(errMsg(e) || "摄入失败，请重试");
    } finally {
      setBusyId(null);
    }
  };

  const handleConfirmDelete = async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      await wikiApi.removeSource(deleteTarget.id);
      toast.success("来源已删除");
      setDeleteTarget(null);
      onChanged();
    } catch (e) {
      console.error("删除来源失败：", e);
      toast.error("删除失败，请重试");
    } finally {
      setDeleting(false);
    }
  };

  if (loading) {
    return (
      <div className="space-y-2">
        {Array.from({ length: 2 }).map((_, i) => (
          <Skeleton key={i} className="h-16 w-full" />
        ))}
      </div>
    );
  }

  return (
    <div className="space-y-4">
      {/* 拖拽上传区：复制 RAG 文档上传组件 */}
      <div
        role="button"
        tabIndex={0}
        onClick={() => pickFiles()}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") pickFiles();
        }}
        onDragOver={(e) => {
          e.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={onDrop}
        className={`flex cursor-pointer flex-col items-center justify-center rounded-xl border-2 border-dashed bg-card px-6 py-10 text-center transition-colors ${
          dragOver
            ? "border-primary ring-2 ring-primary/20"
            : "border-muted-foreground/25 hover:border-primary/50"
        }`}
      >
        <UploadCloud
          className={`h-8 w-8 ${dragOver ? "text-primary" : "text-muted-foreground/60"}`}
        />
        <p className="mt-3 text-sm font-medium">拖拽文件到此处，或点击选择</p>
        <p className="mt-1 text-xs text-muted-foreground">
          支持 .md / .txt / .json / .yaml / 代码 / .pdf 等文本类文件
        </p>
      </div>

      {/* 上传中 */}
      {uploading.length > 0 && (
        <div className="space-y-1">
          {uploading.map((u) => (
            <div
              key={u.id}
              className="flex items-center gap-2 rounded-lg bg-muted/40 px-3 py-2 text-xs text-muted-foreground"
            >
              <Loader2 className="h-3.5 w-3.5 animate-spin" />
              正在摄入「{u.name}」…
            </div>
          ))}
        </div>
      )}

      <Card>
        <CardContent className="space-y-3 pt-2">
          <div className="flex items-center justify-between">
            <h3 className="text-sm font-medium text-muted-foreground">
              来源{sources.length > 0 && ` · ${sources.length}`}
            </h3>
          </div>

      {sources.length === 0 ? (
        <EmptyState
          icon={Database}
          title="还没有来源"
          description="拖拽或选择文件即可上传并自动摄入，生成结构化页面。"
        />
      ) : (
        <div className="space-y-2">
          {sources.map((s) => {
            const meta = SOURCE_KIND[s.kind];
            const Icon = meta.icon;
            const st = SOURCE_STATUS[s.status];
            const pct = s.total > 0 ? Math.round((s.ingested / s.total) * 100) : 0;
            return (
              <div key={s.id} className="rounded-lg border px-4 py-3 transition-colors hover:bg-muted/40">
                <div className="flex items-start gap-3">
                  <span className="mt-0.5 flex h-8 w-8 shrink-0 items-center justify-center rounded-lg bg-muted text-muted-foreground">
                    <Icon size={15} />
                  </span>
                  <div className="min-w-0 flex-1">
                    <div className="flex flex-wrap items-center gap-2">
                      <span className="truncate text-sm font-medium">{meta.label}</span>
                      <StatusBadge tone={st.tone}>{st.label}</StatusBadge>
                      {s.branch && (
                        <span className="text-xs text-muted-foreground">@{s.branch}</span>
                      )}
                    </div>
                    <p className="mt-0.5 text-xs text-muted-foreground">
                      {meta.label}
                      {s.total > 0 && ` · 摄入 ${s.ingested}/${s.total}`}
                      {s.last_ingest_at && ` · 上次 ${fmtTime(s.last_ingest_at)}`}
                    </p>
                    {s.status === "ingesting" && (
                      <div className="mt-2 h-1.5 w-full overflow-hidden rounded-full bg-muted">
                        <div
                          className="h-full rounded-full bg-amber-500/70 transition-all"
                          style={{ width: `${pct}%` }}
                        />
                      </div>
                    )}
                    {s.status === "failed" && s.error && (
                      <p className="mt-1.5 rounded bg-destructive/10 px-2 py-1 text-xs text-destructive">
                        {s.error}
                      </p>
                    )}
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
                    <Button
                      variant="ghost"
                      size="icon"
                      title={s.status === "ingesting" ? "正在摄入" : "重新摄入"}
                      disabled={busyId === s.id || s.status === "ingesting"}
                      onClick={() => handleIngest(s)}
                    >
                      {s.status === "ingesting" ? (
                        <Loader2 className="animate-spin" />
                      ) : (
                        <Play />
                      )}
                    </Button>
                    <Button
                      variant="ghost"
                      size="icon"
                      title="删除来源"
                      className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                      disabled={deleting}
                      onClick={() => setDeleteTarget(s)}
                    >
                      <Trash2 />
                    </Button>
                  </div>
                </div>
              </div>
            );
          })}
        </div>
      )}
        </CardContent>
      </Card>

      {/* 删除来源确认 */}
      <Dialog open={deleteTarget !== null} onOpenChange={(o) => !o && setDeleteTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              删除来源
            </DialogTitle>
            <DialogDescription>
              确认删除来源「{deleteTarget?.locator}」？已由其生成的页面会保留，但不再随该来源更新。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>
              取消
            </Button>
            <Button variant="destructive" onClick={handleConfirmDelete} disabled={deleting}>
              {deleting ? "删除中..." : "确认删除"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 问答 Tab：在项目知识体上提问，返回答案 + [[页面]] 引用
// v1 为单轮 + 会话内历史，会话不落库。
// ---------------------------------------------------------------------------

interface SearchTurn {
  question: string;
  result: WikiAskResult | null;
  error?: string;
}

function SearchTab({ projectId, disabled }: { projectId: string; disabled: boolean }) {
  const [question, setQuestion] = useState("");
  const [asking, setAsking] = useState(false);
  const [turns, setTurns] = useState<SearchTurn[]>([]);

  const handleAsk = async () => {
    const q = question.trim();
    if (!q) return;
    setAsking(true);
    setQuestion("");
    setTurns((prev) => [...prev, { question: q, result: null }]);
    try {
      const r = await wikiApi.ask(projectId, q);
      setTurns((prev) => prev.map((t, i) => (i === prev.length - 1 ? { ...t, result: r } : t)));
    } catch (e) {
      console.error("Wiki 问答失败：", e);
      setTurns((prev) =>
        prev.map((t, i) => (i === prev.length - 1 ? { ...t, error: errMsg(e) } : t)),
      );
    } finally {
      setAsking(false);
    }
  };

  return (
    <div className="space-y-4">
      <Card>
        <CardContent className="space-y-3 p-4">
          <div className="flex items-center gap-2">
            <Input
              value={question}
              onChange={(e) => setQuestion(e.target.value)}
              onKeyDown={(e) => {
                if (e.key === "Enter" && (e.metaKey || e.ctrlKey)) void handleAsk();
              }}
              placeholder={disabled ? "项目已禁用，启用后可搜索" : "搜索 Wiki 内容…（Ctrl/⌘ + Enter 发送）"}
              disabled={disabled || asking}
              className="flex-1"
            />
            <Button onClick={handleAsk} disabled={disabled || asking || !question.trim()}>
              {asking ? <Loader2 className="animate-spin" /> : <Search />}
              搜索
            </Button>
          </div>
          <p className="text-xs text-muted-foreground">
            v1 无向量：先按页面标题与正文的关键词检索命中相关页面，再由对话模型基于页面正文组织答案并给出引用。
          </p>
        </CardContent>
      </Card>

      {turns.length === 0 ? (
        <EmptyState
          icon={Search}
          title="还没有搜索"
          description="搜索会先检索相关页面，再由对话模型基于页面正文作答并给出引用"
        />
      ) : (
        <div className="space-y-3">
          {turns.map((t, i) => (
            <Card key={i}>
              <CardContent className="space-y-3 p-4">
                <p className="text-sm font-medium">{t.question}</p>
                {t.error ? (
                  <p className="rounded bg-destructive/10 px-3 py-2 text-xs text-destructive">
                    {t.error}
                  </p>
                ) : !t.result ? (
                  <div className="flex items-center gap-2 text-xs text-muted-foreground">
                    <Loader2 size={13} className="animate-spin" />
                    检索页面并生成答案…
                  </div>
                ) : (
                  <>
                    <p className="whitespace-pre-wrap text-sm leading-relaxed text-foreground">
                      {t.result.answer}
                    </p>
                    {t.result.citations.length > 0 && (
                      <div className="space-y-1.5">
                        <p className="text-xs font-medium text-muted-foreground">引用页面</p>
                        {t.result.citations.map((c) => (
                          <div key={c.slug} className="rounded-lg bg-muted px-3 py-2">
                            <p className="text-xs font-medium">[[{c.title}]]</p>
                            <p className="mt-0.5 line-clamp-2 text-xs text-muted-foreground">
                              {c.excerpt}
                            </p>
                          </div>
                        ))}
                      </div>
                    )}
                    <p className="text-xs text-muted-foreground">
                      {t.result.prompt_tokens} + {t.result.completion_tokens} tokens ·{" "}
                      {(t.result.duration_ms / 1000).toFixed(1)}s
                    </p>
                  </>
                )}
              </CardContent>
            </Card>
          ))}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// 设置 Tab：基础信息 + 生成配置 + 危险区
// ---------------------------------------------------------------------------

function SettingsTab({
  project,
  channels,
  onSaved,
  onDelete,
}: {
  project: WikiProject;
  channels: Channel[];
  onSaved: () => void;
  onDelete: () => void;
}) {
  const toast = useToast();
  const [name, setName] = useState(project.name);
  const [description, setDescription] = useState(project.description);
  const [channelId, setChannelId] = useState(project.channel_id);
  const [model, setModel] = useState(project.model);
  const [prompt, setPrompt] = useState(project.maintenance_prompt);
  const [chatChannelId, setChatChannelId] = useState(project.chat_channel_id);
  const [chatModel, setChatModel] = useState(project.chat_model);
  const [saving, setSaving] = useState(false);

  const models = useMemo(() => {
    const c = channels.find((x) => x.id === channelId);
    const list = (c?.models ?? []).slice().sort();
    if (model && !list.includes(model)) list.unshift(model);
    return list;
  }, [channels, channelId, model]);

  const chatModels = useMemo(() => {
    const c = channels.find((x) => x.id === chatChannelId);
    const list = (c?.models ?? []).slice().sort();
    if (chatModel && !list.includes(chatModel)) list.unshift(chatModel);
    return list;
  }, [channels, chatChannelId, chatModel]);

  const handleSave = async () => {
    setSaving(true);
    try {
      await wikiApi.update(project.id, {
        name: name.trim(),
        description: description.trim(),
        channel_id: channelId,
        model,
        maintenance_prompt: prompt,
        chat_channel_id: chatChannelId,
        chat_model: chatModel,
      });
      toast.success("设置已保存");
      onSaved();
    } catch (e) {
      console.error("保存 Wiki 设置失败：", e);
      toast.error(errMsg(e) || "保存失败，请重试");
    } finally {
      setSaving(false);
    }
  };

  const dirty =
    name !== project.name ||
    description !== project.description ||
    channelId !== project.channel_id ||
    model !== project.model ||
    chatChannelId !== project.chat_channel_id ||
    chatModel !== project.chat_model ||
    prompt !== project.maintenance_prompt;

  return (
    <div className="space-y-5">
      <Card>
        <CardHeader>
          <CardTitle className="text-sm">基础信息</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4 pt-0">
          <div className="grid gap-2">
            <Label htmlFor="set-name">名称</Label>
            <Input id="set-name" value={name} onChange={(e) => setName(e.target.value)} />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="set-desc">描述</Label>
            <Input id="set-desc" value={description} onChange={(e) => setDescription(e.target.value)} />
          </div>
        </CardContent>
      </Card>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">模型配置</CardTitle>
        </CardHeader>
        <CardContent className="grid gap-4 pt-0">
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="grid gap-2">
              <Label htmlFor="set-channel">生成渠道</Label>
              <Select
                value={channelId || undefined}
                onValueChange={(v) => {
                  setChannelId(v);
                  setModel("");
                }}
                disabled={channels.length === 0}
              >
                <SelectTrigger id="set-channel" className="w-full">
                  <SelectValue
                    placeholder={channels.length === 0 ? "无可用渠道" : "请选择渠道"}
                  />
                </SelectTrigger>
                <SelectContent>
                  {channels.map((c) => (
                    <SelectItem key={c.id} value={c.id}>
                      {c.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="set-model">生成模型</Label>
              <Select
                value={model || undefined}
                onValueChange={setModel}
                disabled={models.length === 0}
              >
                <SelectTrigger id="set-model" className="w-full">
                  <SelectValue placeholder={models.length === 0 ? "请先选渠道" : "请选择模型"} />
                </SelectTrigger>
                <SelectContent>
                  {models.map((m) => (
                    <SelectItem key={m} value={m}>
                      {m}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <div className="grid gap-4 sm:grid-cols-2">
            <div className="grid gap-2">
              <Label htmlFor="set-chat-channel">对话渠道</Label>
              <Select
                value={chatChannelId || undefined}
                onValueChange={(v) => {
                  setChatChannelId(v);
                  setChatModel("");
                }}
                disabled={channels.length === 0}
              >
                <SelectTrigger id="set-chat-channel" className="w-full">
                  <SelectValue
                    placeholder={channels.length === 0 ? "无可用渠道" : "请选择渠道"}
                  />
                </SelectTrigger>
                <SelectContent>
                  {channels.map((c) => (
                    <SelectItem key={c.id} value={c.id}>
                      {c.name}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
            <div className="grid gap-2">
              <Label htmlFor="set-chat-model">对话模型</Label>
              <Select
                value={chatModel || undefined}
                onValueChange={setChatModel}
                disabled={chatModels.length === 0}
              >
                <SelectTrigger id="set-chat-model" className="w-full">
                  <SelectValue
                    placeholder={chatModels.length === 0 ? "请先选渠道" : "请选择模型"}
                  />
                </SelectTrigger>
                <SelectContent>
                  {chatModels.map((m) => (
                    <SelectItem key={m} value={m}>
                      {m}
                    </SelectItem>
                  ))}
                </SelectContent>
              </Select>
            </div>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="set-prompt">维护规则</Label>
            <Textarea
              id="set-prompt"
              value={prompt}
              onChange={(e) => setPrompt(e.target.value)}
              placeholder="约束页面生成与增量更新风格，如：条目简短、以机制描述为主，不贴大段源码"
              className="min-h-[100px] resize-y"
            />
          </div>
          <p className="text-xs text-muted-foreground">
            生成渠道/模型用于阅读来源、产出与更新页面；对话渠道/模型用于在本页「搜索」中检索并组织答案。两者可分别指定。
          </p>
          <div className="flex justify-end">
            <Button onClick={handleSave} disabled={saving || !dirty || !name.trim()}>
              {saving ? "保存中..." : "保存设置"}
            </Button>
          </div>
        </CardContent>
      </Card>

      <Card className="border-destructive/30">
        <CardHeader>
          <CardTitle className="text-sm text-destructive">危险区</CardTitle>
        </CardHeader>
        <CardContent className="flex items-center justify-between pt-0">
          <p className="text-xs text-muted-foreground">
            删除项目会一并移除其下全部页面与来源配置，操作不可恢复。
          </p>
          <Button variant="destructive" size="sm" onClick={onDelete}>
            <Trash2 />
            删除项目
          </Button>
        </CardContent>
      </Card>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 详情页
// ---------------------------------------------------------------------------

export function WikiDetailPage() {
  const navigate = useNavigate();
  const { projectId } = useParams<{ projectId: string }>();
  const toast = useToast();

  const [project, setProject] = useState<WikiProject | null>(null);
  const [loading, setLoading] = useState(true);
  const [notFound, setNotFound] = useState(false);

  const [pages, setPages] = useState<WikiPage[]>([]);
  const [sources, setSources] = useState<WikiSource[]>([]);
  const [listLoading, setListLoading] = useState(true);
  const [refreshing, setRefreshing] = useState(false);

  const [channels, setChannels] = useState<Channel[]>([]);
  const [activeTab, setActiveTab] = useState("overview");

  // 键盘 Tab 切换顶部页签（与服务页一致）：Tab=下一个，Shift+Tab=上一个，首尾循环；
  // graph 为禁用占位项，已剔除，避免 Tab 落到禁用的「图谱」上。
  useTabKeyNavigation(
    WIKI_TABS.filter((t) => !t.disabled).map((t) => t.id),
    activeTab,
    setActiveTab,
  );

  const [preview, setPreview] = useState<WikiPage | null>(null);
  const [deleteOpen, setDeleteOpen] = useState(false);
  const [deleting, setDeleting] = useState(false);

  const loadProject = useCallback(async () => {
    if (!projectId) return;
    try {
      const list = await wikiApi.list();
      const found = list.find((p) => p.id === projectId) ?? null;
      if (!found) setNotFound(true);
      else setProject(found);
    } catch (e) {
      console.error("加载 Wiki 项目失败：", e);
      setNotFound(true);
    } finally {
      setLoading(false);
    }
  }, [projectId]);

  // skeleton=true 仅首屏展示骨架屏；后台轮询与手动刷新传 false，
  // 只更新数据、不把页面/源 Tab 清空成骨架，避免「每秒闪一下空白」的屏闪。
  const loadChildren = useCallback(async (skeleton: boolean) => {
    if (!projectId) return;
    if (skeleton) setListLoading(true);
    else setRefreshing(true);
    const started = Date.now();
    try {
      const [p, s] = await Promise.all([
        wikiApi.pages(projectId),
        wikiApi.sources(projectId),
      ]);
      setPages(p);
      setSources(s);
    } catch (e) {
      console.error("加载 Wiki 页面/来源失败：", e);
    } finally {
      // 数据立即落库（内容秒出），旋转/骨架态再保活至最小可见时长，不再拖慢内容。
      const elapsed = Date.now() - started;
      if (elapsed < 400) await sleep(400 - elapsed);
      if (skeleton) setListLoading(false);
      else setRefreshing(false);
    }
    void loadProject();
  }, [projectId, loadProject]);

  useEffect(() => {
    void loadProject();
    void loadChildren(true);
    channelApi
      .list()
      .then(setChannels)
      .catch(() => setChannels([]));
  }, [loadProject, loadChildren]);

  // 有来源在摄入时轮询，让进度条自己走完
  const ingesting = sources.some((s) => s.status === "ingesting");
  useEffect(() => {
    if (!ingesting) return;
    // 后台轮询：进度推进用，绝不触发骨架屏（见 loadChildren 的 skeleton 参数）。
    const t = setInterval(() => void loadChildren(false), 800);
    return () => clearInterval(t);
  }, [ingesting, loadChildren]);

  const handleConfirmDelete = async () => {
    if (!project) return;
    setDeleting(true);
    try {
      await wikiApi.remove(project.id);
      toast.success(`已删除 Wiki 项目「${project.name}」`);
      navigate("/services?tab=wiki");
    } catch (e) {
      console.error("删除 Wiki 项目失败：", e);
      toast.error("删除失败，请重试");
    } finally {
      setDeleting(false);
    }
  };

  if (loading) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-9 w-full" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }

  if (notFound || !project) {
    return (
      <div className="flex flex-col items-center justify-center py-24 text-center">
        <p className="text-sm font-medium">未找到该 Wiki 项目</p>
        <p className="mt-1 text-xs text-muted-foreground">它可能已被删除，或链接已失效。</p>
        <Button variant="outline" size="sm" className="mt-4" onClick={() => navigate("/services?tab=wiki")}>
          <ArrowLeft />
          返回服务
        </Button>
      </div>
    );
  }

  return (
    <div>
      {/* 面包屑：返回 + 项目名 + 状态。统计信息已移入「概览」Tab */}
      <div className="flex items-center gap-3">
        <Button variant="ghost" size="icon" title="返回服务" onClick={() => navigate("/services?tab=wiki")}>
          <ArrowLeft />
        </Button>
        <div className="flex min-w-0 items-center gap-2">
          <h1 className="truncate text-xl font-semibold tracking-tight">{project.name}</h1>
          <StatusBadge tone={project.status === 1 ? "success" : "warning"}>
            {project.status === 1 ? "就绪" : "禁用"}
          </StatusBadge>
        </div>
        <div className="ml-auto">
          <Button
            variant="outline"
            size="sm"
            title="刷新页面与来源"
            onClick={() => void loadChildren(false)}
            disabled={refreshing}
          >
            <RefreshCw className={refreshing ? "animate-spin" : ""} />
            刷新
          </Button>
        </div>
      </div>

      <Tabs value={activeTab} onValueChange={setActiveTab} className="mt-5 w-full">
        <TabsList className="w-full flex-wrap">
          {WIKI_TABS.map((t) => {
            const Icon = t.icon;
            return (
              <TabsTrigger key={t.id} value={t.id} disabled={t.disabled}>
                <Icon />
                {t.label}
              </TabsTrigger>
            );
          })}
        </TabsList>

        <TabsContent value="overview" className="mt-5">
          <OverviewTab
            project={project}
            pages={pages}
            sources={sources}
            onOpenPage={(p) => {
              setActiveTab("pages");
              setPreview(p);
            }}
          />
        </TabsContent>

        <TabsContent value="pages" className="mt-5">
          <PagesTab pages={pages} loading={listLoading} onOpenPage={setPreview} />
        </TabsContent>

        <TabsContent value="sources" className="mt-5">
          <SourcesTab
            projectId={project.id}
            sources={sources}
            loading={listLoading}
            onChanged={() => void loadChildren(false)}
          />
        </TabsContent>

        <TabsContent value="ask" className="mt-5">
          <SearchTab projectId={project.id} disabled={project.status !== 1} />
        </TabsContent>

        <TabsContent value="graph" className="mt-5">
          <WikiGraphPrototype projectId={project.id} />
        </TabsContent>

        <TabsContent value="settings" className="mt-5">
          <SettingsTab
            project={project}
            channels={channels}
            onSaved={() => void loadProject()}
            onDelete={() => setDeleteOpen(true)}
          />
        </TabsContent>
      </Tabs>

      {/* 页面预览：v1 无 Markdown 渲染依赖，按纯文本展示 */}
      <Dialog open={preview !== null} onOpenChange={(o) => !o && setPreview(null)}>
        <DialogContent className="sm:max-w-3xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              {preview?.title}
              {preview?.kind && (
                <span className={`rounded px-1.5 py-0.5 text-[11px] ${PAGE_KIND_STYLE[preview.kind]}`}>
                  {preview.kind}
                </span>
              )}
            </DialogTitle>
            <DialogDescription>
              {preview ? `${fmtTokens(preview.tokens)} tokens · 更新 ${fmtTime(preview.updated_at)}` : ""}
            </DialogDescription>
          </DialogHeader>
          <div className="max-h-[60vh] overflow-auto rounded-lg bg-muted p-4">
            <pre className="whitespace-pre-wrap font-mono text-xs leading-relaxed text-foreground">
              {preview?.content}
            </pre>
          </div>
          {preview && preview.links.length > 0 && (
            <div className="flex flex-wrap gap-1.5">
              {preview.links.map((l) => (
                <span key={l} className="rounded bg-primary/10 px-1.5 py-0.5 text-[11px] text-primary">
                  [[{l}]]
                </span>
              ))}
            </div>
          )}
        </DialogContent>
      </Dialog>

      {/* 删除项目确认 */}
      <Dialog open={deleteOpen} onOpenChange={setDeleteOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              删除 Wiki 项目
            </DialogTitle>
            <DialogDescription>
              确认删除「{project.name}」？其下全部页面与来源配置将一并移除，操作不可恢复。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteOpen(false)}>
              取消
            </Button>
            <Button variant="destructive" onClick={handleConfirmDelete} disabled={deleting}>
              {deleting ? "删除中..." : "确认删除"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
