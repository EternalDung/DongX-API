import { useEffect, useRef, useState } from "react";
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
import { knowledgeApi, channelApi } from "@/lib/api";
import type { AskResult, KbDocument, KnowledgeBase, KnowledgeBaseInput } from "@/types";

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
// 单个知识库行
// ---------------------------------------------------------------------------

function KnowledgeBaseRow({
  kb,
  onDetail,
  onDelete,
}: {
  kb: KnowledgeBase;
  onDetail: (kb: KnowledgeBase) => void;
  onDelete: (kb: KnowledgeBase) => void;
}) {
  const tone = kb.status === 1 ? "success" : "secondary";
  const label = kb.status === 1 ? "就绪" : "禁用";
  return (
    <div className="group flex items-center justify-between gap-3 rounded-lg px-2 py-3 transition-colors hover:bg-accent/40">
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="font-medium">{kb.name}</span>
          <StatusBadge tone={tone}>{label}</StatusBadge>
        </div>
        {kb.description && (
          <p className="mt-1 line-clamp-2 text-xs text-muted-foreground">
            {kb.description}
          </p>
        )}
        <p className="mt-1 text-[11px] text-muted-foreground">
          {kb.doc_count} 文档 · {kb.chunk_count} 片段
          {kb.embedding_model ? ` · ${kb.embedding_model}` : ""}
          {kb.updated_at ? ` · 更新 ${fmtTime(kb.updated_at)}` : ""}
        </p>
      </div>
      <div className="flex shrink-0 items-center gap-1 opacity-70 transition-opacity group-hover:opacity-100">
        <Button variant="ghost" size="sm" onClick={() => onDetail(kb)}>
          详情
        </Button>
        <Button
          variant="ghost"
          size="icon"
          title="删除知识库"
          className="text-destructive hover:bg-destructive/10 hover:text-destructive"
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

  // 知识库详情（摄入文本 / 问答）
  const [detailTarget, setDetailTarget] = useState<KnowledgeBase | null>(null);
  const [ingestTitle, setIngestTitle] = useState("");
  const [ingestText, setIngestText] = useState("");
  const [ingesting, setIngesting] = useState(false);
  const [askQuestion, setAskQuestion] = useState("");
  const [askModel, setAskModel] = useState("");
  const [asking, setAsking] = useState(false);
  const [askResult, setAskResult] = useState<AskResult | null>(null);
  // 全部启用渠道的模型（去重），作为「回答模型」输入建议
  const [channelModels, setChannelModels] = useState<string[]>([]);

  // 知识库文档列表（详情对话框内展示 / 删除）
  const [documents, setDocuments] = useState<KbDocument[]>([]);
  const [loadingDocs, setLoadingDocs] = useState(false);
  const [docDeleteTarget, setDocDeleteTarget] = useState<KbDocument | null>(null);
  const [deletingDoc, setDeletingDoc] = useState(false);

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

  const loadDocuments = async (kbId: string) => {
    setLoadingDocs(true);
    try {
      const docs = await knowledgeApi.documents(kbId);
      setDocuments(docs);
      // 用文档列表重算统计，确保与库一致（摄入 / 删除后计数同步）。
      const docCount = docs.length;
      const chunkCount = docs.reduce((s, d) => s + (d.chunk_count || 0), 0);
      setDetailTarget((prev) =>
        prev ? { ...prev, doc_count: docCount, chunk_count: chunkCount } : prev,
      );
      setKbs((prev) =>
        prev.map((k) =>
          k.id === kbId ? { ...k, doc_count: docCount, chunk_count: chunkCount } : k,
        ),
      );
    } catch (e) {
      console.error("加载文档失败：", e);
      setDocuments([]);
    } finally {
      setLoadingDocs(false);
    }
  };

  const openDetail = (kb: KnowledgeBase) => {
    setDetailTarget(kb);
    setIngestTitle("");
    setIngestText("");
    setAskQuestion("");
    setAskModel("");
    setAskResult(null);
    // 载入全部启用渠道的模型，作为「回答模型」输入建议（问答由网关全局分发）。
    channelApi
      .list()
      .then((chs) =>
        setChannelModels(
          Array.from(
            new Set(
              chs
                .filter((c) => c.status === 1)
                .flatMap((c) => c.models ?? []),
            ),
          ).sort(),
        ),
      )
      .catch(() => setChannelModels([]));
    // 载入该知识库下的文档列表。
    loadDocuments(kb.id);
  };

  const handleIngest = async () => {
    if (!detailTarget) return;
    if (!ingestTitle.trim() || !ingestText.trim()) {
      toast.error("标题与文本内容均不能为空");
      return;
    }
    setIngesting(true);
    try {
      const res = await knowledgeApi.ingest(
        detailTarget.id,
        ingestTitle.trim(),
        ingestText.trim(),
      );
      toast.success(`已摄入「${ingestTitle.trim()}」，共 ${res.chunk_count} 个片段`);
      // 本地更新统计，避免重新拉取列表。
      setDetailTarget((prev) =>
        prev
          ? {
              ...prev,
              doc_count: prev.doc_count + 1,
              chunk_count: prev.chunk_count + res.chunk_count,
            }
          : prev,
      );
      setKbs((prev) =>
        prev.map((k) =>
          k.id === detailTarget.id
            ? {
                ...k,
                doc_count: k.doc_count + 1,
                chunk_count: k.chunk_count + res.chunk_count,
              }
            : k,
        ),
      );
      setIngestTitle("");
      setIngestText("");
    } catch (e) {
      const msg =
        e && typeof e === "object" && "message" in e
          ? String((e as { message: unknown }).message)
          : String(e);
      console.error("摄入文本失败：", e);
      toast.error(msg || "摄入失败，请重试");
    } finally {
      setIngesting(false);
    }
  };

  const handleAsk = async () => {
    if (!detailTarget) return;
    if (!askQuestion.trim()) {
      toast.error("问题不能为空");
      return;
    }
    if (!askModel.trim()) {
      toast.error("请填写用于生成回答的 chat 模型");
      return;
    }
    setAsking(true);
    setAskResult(null);
    try {
      const res = await knowledgeApi.ask(
        [detailTarget.id],
        askQuestion.trim(),
        askModel.trim(),
      );
      setAskResult(res);
    } catch (e) {
      const msg =
        e && typeof e === "object" && "message" in e
          ? String((e as { message: unknown }).message)
          : String(e);
      console.error("问答失败：", e);
      toast.error(msg || "问答失败，请重试");
    } finally {
      setAsking(false);
    }
  };

  const handleConfirmDeleteDoc = async () => {
    if (!docDeleteTarget || !detailTarget) return;
    const target = docDeleteTarget;
    setDeletingDoc(true);
    try {
      await knowledgeApi.removeDocument(target.id);
      toast.success(`已删除文档「${target.title}」`);
      setDocDeleteTarget(null);
      await loadDocuments(detailTarget.id);
    } catch (e) {
      const msg =
        e && typeof e === "object" && "message" in e
          ? String((e as { message: unknown }).message)
          : String(e);
      console.error("删除文档失败：", e);
      toast.error(msg || "删除失败，请重试");
    } finally {
      setDeletingDoc(false);
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
                  description="知识库是 RAG 检索的数据源。新建一个知识库并摄入文档后，即可在问答中检索引用。后端 RAG 模块待接入（Phase 1）。"
                  action={
                    <Button size="sm" variant="outline" onClick={openCreate}>
                      <Plus />
                      新建第一个知识库
                    </Button>
                  }
                />
              ) : (
                <div className="divide-y">
                  {kbs.map((kb) => (
                    <KnowledgeBaseRow
                      key={kb.id}
                      kb={kb}
                      onDetail={openDetail}
                      onDelete={setDeleteTarget}
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

      {/* 知识库详情：摄入文本 / 问答 */}
      <Dialog
        open={detailTarget !== null}
        onOpenChange={(o) => !o && setDetailTarget(null)}
      >
        <DialogContent className="sm:max-w-2xl max-h-[85vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              {detailTarget?.name}
              {detailTarget && (
                <StatusBadge tone={detailTarget.status === 1 ? "success" : "secondary"}>
                  {detailTarget.status === 1 ? "就绪" : "禁用"}
                </StatusBadge>
              )}
            </DialogTitle>
            <DialogDescription>
              {detailTarget?.description || "暂无描述"}
            </DialogDescription>
          </DialogHeader>

          {/* 统计条 */}
          <div className="flex flex-wrap items-center gap-x-4 gap-y-1 rounded-lg bg-muted/40 px-3 py-2 text-xs text-muted-foreground">
            <span>{detailTarget?.doc_count ?? 0} 文档</span>
            <span>{detailTarget?.chunk_count ?? 0} 片段</span>
            <span>嵌入模型：{detailTarget?.embedding_model || "—"}</span>
          </div>

          <Tabs defaultValue="ingest" className="w-full">
            <TabsList>
              <TabsTrigger value="ingest">摄入文本</TabsTrigger>
              <TabsTrigger value="docs">文档</TabsTrigger>
              <TabsTrigger value="ask">问答</TabsTrigger>
            </TabsList>

            {/* 摄入文本 */}
            <TabsContent value="ingest" className="mt-4 space-y-3">
              <div className="grid gap-2">
                <Label htmlFor="ingest-title">标题</Label>
                <Input
                  id="ingest-title"
                  placeholder="如：产品说明书第 3 章"
                  value={ingestTitle}
                  onChange={(e) => setIngestTitle(e.target.value)}
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="ingest-text">文本内容</Label>
                <Textarea
                  id="ingest-text"
                  placeholder="粘贴待向量化的文本，将自动按长度分块..."
                  rows={8}
                  value={ingestText}
                  onChange={(e) => setIngestText(e.target.value)}
                  className="font-mono text-xs"
                />
              </div>
              <div className="flex justify-end">
                <Button onClick={handleIngest} disabled={ingesting}>
                  {ingesting ? "摄入中..." : "摄入"}
                </Button>
              </div>
            </TabsContent>

            {/* 文档列表 */}
            <TabsContent value="docs" className="mt-4 space-y-3">
              {loadingDocs ? (
                <div className="space-y-2">
                  <Skeleton className="h-12 w-full" />
                  <Skeleton className="h-12 w-full" />
                </div>
              ) : documents.length === 0 ? (
                <p className="text-sm text-muted-foreground">
                  暂无文档，请到「摄入文本」粘贴资料。
                </p>
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
                        <p className="text-[11px] text-muted-foreground">
                          {d.chunk_count} 片段 · {fmtTime(d.created_at)}
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
            </TabsContent>

            {/* 问答 */}
            <TabsContent value="ask" className="mt-4 space-y-3">
              <div className="grid gap-2">
                <Label htmlFor="ask-question">问题</Label>
                <Textarea
                  id="ask-question"
                  placeholder="基于该知识库内容提出问题..."
                  rows={3}
                  value={askQuestion}
                  onChange={(e) => setAskQuestion(e.target.value)}
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="ask-model">回答模型</Label>
                <Input
                  id="ask-model"
                  list="kb-answer-model-options"
                  placeholder="用于生成回答的 chat 模型，如 gpt-4.1-mini / deepseek-chat"
                  value={askModel}
                  onChange={(e) => setAskModel(e.target.value)}
                  className="font-mono"
                />
                <datalist id="kb-answer-model-options">
                  {channelModels.map((m) => (
                    <option key={m} value={m} />
                  ))}
                </datalist>
                <p className="text-xs text-muted-foreground">
                  回答模型与嵌入模型相互独立，由网关在所有启用且支持该模型的渠道间分发。
                </p>
              </div>
              <div className="flex justify-end">
                <Button onClick={handleAsk} disabled={asking}>
                  {asking ? "检索中..." : "问答"}
                </Button>
              </div>

              {askResult && (
                <div className="mt-2 space-y-3 rounded-lg border p-3">
                  <div>
                    <p className="mb-1 text-xs font-medium text-muted-foreground">回答</p>
                    <p className="whitespace-pre-wrap text-sm leading-relaxed">
                      {askResult.answer}
                    </p>
                  </div>
                  {askResult.sources.length > 0 && (
                    <div>
                      <p className="mb-1 text-xs font-medium text-muted-foreground">
                        引用来源（{askResult.sources.length}）
                      </p>
                      <ul className="space-y-2">
                        {askResult.sources.map((s, i) => (
                          <li key={i} className="rounded-md bg-muted/40 p-2 text-xs">
                            <div className="mb-1 flex items-center justify-between">
                              <span className="font-medium">{s.doc_title}</span>
                              <span className="text-muted-foreground">
                                相似度 {s.score.toFixed(3)}
                              </span>
                            </div>
                            <p className="line-clamp-3 text-muted-foreground">{s.content}</p>
                          </li>
                        ))}
                      </ul>
                    </div>
                  )}
                </div>
              )}
            </TabsContent>
          </Tabs>
        </DialogContent>
      </Dialog>

      {/* 文档删除确认 Dialog */}
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
              onClick={handleConfirmDeleteDoc}
              disabled={deletingDoc}
            >
              {deletingDoc ? "删除中..." : "确认删除"}
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
