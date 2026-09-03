import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate, useParams } from "react-router-dom";
import { open as openDialog } from "@tauri-apps/plugin-dialog";
import {
  ArrowLeft,
  FileText,
  Database,
  Search,
  MessageSquare,
  Layers,
  Settings,
  Boxes,
  UploadCloud,
  Trash2,
  RefreshCw,
  AlertTriangle,
  Loader2,
  GitBranch,
  Link2,
  FolderOpen,
  Plus,
  CheckCircle2,
  Sparkles,
  SlidersHorizontal,
  MessageCircle,
  ChevronRight,
  Send,
  User,
  Bot,
  MessageCircleQuestion,
  ListTree,
  BarChart3,
  Terminal,
} from "lucide-react";
import {
  Tabs,
  TabsContent,
  TabsList,
  TabsTrigger,
} from "@/components/ui/tabs";
import { Card, CardContent, CardHeader, CardTitle, CardAction } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select } from "@/components/ui/select";
import { Textarea } from "@/components/ui/textarea";
import { Switch } from "@/components/ui/switch";
import { Slider } from "@/components/ui/slider";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusBadge } from "@/components/ui/status-badge";
import { CopyButton } from "@/components/ui/copy-button";
import { useToast } from "@/components/ui/toast";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
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
import { knowledgeApi, channelApi, mcpApi } from "@/lib/api";
import type {
  AskResult,
  Channel,
  McpStatus,
  KbDocument,
  KbSource,
  ImportSourceInput,
  RetrievalHit,
  IndexStatus,
  KnowledgeBase,
  KnowledgeBaseUpdate,
} from "@/types";

/** 顶部水平 Tabs 定义（带 lucide 图标）。 */
const KB_TABS = [
  { id: "documents", label: "文档", icon: FileText },
  { id: "sources", label: "来源", icon: Database },
  { id: "retrieval", label: "检索", icon: Search },
  { id: "ask", label: "问答", icon: MessageSquare },
  { id: "index", label: "索引", icon: Layers },
  { id: "settings", label: "设置", icon: Settings },
  { id: "mcp", label: "MCP", icon: Boxes },
] as const;

function fmtTime(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}`;
}

function errMsg(e: unknown): string {
  if (e && typeof e === "object" && "message" in e) {
    return String((e as { message: unknown }).message);
  }
  return String(e);
}

// ---------------------------------------------------------------------------
// 文档 Tab：拖拽上传 + 文档列表
// ---------------------------------------------------------------------------

const ACCEPT =
  ".txt,.md,.markdown,.json,.yaml,.yml,.csv,.log,.ts,.tsx,.js,.jsx,.py,.rs,.go,.java,.kt,.c,.cpp,.h,.sh,.toml,.xml,.html,.css";

function DocumentsTab({ kb }: { kb: KnowledgeBase }) {
  const toast = useToast();
  const [documents, setDocuments] = useState<KbDocument[]>([]);
  const [loading, setLoading] = useState(true);
  const [uploading, setUploading] = useState<{ id: string; name: string }[]>([]);
  const [dragOver, setDragOver] = useState(false);
  const fileInputRef = useRef<HTMLInputElement>(null);
  const [docDeleteTarget, setDocDeleteTarget] = useState<KbDocument | null>(null);
  const [deletingDoc, setDeletingDoc] = useState(false);

  const refresh = async () => {
    setLoading(true);
    try {
      const docs = await knowledgeApi.documents(kb.id);
      setDocuments(docs);
    } catch (e) {
      console.error("加载文档失败：", e);
      setDocuments([]);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    refresh();
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [kb.id]);

  const handleFiles = async (files: FileList | null) => {
    if (!files || files.length === 0) return;
    for (const file of Array.from(files)) {
      const uid = crypto.randomUUID();
      setUploading((prev) => [...prev, { id: uid, name: file.name }]);
      try {
        const text = await file.text();
        const res = await knowledgeApi.ingest(kb.id, file.name, text);
        toast.success(`已摄入「${file.name}」，共 ${res.chunk_count} 个片段`);
        await refresh();
      } catch (e) {
        console.error("摄入失败：", e);
        toast.error(`摄入「${file.name}」失败：${errMsg(e) || "请重试"}`);
      } finally {
        setUploading((prev) => prev.filter((u) => u.id !== uid));
      }
    }
  };

  const onDrop = (e: React.DragEvent) => {
    e.preventDefault();
    setDragOver(false);
    handleFiles(e.dataTransfer.files);
  };

  const confirmDeleteDoc = async () => {
    if (!docDeleteTarget) return;
    const target = docDeleteTarget;
    setDeletingDoc(true);
    try {
      await knowledgeApi.removeDocument(target.id);
      toast.success(`已删除文档「${target.title}」`);
      setDocDeleteTarget(null);
      await refresh();
    } catch (e) {
      console.error("删除文档失败：", e);
      toast.error(`删除失败：${errMsg(e) || "请重试"}`);
    } finally {
      setDeletingDoc(false);
    }
  };

  return (
    <div className="space-y-4">
      {/* 拖拽上传区 */}
      <div
        role="button"
        tabIndex={0}
        onClick={() => fileInputRef.current?.click()}
        onKeyDown={(e) => {
          if (e.key === "Enter" || e.key === " ") fileInputRef.current?.click();
        }}
        onDragOver={(e) => {
          e.preventDefault();
          setDragOver(true);
        }}
        onDragLeave={() => setDragOver(false)}
        onDrop={onDrop}
        className={`flex cursor-pointer flex-col items-center justify-center rounded-xl border-2 border-dashed px-6 py-10 text-center transition-colors ${
          dragOver
            ? "border-primary bg-primary/5"
            : "border-muted-foreground/25 hover:border-primary/50"
        }`}
      >
        <UploadCloud
          className={`h-8 w-8 ${dragOver ? "text-primary" : "text-muted-foreground/60"}`}
        />
        <p className="mt-3 text-sm font-medium">
          拖拽文件到此处，或点击选择
        </p>
        <p className="mt-1 text-xs text-muted-foreground">
          支持 .md / .txt / .json / .yaml / 代码 / .pdf 等文本类文件
        </p>
        <input
          ref={fileInputRef}
          type="file"
          multiple
          accept={ACCEPT}
          className="hidden"
          onChange={(e) => {
            handleFiles(e.target.files);
            e.target.value = "";
          }}
        />
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

      {/* 文档列表 */}
      <div className="flex items-center justify-between">
        <h3 className="text-sm font-medium text-muted-foreground">文档</h3>
        <Button variant="outline" size="sm" onClick={refresh} disabled={loading}>
          <RefreshCw className={loading ? "animate-spin" : ""} />
          刷新
        </Button>
      </div>

      {loading ? (
        <div className="space-y-2">
          <Skeleton className="h-12 w-full" />
          <Skeleton className="h-12 w-full" />
        </div>
      ) : documents.length === 0 ? (
        <EmptyState
          icon={FileText}
          title="暂无文档"
          description="拖拽或选择文件即可摄入。文档会按长度自动分块并向量化，随后可在「问答」中检索引用。"
        />
      ) : (
        <div className="divide-y rounded-lg border">
          {documents.map((d) => (
            <div
              key={d.id}
              className="flex items-center justify-between gap-3 px-3 py-2"
            >
              <div className="min-w-0">
                <div className="flex items-center gap-2">
                  <span className="truncate font-medium">{d.title}</span>
                  <StatusBadge
                    tone={
                      d.status === 1
                        ? "success"
                        : d.status === 2
                          ? "destructive"
                          : "warning"
                    }
                  >
                    {d.status === 1 ? "就绪" : d.status === 2 ? "失败" : "处理中"}
                  </StatusBadge>
                </div>
                <p className="mt-0.5 text-[11px] text-muted-foreground">
                  {d.chunk_count} 片段 · {fmtTime(d.created_at)}
                  {d.error_message ? ` · ${d.error_message}` : ""}
                </p>
              </div>
              <Button
                variant="ghost"
                size="icon"
                title="删除文档"
                className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                onClick={() => setDocDeleteTarget(d)}
              >
                <Trash2 />
              </Button>
            </div>
          ))}
        </div>
      )}

      <Dialog
        open={docDeleteTarget !== null}
        onOpenChange={(o) => !o && setDocDeleteTarget(null)}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              删除文档
            </DialogTitle>
            <DialogDescription>
              确认删除文档「{docDeleteTarget?.title}」？其下全部向量分块将一并移除，操作不可恢复。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDocDeleteTarget(null)}>
              取消
            </Button>
            <Button
              variant="destructive"
              onClick={confirmDeleteDoc}
              disabled={deletingDoc}
            >
              {deletingDoc ? "删除中..." : "确认删除"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 问答 Tab
// ---------------------------------------------------------------------------

// ---------------------------------------------------------------------------
// 问答 Tab（deepseek 风格聊天 UI）
// ---------------------------------------------------------------------------

interface AskMessage {
  role: "user" | "assistant";
  content: string;
  sources?: AskResult["sources"];
}

interface RetrievalConfig {
  /** 检索模式：vector / keyword / hybrid */
  mode: "vector" | "keyword" | "hybrid";
  /** 召回数量 */
  topK: number;
  /** 向量权重（0-1，仅混合模式生效） */
  vectorWeight: number;
  /** 关键词权重（0-1，仅混合模式生效） */
  keywordWeight: number;
}

const DEFAULT_RETRIEVAL: RetrievalConfig = {
  mode: "vector",
  topK: 5,
  vectorWeight: 0.7,
  keywordWeight: 0.3,
};

const RETRIEVAL_MODES: Array<{ id: RetrievalConfig["mode"]; label: string; hint: string }> = [
  { id: "vector", label: "向量", hint: "仅向量召回（当前默认）" },
  { id: "keyword", label: "关键词", hint: "BM25 关键词召回" },
  { id: "hybrid", label: "混合", hint: "向量 + 关键词加权融合" },
];

function AskTab({ kb }: { kb: KnowledgeBase }) {
  const toast = useToast();
  const [channels, setChannels] = useState<Channel[]>([]);
  const [selectedChannelId, setSelectedChannelId] = useState<string>("");
  const [modelsForChannel, setModelsForChannel] = useState<string[]>([]);
  const [selectedModel, setSelectedModel] = useState<string>(kb.embedding_model || "");
  const [deepResearch, setDeepResearch] = useState(false);
  const [retrieval, setRetrieval] = useState<RetrievalConfig>(DEFAULT_RETRIEVAL);
  const [showRetrievalDialog, setShowRetrievalDialog] = useState(false);
  const [messages, setMessages] = useState<AskMessage[]>([]);
  const [query, setQuery] = useState("");
  const [asking, setAsking] = useState(false);
  const scrollRef = useRef<HTMLDivElement | null>(null);
  const textareaRef = useRef<HTMLTextAreaElement | null>(null);

  // 加载启用渠道；默认选第一个支持的模型对应渠道（让模型可填入）。
  useEffect(() => {
    channelApi
      .list()
      .then((chs) => {
        const enabled = chs.filter((c) => c.status === 1);
        setChannels(enabled);
        // 缺省：使用嵌入渠道同渠道（若存在）便于用户编辑；否则取第一个支持嵌入模型的渠道；都没有就置空。
        const embedCh = enabled.find((c) => c.id === kb.embedding_channel_id);
        const sorted = enabled.sort((a, b) => b.priority - a.priority);
        const fallback = embedCh || sorted[0] || null;
        if (fallback) {
          setSelectedChannelId(fallback.id);
          setModelsForChannel((fallback.models ?? []).slice().sort());
        }
        // 若没默认模型，从默认渠道的 models 中挑第一个
        const def = fallback?.models?.[0] || sorted.flatMap((c) => c.models ?? [])[0];
        if (def) setSelectedModel(def);
      })
      .catch(() => setChannels([]));
    // 故意忽略 kb 依赖：每次 kb 变都触发会冲掉用户选择
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // 渠道变化 → 重算可用模型 + 默认选第一项（若当前 model 不在新列表中）。
  useEffect(() => {
    if (!selectedChannelId) {
      setModelsForChannel([]);
      return;
    }
    const ch = channels.find((c) => c.id === selectedChannelId);
    const ms = (ch?.models ?? []).slice().sort();
    setModelsForChannel(ms);
    setSelectedModel((prev) => (ms.includes(prev) ? prev : ms[0] || ""));
  }, [selectedChannelId, channels]);

  // 发送 / 接收消息后滚到底部。
  useEffect(() => {
    if (scrollRef.current) {
      scrollRef.current.scrollTop = scrollRef.current.scrollHeight;
    }
  }, [messages, asking]);

  const send = async (text: string) => {
    const q = text.trim();
    if (!q || asking) return;
    if (!selectedModel.trim()) {
      toast.error("请先选择模型");
      return;
    }
    setAsking(true);
    setQuery("");
    setMessages((prev) => [...prev, { role: "user", content: q }]);
    try {
      const res = await knowledgeApi.ask(
        [kb.id],
        q,
        selectedModel.trim(),
        selectedChannelId || undefined,
        {
          mode: retrieval.mode,
          topK: retrieval.topK,
          keywordWeight: retrieval.keywordWeight,
        },
      );
      setMessages((prev) => [
        ...prev,
        { role: "assistant", content: res.answer, sources: res.sources },
      ]);
    } catch (e) {
      console.error("问答失败：", e);
      const msg = errMsg(e) || "请重试";
      toast.error(`问答失败：${msg}`);
      setMessages((prev) => [
        ...prev,
        {
          role: "assistant",
          content: `[错误] ${msg}。可调整模型 / 渠道后重试。`,
          sources: [],
        },
      ]);
    } finally {
      setAsking(false);
    }
  };

  const onKeyDownTextarea = (e: React.KeyboardEvent<HTMLTextAreaElement>) => {
    if (e.key === "Enter" && !e.shiftKey && !e.nativeEvent.isComposing) {
      e.preventDefault();
      send(query);
    }
  };

  return (
    <div className="-mx-4 flex h-[70vh] min-h-[520px] flex-col">
      {/* ===== 顶部栏：渠道 / 模型 / Deep Search / 检索配置 ===== */}
      <div className="flex flex-wrap items-center gap-2 border-b px-4 py-3">
        <div className="flex items-center gap-1.5">
          <span className="text-xs text-muted-foreground">渠道</span>
          <Select
            value={selectedChannelId}
            onChange={(e) => setSelectedChannelId(e.target.value)}
            className="h-8 w-auto min-w-[120px] text-sm"
            disabled={channels.length === 0}
          >
            {channels.length === 0 ? (
              <option value="">无可用渠道</option>
            ) : (
              channels.map((c) => (
                <option key={c.id} value={c.id}>
                  {c.name}
                </option>
              ))
            )}
          </Select>
        </div>
        <ChevronRight className="h-3.5 w-3.5 text-muted-foreground/60" />
        <div className="flex items-center gap-1.5">
          <span className="text-xs text-muted-foreground">模型</span>
          <Select
            value={selectedModel}
            onChange={(e) => setSelectedModel(e.target.value)}
            className="h-8 w-auto min-w-[160px] text-sm"
            disabled={modelsForChannel.length === 0}
          >
            {modelsForChannel.length === 0 ? (
              <option value="">该渠道未列出模型</option>
            ) : (
              modelsForChannel.map((m) => (
                <option key={m} value={m}>
                  {m}
                </option>
              ))
            )}
          </Select>
        </div>
        {selectedModel && (
          <span className="ml-1 rounded-md bg-primary/10 px-2 py-0.5 text-[11px] font-medium text-primary">
            {selectedModel}
          </span>
        )}
        <div className="ml-auto flex items-center gap-3">
          <label className="flex cursor-pointer items-center gap-1.5 text-xs">
            <Sparkles className="h-3.5 w-3.5 text-primary" />
            <span>Deep Search</span>
            <Switch
              checked={deepResearch}
              onCheckedChange={setDeepResearch}
              className="ml-1"
              title="Deep Search：在线扩展检索（UI 预留，后端待对接）"
            />
          </label>
          <Button
            variant="ghost"
            size="sm"
            onClick={() => setShowRetrievalDialog(true)}
            title="检索配置"
          >
            <SlidersHorizontal />
            检索配置
          </Button>
        </div>
      </div>

      {/* ===== 消息列表区 ===== */}
      <div
        ref={scrollRef}
        className="flex-1 space-y-4 overflow-y-auto bg-muted/20 px-4 py-6"
      >
        {messages.length === 0 ? (
          <div className="flex h-full flex-col items-center justify-center text-center">
            <div className="rounded-full bg-muted/60 p-4">
              <MessageCircle className="h-7 w-7 text-muted-foreground/70" />
            </div>
            <p className="mt-4 text-sm font-medium">
              向 RAG 提问，AI 将基于检索到的内容回答
            </p>
            <p className="mt-1 text-xs text-muted-foreground">
              {kb.doc_count} 文档 · {kb.chunk_count} 切片可供检索
            </p>
            <p className="mt-3 max-w-md text-[11px] text-muted-foreground/70">
              检索模式：{RETRIEVAL_MODES.find((m) => m.id === retrieval.mode)?.label} ·
              Top {retrieval.topK}
            </p>
          </div>
        ) : (
          messages.map((m, i) => (
            <MessageBubble key={i} message={m} />
          ))
        )}
        {asking && messages[messages.length - 1]?.role === "user" && (
          <div className="flex justify-start">
            <div className="flex max-w-[80%] items-start gap-2">
              <div className="flex h-7 w-7 shrink-0 items-center justify-center rounded-full bg-primary/15">
                <Bot className="h-4 w-4 text-primary" />
              </div>
              <div className="rounded-2xl rounded-tl-sm bg-card px-3.5 py-2 text-sm shadow-sm">
                <span className="inline-flex gap-0.5">
                  <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-muted-foreground/60 [animation-delay:0ms]" />
                  <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-muted-foreground/60 [animation-delay:120ms]" />
                  <span className="h-1.5 w-1.5 animate-bounce rounded-full bg-muted-foreground/60 [animation-delay:240ms]" />
                </span>
              </div>
            </div>
          </div>
        )}
      </div>

      {/* ===== 底部输入区 ===== */}
      <div className="border-t bg-background px-4 py-3">
        <div className="flex items-end gap-2 rounded-xl border bg-card p-1.5 shadow-sm">
          <Textarea
            ref={textareaRef}
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={onKeyDownTextarea}
            placeholder="输入问题，Enter 发送，Shift+Enter 换行..."
            rows={1}
            className="min-h-[40px] max-h-32 resize-none border-0 bg-transparent px-2 py-1.5 text-sm shadow-none focus-visible:ring-0"
          />
          <Button
            onClick={() => send(query)}
            disabled={asking || !query.trim() || !selectedModel.trim()}
            className="h-9 shrink-0 rounded-lg px-4"
          >
            <Send />
            {asking ? "发送中" : "发送"}
          </Button>
        </div>
        <p className="mt-1.5 px-1 text-[11px] text-muted-foreground">
          {asking
            ? "正在检索与生成回答..."
            : `已选 ${selectedModel || "—"} · 渠道${channels.find((c) => c.id === selectedChannelId)?.name || "自动"}`}
        </p>
      </div>

      {/* ===== 检索配置 Dialog ===== */}
      <Dialog open={showRetrievalDialog} onOpenChange={setShowRetrievalDialog}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <SlidersHorizontal className="h-4 w-4" />
              检索配置
            </DialogTitle>
            <DialogDescription>
              调整召回方式与权重；混合模式下向量与关键词分数会按权重融合。
              向量检索会先于关键词检索进行。
            </DialogDescription>
          </DialogHeader>

          <div className="space-y-5">
            {/* 检索模式：3 选 1 */}
            <div className="grid gap-2">
              <Label>检索模式</Label>
              <div className="inline-flex rounded-md border bg-muted/30 p-1">
                {RETRIEVAL_MODES.map((m) => {
                  const active = retrieval.mode === m.id;
                  return (
                    <button
                      key={m.id}
                      type="button"
                      title={m.hint}
                      onClick={() =>
                        setRetrieval((p) => ({ ...p, mode: m.id }))
                      }
                      className={cn(
                        "rounded-sm px-3 py-1 text-sm transition-colors",
                        active
                          ? "bg-background shadow-sm"
                          : "text-muted-foreground hover:text-foreground",
                      )}
                    >
                      {m.label}
                    </button>
                  );
                })}
              </div>
              <p className="text-[11px] text-muted-foreground">
                {RETRIEVAL_MODES.find((m) => m.id === retrieval.mode)?.hint}
              </p>
            </div>

            {/* Top K */}
            <div className="grid gap-2">
              <div className="flex items-center justify-between">
                <Label htmlFor="ask-topk">Top K</Label>
                <span className="font-mono text-xs text-muted-foreground">
                  {retrieval.topK}
                </span>
              </div>
              <Slider
                id="ask-topk"
                min={1}
                max={20}
                step={1}
                value={[retrieval.topK]}
                onValueChange={(v) =>
                  setRetrieval((p) => ({ ...p, topK: v[0] }))
                }
              />
            </div>

            {/* 向量权重 */}
            <div className="grid gap-2">
              <div className="flex items-center justify-between">
                <Label htmlFor="ask-vw">向量权重</Label>
                <span className="font-mono text-xs text-muted-foreground">
                  {retrieval.vectorWeight.toFixed(2)}
                </span>
              </div>
              <Slider
                id="ask-vw"
                min={0}
                max={1}
                step={0.05}
                value={[retrieval.vectorWeight]}
                disabled={retrieval.mode !== "hybrid"}
                onValueChange={(v) =>
                  setRetrieval((p) => ({ ...p, vectorWeight: v[0] }))
                }
              />
              {retrieval.mode === "hybrid" && (
                <p className="text-[11px] text-muted-foreground">
                  关键词权重会按 1 - 向量权重 自动联动（{retrieval.keywordWeight.toFixed(2)}）。
                </p>
              )}
            </div>

            {/* 关键词权重 */}
            <div className="grid gap-2">
              <div className="flex items-center justify-between">
                <Label htmlFor="ask-kw">关键词权重</Label>
                <span className="font-mono text-xs text-muted-foreground">
                  {retrieval.keywordWeight.toFixed(2)}
                </span>
              </div>
              <Slider
                id="ask-kw"
                min={0}
                max={1}
                step={0.05}
                value={[retrieval.keywordWeight]}
                disabled={retrieval.mode !== "hybrid"}
                onValueChange={(v) => {
                  const kw = v[0];
                  setRetrieval((p) => ({
                    ...p,
                    keywordWeight: kw,
                    vectorWeight: Number((1 - kw).toFixed(2)),
                  }));
                }}
              />
            </div>

            <p className="rounded-md bg-muted/40 px-3 py-2 text-[11px] leading-relaxed text-muted-foreground">
              向量 / 关键词（BM25）/ 混合检索均已由后端支持，模式与权重实时生效；
              混合模式按向量权重 = 1 − 关键词权重做归一化加权融合。
            </p>
          </div>

          <DialogFooter>
            <Button
              variant="ghost"
              onClick={() => setRetrieval(DEFAULT_RETRIEVAL)}
            >
              恢复默认
            </Button>
            <Button onClick={() => setShowRetrievalDialog(false)}>完成</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

function MessageBubble({ message }: { message: AskMessage }) {
  const isUser = message.role === "user";
  return (
    <div className={cn("flex w-full", isUser ? "justify-end" : "justify-start")}>
      <div
        className={cn(
          "flex max-w-[80%] items-start gap-2",
          isUser && "flex-row-reverse",
        )}
      >
        <div
          className={cn(
            "flex h-7 w-7 shrink-0 items-center justify-center rounded-full",
            isUser ? "bg-primary text-primary-foreground" : "bg-primary/15",
          )}
        >
          {isUser ? (
            <User className="h-4 w-4" />
          ) : (
            <Bot className="h-4 w-4 text-primary" />
          )}
        </div>
        <div
          className={cn(
            "rounded-2xl px-3.5 py-2 text-sm shadow-sm",
            isUser
              ? "rounded-tr-sm bg-primary text-primary-foreground"
              : "rounded-tl-sm bg-card",
          )}
        >
          <p className="whitespace-pre-wrap break-words leading-relaxed">
            {message.content}
          </p>
          {message.sources && message.sources.length > 0 && (
            <div className="mt-2 space-y-1.5 border-t pt-2 text-xs">
              <p className="font-medium text-muted-foreground">
                引用来源（{message.sources.length}）
              </p>
              <ul className="space-y-1.5">
                {message.sources.map((s, i) => (
                  <li
                    key={i}
                    className="rounded-md bg-muted/40 px-2 py-1.5"
                  >
                    <div className="flex items-center justify-between text-[11px]">
                      <span className="truncate font-medium">{s.doc_title}</span>
                      <span className="font-mono text-muted-foreground">
                        {s.score.toFixed(3)}
                      </span>
                    </div>
                    <p className="mt-0.5 line-clamp-2 text-[11px] text-muted-foreground">
                      {s.content}
                    </p>
                  </li>
                ))}
              </ul>
            </div>
          )}
        </div>
      </div>
    </div>
  );
}


// ---------------------------------------------------------------------------
// 设置 Tab
// ---------------------------------------------------------------------------

function SettingsTab({ kb, onSaved }: { kb: KnowledgeBase; onSaved: (next: KnowledgeBase) => void }) {
  const toast = useToast();
  const [channels, setChannels] = useState<Record<string, string>>({});
  const [saving, setSaving] = useState(false);

  const [name, setName] = useState(kb.name);
  const [description, setDescription] = useState(kb.description);
  const [enabled, setEnabled] = useState(kb.status === 1);
  const [mcpExposed, setMcpExposed] = useState(kb.mcp_exposed === 1);
  const [batchSize, setBatchSize] = useState(
    kb.embedding_batch_size != null ? String(kb.embedding_batch_size) : "",
  );
  const [excludeDirs, setExcludeDirs] = useState(kb.exclude_dirs ?? "");
  const [excludeFiles, setExcludeFiles] = useState(kb.exclude_files ?? "");
  const [includeTypes, setIncludeTypes] = useState(kb.include_file_types ?? "");

  useEffect(() => {
    channelApi
      .list()
      .then((chs) =>
        setChannels(Object.fromEntries(chs.map((c) => [c.id, c.name]))),
      )
      .catch(() => setChannels({}));
  }, []);

  const handleSave = async () => {
    if (!name.trim()) {
      toast.error("知识库名称不能为空");
      return;
    }
    setSaving(true);
    const patch: KnowledgeBaseUpdate = {
      name: name.trim(),
      description: description.trim(),
      status: enabled ? 1 : 0,
      mcp_exposed: mcpExposed ? 1 : 0,
      embedding_batch_size:
        batchSize.trim() === "" ? null : Number(batchSize),
      exclude_dirs: excludeDirs.trim() || null,
      exclude_files: excludeFiles.trim() || null,
      include_file_types: includeTypes.trim() || null,
    };
    try {
      const updated = await knowledgeApi.update(kb.id, patch);
      onSaved(updated);
      toast.success("设置已保存");
    } catch (e) {
      console.error("保存设置失败：", e);
      toast.error(`保存失败：${errMsg(e) || "请重试"}`);
    } finally {
      setSaving(false);
    }
  };

  return (
    <div className="grid gap-4 lg:grid-cols-2">
      {/* 基本信息 */}
      <Card>
        <CardHeader>
          <CardTitle className="text-sm">基本信息</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-2">
            <Label htmlFor="set-name">名称</Label>
            <Input
              id="set-name"
              value={name}
              onChange={(e) => setName(e.target.value)}
            />
          </div>
          <div className="grid gap-2">
            <Label htmlFor="set-desc">描述</Label>
            <Textarea
              id="set-desc"
              rows={3}
              value={description}
              onChange={(e) => setDescription(e.target.value)}
            />
          </div>
          <div className="flex items-center justify-between rounded-lg border px-3 py-2">
            <div>
              <p className="text-sm font-medium">启用 RAG</p>
              <p className="text-xs text-muted-foreground">
                停用后该知识库不参与问答检索
              </p>
            </div>
            <Switch checked={enabled} onCheckedChange={setEnabled} />
          </div>
          <div className="flex items-center justify-between rounded-lg border px-3 py-2">
            <div>
              <p className="text-sm font-medium">MCP 暴露</p>
              <p className="text-xs text-muted-foreground">
                将本知识库暴露给 MCP 层供外部工具调用
              </p>
            </div>
            <Switch checked={mcpExposed} onCheckedChange={setMcpExposed} />
          </div>
        </CardContent>
      </Card>

      {/* Embedding 配置 */}
      <Card>
        <CardHeader>
          <CardTitle className="text-sm">Embedding 配置</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-2">
            <Label>绑定渠道</Label>
            <p className="text-sm text-muted-foreground">
              {channels[kb.embedding_channel_id] ?? kb.embedding_channel_id ?? "—"}
            </p>
          </div>
          <div className="grid gap-2">
            <Label>嵌入模型</Label>
            <p className="font-mono text-sm text-muted-foreground">
              {kb.embedding_model || "—"}
            </p>
          </div>
          <div className="grid gap-2">
            <Label htmlFor="set-batch">Embedding 批次</Label>
            <Input
              id="set-batch"
              type="number"
              min={1}
              placeholder="留空使用引擎默认（如 16）"
              value={batchSize}
              onChange={(e) => setBatchSize(e.target.value)}
            />
          </div>
        </CardContent>
      </Card>

      {/* 分块与过滤 */}
      <Card className="lg:col-span-2">
        <CardHeader>
          <CardTitle className="text-sm">分块与过滤</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          <div className="grid gap-4 md:grid-cols-3">
            <div className="grid gap-2">
              <Label htmlFor="set-exdir">排除目录</Label>
              <Input
                id="set-exdir"
                placeholder="逗号分隔，如 node_modules,dist"
                value={excludeDirs}
                onChange={(e) => setExcludeDirs(e.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="set-exfile">排除文件</Label>
              <Input
                id="set-exfile"
                placeholder="逗号分隔，如 secrets.env"
                value={excludeFiles}
                onChange={(e) => setExcludeFiles(e.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="set-intype">包含文件类型</Label>
              <Input
                id="set-intype"
                placeholder="逗号分隔，如 .md,.txt"
                value={includeTypes}
                onChange={(e) => setIncludeTypes(e.target.value)}
              />
            </div>
          </div>
          <p className="text-xs text-muted-foreground">
            分块大小 / 重叠比例将在后续版本支持，当前由引擎按文档长度自动分块。
          </p>
        </CardContent>
      </Card>

      {/* 统计 */}
      <Card className="lg:col-span-2">
        <CardHeader>
          <CardTitle className="text-sm">统计</CardTitle>
        </CardHeader>
        <CardContent>
          <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
            <div className="rounded-lg bg-muted/40 px-3 py-2">
              <p className="text-xs text-muted-foreground">文档数</p>
              <p className="text-lg font-semibold">{kb.doc_count}</p>
            </div>
            <div className="rounded-lg bg-muted/40 px-3 py-2">
              <p className="text-xs text-muted-foreground">片段数</p>
              <p className="text-lg font-semibold">{kb.chunk_count}</p>
            </div>
            <div className="rounded-lg bg-muted/40 px-3 py-2">
              <p className="text-xs text-muted-foreground">创建于</p>
              <p className="text-sm font-medium">{fmtTime(kb.created_at)}</p>
            </div>
            <div className="rounded-lg bg-muted/40 px-3 py-2">
              <p className="text-xs text-muted-foreground">更新于</p>
              <p className="text-sm font-medium">{fmtTime(kb.updated_at)}</p>
            </div>
          </div>
        </CardContent>
      </Card>

      <div className="lg:col-span-2 flex justify-end">
        <Button onClick={handleSave} disabled={saving}>
          {saving ? "保存中..." : "保存设置"}
        </Button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------
// 检索 Tab：检索调试（实时查看 Top-K 命中与相似度）
// ---------------------------------------------------------------------------

function RetrievalTab({ kb }: { kb: KnowledgeBase }) {
  const toast = useToast();
  const [query, setQuery] = useState("");
  const [topK, setTopK] = useState("5");
  const [hits, setHits] = useState<RetrievalHit[]>([]);
  const [searched, setSearched] = useState(false);
  const [loading, setLoading] = useState(false);

  const handleSearch = async () => {
    if (!query.trim()) {
      toast.error("请输入查询内容");
      return;
    }
    setLoading(true);
    try {
      const res = await knowledgeApi.retrieve(
        kb.id,
        query.trim(),
        topK.trim() === "" ? undefined : Number(topK),
      );
      setHits(res);
      setSearched(true);
    } catch (e) {
      console.error("检索失败：", e);
      toast.error(`检索失败：${errMsg(e) || "请重试"}`);
    } finally {
      setLoading(false);
    }
  };

  return (
    <div className="space-y-4">
      {/* 同一行：查询框 + 返回条数 + 检索按钮（带 Search 图标） */}
      <div className="flex flex-col gap-2 md:flex-row md:items-end">
        <div className="grid grow gap-2">
          <Label htmlFor="ret-query">查询内容</Label>
          <Input
            id="ret-query"
            placeholder="例如：如何配置鉴权中间件？"
            value={query}
            onChange={(e) => setQuery(e.target.value)}
            onKeyDown={(e) => {
              if (e.key === "Enter") handleSearch();
            }}
          />
        </div>
        <div className="grid gap-2 md:w-24">
          <Label htmlFor="ret-k">Top K</Label>
          <Input
            id="ret-k"
            type="number"
            min={1}
            max={20}
            value={topK}
            onChange={(e) => setTopK(e.target.value)}
          />
        </div>
        <Button onClick={handleSearch} disabled={loading} className="md:self-end">
          <Search />
          {loading ? "检索中..." : "检索"}
        </Button>
      </div>

      {!searched ? (
        <EmptyState
          icon={Search}
          title="检索调试"
          description="输入查询词，实时查看该知识库检索到的 Top-K 分块及其相似度，用于验证摄入与向量化效果。"
        />
      ) : hits.length === 0 ? (
        <EmptyState
          icon={Search}
          title="无命中"
          description="没有检索到相关分块。可尝试更换查询词，或确认文档已成功摄入并向量化。"
        />
      ) : (
        <div className="space-y-2">
          <p className="text-xs text-muted-foreground">
            命中 {hits.length} 个分块（按相似度降序）
          </p>
          {hits.map((h, i) => {
            const pct = Math.max(0, Math.min(100, Math.round(h.score * 100)));
            return (
              <div key={`${h.doc_id}-${i}`} className="rounded-lg border p-3">
                <div className="flex items-center justify-between gap-2">
                  <span className="truncate text-sm font-medium">
                    {h.doc_title || "（无标题）"}
                  </span>
                  <span className="shrink-0 font-mono text-xs text-muted-foreground">
                    相似度 {h.score.toFixed(3)}
                  </span>
                </div>
                <div className="mt-1.5 h-1 w-full overflow-hidden rounded-full bg-muted">
                  <div
                    className="h-full rounded-full bg-primary"
                    style={{ width: `${pct}%` }}
                  />
                </div>
                <p className="mt-2 line-clamp-4 whitespace-pre-wrap text-xs text-muted-foreground">
                  {h.content}
                </p>
              </div>
            );
          })}
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------
// 索引 Tab：索引状态查看 + 重建索引（按当前嵌入模型重嵌全部分块）
// ---------------------------------------------------------------------------

function IndexTab({ kb }: { kb: KnowledgeBase }) {
  const toast = useToast();
  const [status, setStatus] = useState<IndexStatus | null>(null);
  const [loading, setLoading] = useState(false);
  const [reindexing, setReindexing] = useState(false);
  const [confirmOpen, setConfirmOpen] = useState(false);

  const refresh = useCallback(async () => {
    setLoading(true);
    try {
      const s = await knowledgeApi.indexStatus(kb.id);
      setStatus(s);
    } catch (e) {
      console.error("获取索引状态失败：", e);
      toast.error(`获取索引状态失败：${errMsg(e) || "请重试"}`);
    } finally {
      setLoading(false);
    }
  }, [kb.id, toast]);

  useEffect(() => {
    void refresh();
  }, [refresh]);

  const handleReindex = async () => {
    setConfirmOpen(false);
    setReindexing(true);
    try {
      const s = await knowledgeApi.reindex(kb.id);
      setStatus(s);
      toast.success("索引已重建：全部分块已按当前嵌入模型重新向量化");
    } catch (e) {
      console.error("重建索引失败：", e);
      toast.error(`重建索引失败：${errMsg(e) || "请重试"}`);
    } finally {
      setReindexing(false);
    }
  };

  if (status && status.doc_count === 0) {
    return (
      <EmptyState
        icon={Layers}
        title="暂无索引内容"
        description="该知识库还没有任何文档。请先到「文档」或「来源」导入内容，向量化完成后即可在此查看索引状态。"
      />
    );
  }

  // 健康度：完整且非 stale → 正常；有 stale → 需重建；否则部分缺失
  const tone: "success" | "warning" | "destructive" | "secondary" = status
    ? status.is_stale
      ? "warning"
      : status.is_complete
        ? "success"
        : "destructive"
    : "secondary";

  const healthLabel = status
    ? status.is_stale
      ? "需重建"
      : status.is_complete
        ? "完整"
        : "部分缺失"
    : "—";

  const stats: { label: string; value: number | string }[] = status
    ? [
        { label: "文档数", value: status.doc_count },
        { label: "分块数", value: status.chunk_count },
        { label: "已向量化", value: status.embedded_count },
        { label: "待重建(stale)", value: status.stale_count },
      ]
    : [];

  return (
    <div className="space-y-4">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div className="flex items-center gap-2">
          <span className="text-sm text-muted-foreground">索引健康度</span>
          {status ? (
            <StatusBadge tone={tone}>
              {tone === "success" ? (
                <CheckCircle2 className="h-3.5 w-3.5" />
              ) : (
                <AlertTriangle className="h-3.5 w-3.5" />
              )}
              {healthLabel}
            </StatusBadge>
          ) : (
            <Loader2 className="h-4 w-4 animate-spin text-muted-foreground" />
          )}
        </div>
        <div className="flex items-center gap-2">
          <Button variant="outline" size="sm" onClick={refresh} disabled={loading || reindexing}>
            {loading ? "刷新中..." : "刷新"}
          </Button>
          <Button
            size="sm"
            onClick={() => setConfirmOpen(true)}
            disabled={reindexing || (status?.chunk_count ?? 0) === 0}
          >
            <RefreshCw className={reindexing ? "h-4 w-4 animate-spin" : "h-4 w-4"} />
            {reindexing ? "重建中..." : "重建索引"}
          </Button>
        </div>
      </div>

      <Card>
        <CardHeader>
          <CardTitle className="text-sm">当前嵌入模型</CardTitle>
        </CardHeader>
        <CardContent>
          <p className="font-mono text-sm">{status?.embedding_model ?? "—"}</p>
          <p className="mt-1 text-xs text-muted-foreground">
            重建索引会用该模型重新向量化全部分块。切换嵌入模型后，旧分块会被标记为「待重建」，
            检索前建议重建一次以保证相似度口径一致。
          </p>
        </CardContent>
      </Card>

      <div className="grid grid-cols-2 gap-3 sm:grid-cols-4">
        {stats.map((s) => (
          <Card key={s.label}>
            <CardContent className="pt-4">
              <p className="text-2xl font-semibold tabular-nums">{s.value}</p>
              <p className="mt-1 text-xs text-muted-foreground">{s.label}</p>
            </CardContent>
          </Card>
        ))}
      </div>

      <Dialog open={confirmOpen} onOpenChange={setConfirmOpen}>
        <DialogContent>
          <DialogHeader>
            <DialogTitle>重建索引</DialogTitle>
            <DialogDescription>
              将按当前嵌入模型（{status?.embedding_model ?? "—"}）重新向量化全部{" "}
              {status?.chunk_count ?? 0} 个分块并写回。耗时取决于分块数量与嵌入接口速度，
              过程会在后台进行，请耐心等待。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setConfirmOpen(false)} disabled={reindexing}>
              取消
            </Button>
            <Button onClick={handleReindex} disabled={reindexing}>
              {reindexing ? "重建中..." : "开始重建"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

// ---------------------------------------------------------------------------
// MCP Tab：MCP 端点 + 可用工具展示页（纯展示，无可配置项）
//
// 端点来自动态运行态（与网关同源）；工具为静态元数据。
// 「已暴露」取自 kb.mcp_exposed。
//
// 注：MCP 路由通过 services::mcp 注册进网关，端点 = 网关地址 + /mcp
// （默认 http://127.0.0.1:9842/mcp）。改网关端口 → 重启网关 → 端点自动跟随，
// 因此端点一律以后端返回的运行态值为准，硬编码值只作首屏兜底。
// ---------------------------------------------------------------------------

/** MCP 工具元数据：name → 用途 + 必填参数。 */
const MCP_TOOLS: Array<{
  name: string;
  description: string;
  required: string[];
  icon: React.ComponentType<{ className?: string }>;
}> = [
  {
    name: "search_knowledge_base",
    description: "语义检索 RAG，返回匹配文本片段和相似度评分",
    required: ["query"],
    icon: Search,
  },
  {
    name: "list_knowledge_bases",
    description: "列出所有已暴露的 RAG（ID/名称/文档数）",
    required: [],
    icon: ListTree,
  },
  {
    name: "ask_knowledge_base",
    description: "RAG 问答，基于检索内容生成回答并返回来源引用",
    required: ["question"],
    icon: MessageCircleQuestion,
  },
  {
    name: "read_document",
    description: "读取指定文档的完整内容",
    required: ["kb_id", "doc_id"],
    icon: FileText,
  },
  {
    name: "get_knowledge_base_stats",
    description: "获取 RAG 统计信息（文档数 / 切片数 / token 数）",
    required: ["kb_id"],
    icon: BarChart3,
  },
];

/**
 * MCP 对接展示页：MCP 端点 + 可用工具列表（纯展示，无配置项）。
 *
 * - 「已暴露」取自 kb.mcp_exposed（每 KB 独立开关）
 * - 服务运行态（监听地址、是否在线）来自后端 `get_mcp_status`，每 3s 轮询
 */
function McpTab({ kb }: { kb: KnowledgeBase }) {
  const isExposed = kb.mcp_exposed === 1;
  const [status, setStatus] = useState<McpStatus | null>(null);
  useEffect(() => {
    let cancelled = false;
    const tick = () => {
      mcpApi
        .status()
        .then((s) => {
          if (!cancelled) setStatus(s);
        })
        .catch(() => {
          // 后端命令缺失/未注册时静默吞掉——MCP server 暂时不在也是一种状态。
        });
    };
    tick();
    const id = setInterval(tick, 3000);
    return () => {
      cancelled = true;
      clearInterval(id);
    };
  }, []);

  // 端点优先用后端运行态（MCP 与网关同源，端口随网关设置变化）；
  // 未拿到时回退到网关默认端口 9842。
  const endpoint = status?.endpoint ?? "http://127.0.0.1:9842/mcp";
  const clientSnippet = JSON.stringify(
    {
      mcpServers: {
        dongx: {
          type: "http",
          url: endpoint,
        },
      },
    },
    null,
    2,
  );

  const toolsCount = status?.toolsCount ?? MCP_TOOLS.length;
  const serverUp = status?.running === true;

  return (
    <div className="space-y-5">
      {/* MCP 端点 */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Terminal className="h-4 w-4 text-primary" />
            MCP 对接
          </CardTitle>
          <CardAction className="flex items-center gap-2">
            <StatusBadge tone={serverUp ? "success" : "secondary"}>
              {serverUp ? "服务运行中" : "服务未运行"}
            </StatusBadge>
            <StatusBadge tone={isExposed ? "success" : "secondary"}>
              {isExposed ? "已暴露" : "未暴露"}
            </StatusBadge>
          </CardAction>
        </CardHeader>
        <CardContent className="space-y-3">
          <div className="grid gap-2">
            <Label htmlFor="mcp-endpoint">MCP 端点（JSON-RPC over HTTP）</Label>
            <div className="flex items-stretch gap-2">
              <Input
                id="mcp-endpoint"
                readOnly
                value={endpoint}
                onFocus={(e) => e.currentTarget.select()}
                className="font-mono text-xs"
              />
              <CopyButton value={endpoint} label="MCP 端点" title="复制端点" />
            </div>
            <p className="text-[11px] text-muted-foreground">
              端点挂载在网关下，与网关共用端口；修改网关端口并重启后，此处自动更新。
            </p>
          </div>
          <div className="rounded-lg border border-primary/30 bg-primary/5 p-3 text-xs leading-relaxed">
            <p className="flex items-center gap-1.5 font-medium text-primary">
              <Sparkles className="h-3.5 w-3.5" />
              MCP（Model Context Protocol）对接
            </p>
            <p className="mt-1.5 text-foreground/80">
              其他 AI Agent / 工具可通过 MCP 协议接入此 RAG。
              将上方端点配置到支持 MCP 的客户端（如 Claude Desktop、Cursor、自定义 Agent），
              即可让 AI 自动检索和问答你的私有 RAG。
            </p>
          </div>
          <details className="rounded-lg border bg-muted/30 px-3 py-2 text-xs">
            <summary className="cursor-pointer select-none font-medium text-muted-foreground">
              客户端配置片段（MCP client config）
            </summary>
            <div className="mt-2 space-y-2">
              <CodeBlock code={clientSnippet} lang="json" />
              <CopyButton
                value={clientSnippet}
                label="客户端配置"
                variant="button"
                title="复制客户端配置"
              />
            </div>
          </details>
        </CardContent>
      </Card>

      {/* 可用 MCP 工具 */}
      <Card>
        <CardHeader>
          <CardTitle className="flex items-center gap-2 text-base">
            <Boxes className="h-4 w-4 text-primary" />
            可用 MCP 工具
          </CardTitle>
          <CardAction>
            <span className="text-xs text-muted-foreground">
              {toolsCount} 个工具
            </span>
          </CardAction>
        </CardHeader>
        <CardContent>
          <Table>
            <TableHeader>
              <TableRow>
                {/* 工具名 / 必填列用 w-[1%] + nowrap 压到最小宽度，
                    剩余空间全部给说明列——比 table-fixed 更耐窄屏。 */}
                <TableHead className="w-[1%]">工具</TableHead>
                <TableHead>说明</TableHead>
                <TableHead className="w-[1%]">必填参数</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {MCP_TOOLS.map((tool) => {
                const Icon = tool.icon;
                return (
                  <TableRow key={tool.name}>
                    <TableCell className="align-top">
                      <code className="inline-flex items-center rounded-md border bg-muted/60 px-1.5 py-0.5 font-mono text-[11px]">
                        {tool.name}
                      </code>
                    </TableCell>
                    <TableCell className="whitespace-normal align-top">
                      <span className="flex items-start gap-1.5 leading-relaxed">
                        <Icon className="mt-0.5 h-3.5 w-3.5 shrink-0 text-muted-foreground" />
                        <span>{tool.description}</span>
                      </span>
                    </TableCell>
                    <TableCell className="align-top">
                      {tool.required.length > 0 ? (
                        <div className="flex flex-wrap gap-1">
                          {tool.required.map((k) => (
                            <code
                              key={k}
                              className="rounded bg-muted/60 px-1 font-mono text-[10px]"
                            >
                              {k}
                            </code>
                          ))}
                        </div>
                      ) : (
                        <span className="text-xs text-muted-foreground">无</span>
                      )}
                    </TableCell>
                  </TableRow>
                );
              })}
            </TableBody>
          </Table>
        </CardContent>
      </Card>

    </div>
  );
}

// ---------------------------------------------------------------------------
// 来源 Tab：Git / URL / 本地目录 导入 + 来源列表
// ---------------------------------------------------------------------------

const SOURCE_TYPE_LABEL: Record<string, string> = {
  git: "Git 仓库",
  url: "单个 URL",
  local_dir: "本地目录",
};

function sourceRefLabel(s: KbSource): string {
  if (s.source_type === "git") return s.repo_url ?? "—";
  if (s.source_type === "url") return s.url ?? "—";
  return s.dir_path ?? "—";
}

/** 来源状态徽标：导入中带旋转图标，完成/失败定色。 */
function SourceStatusBadge({ s }: { s: KbSource }) {
  if (s.status === "fetching") {
    return (
      <StatusBadge tone="warning">
        <Loader2 className="h-3 w-3 animate-spin" />
        导入中
      </StatusBadge>
    );
  }
  if (s.status === "done") {
    return <StatusBadge tone="success">完成</StatusBadge>;
  }
  return <StatusBadge tone="destructive">失败</StatusBadge>;
}

/** 导入来源对话框：Git / URL / 本地目录 三选一 + 共享过滤选项。 */
function ImportSourceDialog({
  kbId,
  open,
  onOpenChange,
  onImported,
}: {
  kbId: string;
  open: boolean;
  onOpenChange: (o: boolean) => void;
  onImported: () => void;
}) {
  const toast = useToast();
  const [tab, setTab] = useState<"git" | "url" | "local_dir">("git");
  const [repoUrl, setRepoUrl] = useState("");
  const [branch, setBranch] = useState("");
  const [token, setToken] = useState("");
  const [url, setUrl] = useState("");
  const [dirPath, setDirPath] = useState("");
  const [subpath, setSubpath] = useState("");
  const [excludedDirs, setExcludedDirs] = useState("");
  const [includedFiles, setIncludedFiles] = useState("");
  const [maxMb, setMaxMb] = useState("");
  const [submitting, setSubmitting] = useState(false);

  const pickDir = async () => {
    try {
      const picked = await openDialog({ directory: true, multiple: false });
      if (typeof picked === "string") setDirPath(picked);
    } catch {
      /* 用户取消或非桌面环境：忽略 */
    }
  };

  const reset = () => {
    setTab("git");
    setRepoUrl("");
    setBranch("");
    setToken("");
    setUrl("");
    setDirPath("");
    setSubpath("");
    setExcludedDirs("");
    setIncludedFiles("");
    setMaxMb("");
  };

  const handleSubmit = async () => {
    if (tab === "git" && !repoUrl.trim()) {
      toast.error("请填写仓库地址");
      return;
    }
    if (tab === "url" && !url.trim()) {
      toast.error("请填写 URL");
      return;
    }
    if (tab === "local_dir" && !dirPath.trim()) {
      toast.error("请选择本地目录");
      return;
    }
    setSubmitting(true);
    const input: ImportSourceInput = {
      source_type: tab,
      repo_url: tab === "git" ? repoUrl.trim() : undefined,
      branch: tab === "git" && branch.trim() ? branch.trim() : undefined,
      token: tab === "git" && token.trim() ? token.trim() : undefined,
      url: tab === "url" ? url.trim() : undefined,
      dir_path: tab === "local_dir" ? dirPath.trim() : undefined,
      subpath: subpath.trim() || undefined,
      excluded_dirs: excludedDirs.trim() || undefined,
      included_files: includedFiles.trim() || undefined,
      max_file_size_mb: maxMb.trim() === "" ? undefined : Number(maxMb),
    };
    try {
      await knowledgeApi.importSource(kbId, input);
      toast.success("已提交导入任务，可在下方查看进度");
      reset();
      onOpenChange(false);
      onImported();
    } catch (e) {
      console.error("导入来源失败：", e);
      toast.error(`导入失败：${errMsg(e) || "请重试"}`);
    } finally {
      setSubmitting(false);
    }
  };

  return (
    <Dialog open={open} onOpenChange={onOpenChange}>
      <DialogContent className="sm:max-w-lg">
        <DialogHeader>
          <DialogTitle>导入来源</DialogTitle>
          <DialogDescription>
            从 Git 仓库 / 单个网页 / 本地目录批量摄入文本文件。导入在后台执行，可关闭本窗口后于列表查看进度。
          </DialogDescription>
        </DialogHeader>

        <Tabs value={tab} onValueChange={(v) => setTab(v as typeof tab)}>
          <TabsList className="grid w-full grid-cols-3">
            <TabsTrigger value="git">
              <GitBranch />
              Git
            </TabsTrigger>
            <TabsTrigger value="url">
              <Link2 />
              URL
            </TabsTrigger>
            <TabsTrigger value="local_dir">
              <FolderOpen />
              本地目录
            </TabsTrigger>
          </TabsList>

          <TabsContent value="git" className="space-y-3">
            <div className="grid gap-2">
              <Label htmlFor="imp-repo">仓库地址</Label>
              <Input
                id="imp-repo"
                placeholder="https://github.com/owner/repo"
                value={repoUrl}
                onChange={(e) => setRepoUrl(e.target.value)}
                className="font-mono"
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="imp-branch">分支（可选）</Label>
              <Input
                id="imp-branch"
                placeholder="main"
                value={branch}
                onChange={(e) => setBranch(e.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="imp-token">Access Token（可选）</Label>
              <Input
                id="imp-token"
                type="password"
                placeholder="私有仓库填写，将注入 clone URL 完成鉴权"
                value={token}
                onChange={(e) => setToken(e.target.value)}
                className="font-mono"
              />
            </div>
          </TabsContent>

          <TabsContent value="url" className="space-y-3">
            <div className="grid gap-2">
              <Label htmlFor="imp-url">链接</Label>
              <Input
                id="imp-url"
                placeholder="https://example.com/doc.md"
                value={url}
                onChange={(e) => setUrl(e.target.value)}
                className="font-mono"
              />
              <p className="text-xs text-muted-foreground">
                仅摄入单页文本；HTML 会自动剥离标签。
              </p>
            </div>
          </TabsContent>

          <TabsContent value="local_dir" className="space-y-3">
            <div className="grid gap-2">
              <Label htmlFor="imp-dir">目录路径</Label>
              <div className="flex gap-2">
                <Input
                  id="imp-dir"
                  readOnly
                  value={dirPath}
                  placeholder="选择本地目录"
                  className="font-mono"
                />
                <Button type="button" variant="outline" onClick={pickDir}>
                  <FolderOpen />
                  选择
                </Button>
              </div>
            </div>
          </TabsContent>
        </Tabs>

        {/* 共享过滤选项：三种导入方式共用 */}
        <div className="space-y-3 rounded-lg border p-3">
          <p className="text-xs font-medium text-muted-foreground">
            过滤选项（三种方式共用）
          </p>
          <div className="grid gap-3 md:grid-cols-2">
            <div className="grid gap-2">
              <Label htmlFor="imp-sub">子路径（可选）</Label>
              <Input
                id="imp-sub"
                placeholder="仅扫描 repo/subdir"
                value={subpath}
                onChange={(e) => setSubpath(e.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="imp-max">最大文件大小（MB）</Label>
              <Input
                id="imp-max"
                type="number"
                min={1}
                placeholder="默认 1"
                value={maxMb}
                onChange={(e) => setMaxMb(e.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="imp-exdir">排除目录</Label>
              <Input
                id="imp-exdir"
                placeholder="逗号分隔，如 node_modules,dist"
                value={excludedDirs}
                onChange={(e) => setExcludedDirs(e.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="imp-exfile">包含文件类型</Label>
              <Input
                id="imp-exfile"
                placeholder="逗号分隔，如 .md,.rs（留空取内置白名单）"
                value={includedFiles}
                onChange={(e) => setIncludedFiles(e.target.value)}
              />
            </div>
          </div>
        </div>

        <DialogFooter>
          <Button variant="outline" onClick={() => onOpenChange(false)}>
            取消
          </Button>
          <Button onClick={handleSubmit} disabled={submitting}>
            {submitting ? "提交中..." : "开始导入"}
          </Button>
        </DialogFooter>
      </DialogContent>
    </Dialog>
  );
}

function SourcesTab({ kb }: { kb: KnowledgeBase }) {
  const toast = useToast();
  const [sources, setSources] = useState<KbSource[]>([]);
  const [loading, setLoading] = useState(true);
  const [importOpen, setImportOpen] = useState(false);
  const [deleteTarget, setDeleteTarget] = useState<KbSource | null>(null);
  const [deleting, setDeleting] = useState(false);

  const refresh = useCallback(async () => {
    try {
      setSources(await knowledgeApi.listSources(kb.id));
    } catch (e) {
      console.error("加载来源失败：", e);
      setSources([]);
    } finally {
      setLoading(false);
    }
  }, [kb.id]);

  useEffect(() => {
    refresh();
  }, [refresh]);

  // 任一来源仍在导入中 → 每 2s 轮询进度
  const polling = sources.some((s) => s.status === "fetching");
  useEffect(() => {
    if (!polling) return;
    const t = setInterval(refresh, 2000);
    return () => clearInterval(t);
  }, [polling, refresh]);

  const confirmDelete = async () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setDeleting(true);
    try {
      await knowledgeApi.deleteSource(target.id);
      toast.success("已删除来源及其导入的文档");
      setDeleteTarget(null);
      await refresh();
    } catch (e) {
      console.error("删除来源失败：", e);
      toast.error(`删除失败：${errMsg(e) || "请重试"}`);
    } finally {
      setDeleting(false);
    }
  };

  return (
    <div className="space-y-4">
      <div className="flex items-center justify-between">
        <div>
          <h3 className="text-sm font-medium">来源</h3>
          <p className="text-xs text-muted-foreground">
            一次导入任务的记录，可追踪进度、复跑与删除。
          </p>
        </div>
        <Button size="sm" onClick={() => setImportOpen(true)}>
          <Plus />
          导入来源
        </Button>
      </div>

      {loading ? (
        <div className="space-y-2">
          <Skeleton className="h-12 w-full" />
          <Skeleton className="h-12 w-full" />
        </div>
      ) : sources.length === 0 ? (
        <EmptyState
          icon={Database}
          title="暂无来源"
          description="点击「导入来源」从 Git 仓库、单个 URL 或本地目录批量摄入文档。"
        />
      ) : (
        <div className="divide-y rounded-lg border">
          {sources.map((s) => {
            const Icon =
              s.source_type === "git"
                ? GitBranch
                : s.source_type === "url"
                  ? Link2
                  : FolderOpen;
            return (
              <div
                key={s.id}
                className="flex items-center justify-between gap-3 px-3 py-2"
              >
                <div className="min-w-0">
                  <div className="flex items-center gap-2">
                    <Icon className="h-4 w-4 shrink-0 text-muted-foreground" />
                    <span className="truncate font-medium">
                      {sourceRefLabel(s)}
                    </span>
                    <span className="shrink-0 rounded bg-muted px-1.5 py-0.5 text-[11px] text-muted-foreground">
                      {SOURCE_TYPE_LABEL[s.source_type] ?? s.source_type}
                    </span>
                    <SourceStatusBadge s={s} />
                  </div>
                  <p className="mt-0.5 text-[11px] text-muted-foreground">
                    {s.status === "done"
                      ? `${s.file_count} 个文件`
                      : s.status === "fetching"
                        ? "正在扫描与向量化…"
                        : (s.error_message ?? "导入失败")}
                    {s.subpath ? ` · 子路径 ${s.subpath}` : ""}
                  </p>
                </div>
                <Button
                  variant="ghost"
                  size="icon"
                  title="删除来源"
                  className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                  onClick={() => setDeleteTarget(s)}
                  disabled={s.status === "fetching"}
                >
                  <Trash2 />
                </Button>
              </div>
            );
          })}
        </div>
      )}

      <ImportSourceDialog
        kbId={kb.id}
        open={importOpen}
        onOpenChange={setImportOpen}
        onImported={refresh}
      />

      <Dialog
        open={deleteTarget !== null}
        onOpenChange={(o) => !o && setDeleteTarget(null)}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              删除来源
            </DialogTitle>
            <DialogDescription>
              确认删除来源「{deleteTarget ? sourceRefLabel(deleteTarget) : ""}」？其导入产生的全部文档与向量分块将一并移除，操作不可恢复。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>
              取消
            </Button>
            <Button
              variant="destructive"
              onClick={confirmDelete}
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

// ---------------------------------------------------------------------------
// 页面
// ---------------------------------------------------------------------------

export function KnowledgeBaseDetailPage() {
  const navigate = useNavigate();
  const { kbId } = useParams<{ kbId: string }>();
  const toast = useToast();

  const [kb, setKb] = useState<KnowledgeBase | null>(null);
  const [loading, setLoading] = useState(true);
  const [notFound, setNotFound] = useState(false);

  useEffect(() => {
    if (!kbId) return;
    setLoading(true);
    knowledgeApi
      .list()
      .then((list) => {
        const found = list.find((k) => k.id === kbId) ?? null;
        if (!found) {
          setNotFound(true);
        } else {
          setKb(found);
        }
      })
      .catch((e) => {
        console.error("加载知识库失败：", e);
        toast.error("加载知识库失败，请返回重试");
        setNotFound(true);
      })
      .finally(() => setLoading(false));
  }, [kbId]);

  if (loading) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-8 w-64" />
        <Skeleton className="h-9 w-full" />
        <Skeleton className="h-64 w-full" />
      </div>
    );
  }

  if (notFound || !kb) {
    return (
      <div className="flex flex-col items-center justify-center py-24 text-center">
        <p className="text-sm font-medium">未找到该知识库</p>
        <p className="mt-1 text-xs text-muted-foreground">
          它可能已被删除，或链接已失效。
        </p>
        <Button
          variant="outline"
          size="sm"
          className="mt-4"
          onClick={() => navigate("/services")}
        >
          <ArrowLeft />
          返回服务
        </Button>
      </div>
    );
  }

  return (
    <div>
      {/* 面包屑：返回 + KB 名 + 统计 */}
      <div className="flex items-center gap-3">
        <Button
          variant="ghost"
          size="icon"
          title="返回服务"
          onClick={() => navigate("/services")}
        >
          <ArrowLeft />
        </Button>
        <div className="min-w-0">
          <div className="flex items-center gap-2">
            <h1 className="truncate text-xl font-semibold tracking-tight">
              {kb.name}
            </h1>
            <StatusBadge tone={kb.status === 1 ? "success" : "secondary"}>
              {kb.status === 1 ? "就绪" : "禁用"}
            </StatusBadge>
          </div>
          <p className="text-xs text-muted-foreground">
            {kb.doc_count} 文档 · {kb.chunk_count} 切片
            {kb.embedding_model ? ` · ${kb.embedding_model}` : ""}
          </p>
        </div>
      </div>

      {/* 顶部水平 Tabs */}
      <Tabs defaultValue="documents" className="mt-5 w-full">
        <TabsList className="w-full flex-wrap">
          {KB_TABS.map((t) => {
            const Icon = t.icon;
            return (
              <TabsTrigger key={t.id} value={t.id}>
                <Icon />
                {t.label}
              </TabsTrigger>
            );
          })}
        </TabsList>

        <TabsContent value="documents" className="mt-5">
          <DocumentsTab kb={kb} />
        </TabsContent>

        <TabsContent value="sources" className="mt-5">
          <SourcesTab kb={kb} />
        </TabsContent>

        <TabsContent value="retrieval" className="mt-5">
          <RetrievalTab kb={kb} />
        </TabsContent>

        <TabsContent value="ask" className="mt-5">
          <AskTab kb={kb} />
        </TabsContent>

        <TabsContent value="index" className="mt-5">
          <IndexTab kb={kb} />
        </TabsContent>

        <TabsContent value="settings" className="mt-5">
          <SettingsTab kb={kb} onSaved={(next) => setKb(next)} />
        </TabsContent>

        <TabsContent value="mcp" className="mt-5">
          <McpTab kb={kb} />
        </TabsContent>
      </Tabs>
    </div>
  );
}
