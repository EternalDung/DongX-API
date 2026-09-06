import { useCallback, useEffect, useState, useRef } from "react";
import { sleep } from "@/lib/async";
import { useNavigate } from "react-router-dom";
import { Globe, Plus, Trash2, RefreshCw, AlertTriangle, FileText, Database, Link2 } from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
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
import { wikiApi, channelApi } from "@/lib/api";
import type { Channel, WikiProject, WikiProjectInput } from "@/types";

/** 与 RAG 列表一致的头像配色：低透明底 + 同色系文字 + 细边框。 */
const AVATAR_BG = [
  "bg-sky-500/15 text-sky-600 ring-sky-500/25 dark:text-sky-400",
  "bg-teal-500/15 text-teal-600 ring-teal-500/25 dark:text-teal-400",
  "bg-indigo-500/15 text-indigo-600 ring-indigo-500/25 dark:text-indigo-400",
  "bg-orange-500/15 text-orange-600 ring-orange-500/25 dark:text-orange-400",
  "bg-fuchsia-500/15 text-fuchsia-600 ring-fuchsia-500/25 dark:text-fuchsia-400",
  "bg-lime-500/15 text-lime-600 ring-lime-500/25 dark:text-lime-400",
];

function avatarColor(name: string): string {
  let h = 0;
  for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0;
  return AVATAR_BG[h % AVATAR_BG.length];
}

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

interface ProjectForm {
  name: string;
  description: string;
  channel_id: string;
  model: string;
}

function emptyForm(): ProjectForm {
  return { name: "", description: "", channel_id: "", model: "" };
}

// ---------------------------------------------------------------------------
// 单个项目行
// 操作区（开关 / 删除）阻断行点击，避免被带去详情页。
// ---------------------------------------------------------------------------

function WikiProjectRow({
  project,
  onOpen,
  onDelete,
  onToggleStatus,
  busy = false,
}: {
  project: WikiProject;
  onOpen: (p: WikiProject) => void;
  onDelete: (p: WikiProject) => void;
  onToggleStatus: (p: WikiProject, next: 0 | 1) => void;
  busy?: boolean;
}) {
  const enabled = project.status === 1;
  return (
    <div
      role="button"
      tabIndex={0}
      onClick={() => onOpen(project)}
      onKeyDown={(e) => {
        if (e.key === "Enter" || e.key === " ") onOpen(project);
      }}
      className="group flex cursor-pointer items-center gap-4 rounded-lg border bg-card/30 px-4 py-4 transition-colors hover:bg-accent/50"
    >
      <div
        aria-hidden
        className={cn(
          "flex h-12 w-12 shrink-0 items-center justify-center rounded-lg ring-1 ring-inset",
          avatarColor(project.name),
        )}
      >
        <Globe size={20} />
      </div>

      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-2">
          <span className="truncate text-base font-semibold">{project.name}</span>
          {/* 项目只有「就绪 / 禁用」两态：源在摄入不冒泡成项目状态 */}
          <StatusBadge tone={enabled ? "success" : "warning"}>
            {enabled ? "就绪" : "禁用"}
          </StatusBadge>
        </div>
        {project.description && (
          <p className="mt-1 line-clamp-1 text-sm text-muted-foreground">{project.description}</p>
        )}
        <p className="mt-1.5 flex flex-wrap items-center gap-x-3 gap-y-0.5 text-xs text-muted-foreground">
          <span className="inline-flex items-center gap-1">
            <Database size={12} />
            {project.source_count} 源
          </span>
          <span className="inline-flex items-center gap-1">
            <FileText size={12} />
            {project.page_count} 页面
          </span>
          <span className="inline-flex items-center gap-1">
            <Link2 size={12} />
            {project.link_count} 引用
          </span>
          {project.updated_at && <span>更新 {fmtTime(project.updated_at)}</span>}
        </p>
      </div>

      <div className="flex shrink-0 items-center gap-3 pl-2" onClick={(e) => e.stopPropagation()}>
        <div className="flex flex-col items-center gap-1" title="MCP 暴露：后端接入后开放">
          <Switch checked={project.mcp_exposed === 1} disabled title="MCP 暴露：后端接入后开放" />
          <span className="text-[10px] text-muted-foreground">MCP</span>
        </div>
        <div className="flex flex-col items-center gap-1">
          <Switch
            checked={enabled}
            disabled={busy}
            title={enabled ? "已启用，点击禁用" : "已禁用，点击启用"}
            onCheckedChange={(v) => onToggleStatus(project, v ? 1 : 0)}
          />
          <span className="text-[10px] text-muted-foreground">启用</span>
        </div>
        <Button
          variant="ghost"
          size="icon"
          title="删除 Wiki 项目"
          className="text-destructive hover:bg-destructive/10 hover:text-destructive"
          disabled={busy}
          onClick={() => onDelete(project)}
        >
          <Trash2 />
        </Button>
      </div>
    </div>
  );
}

// ---------------------------------------------------------------------------

export function WikiTabPanel() {
  const navigate = useNavigate();
  const toast = useToast();

  const [projects, setProjects] = useState<WikiProject[]>([]);
  const [loading, setLoading] = useState(true);

  const [dialogOpen, setDialogOpen] = useState(false);
  const [form, setForm] = useState<ProjectForm>(emptyForm());
  const [saving, setSaving] = useState(false);

  const [deleteTarget, setDeleteTarget] = useState<WikiProject | null>(null);
  const [deleting, setDeleting] = useState(false);

  // 生成用渠道/模型：取自真实渠道列表（该能力后端已有）
  const [channels, setChannels] = useState<Channel[]>([]);
  const [models, setModels] = useState<string[]>([]);

  // 方案A：解耦「内容出现」与「刷新按钮旋转」。
  const [spinning, setSpinning] = useState(false);
  const projectsRef = useRef<WikiProject[]>([]);
  projectsRef.current = projects;
  const load = useCallback(async () => {
    const showSkeleton = projectsRef.current.length === 0;
    if (showSkeleton) setLoading(true);
    setSpinning(true);
    const started = Date.now();
    try {
      const list = await wikiApi.list();
      setProjects(list);
    } catch (e) {
      console.error("Wiki 项目列表加载失败：", e);
      setProjects([]);
    } finally {
      if (showSkeleton) setLoading(false);
      const elapsed = Date.now() - started;
      if (elapsed < 400) await sleep(400 - elapsed);
      setSpinning(false);
    }
  }, []);

  useEffect(() => {
    void load();
    channelApi
      .list()
      .then(setChannels)
      .catch(() => setChannels([]));
  }, [load]);

  // 选中渠道后刷新可选模型
  useEffect(() => {
    const c = channels.find((x) => x.id === form.channel_id);
    setModels((c?.models ?? []).slice().sort());
  }, [channels, form.channel_id]);

  const openCreate = () => {
    setForm(emptyForm());
    setDialogOpen(true);
  };

  const handleCreate = async () => {
    if (!form.name.trim()) return;
    setSaving(true);
    try {
      const input: WikiProjectInput = {
        name: form.name.trim(),
        description: form.description.trim(),
        channel_id: form.channel_id,
        model: form.model,
      };
      const created = await wikiApi.create(input);
      toast.success(`Wiki 项目「${created.name}」已创建，去添加来源吧`);
      setDialogOpen(false);
      await load();
    } catch (e) {
      console.error("创建 Wiki 项目失败：", e);
      toast.error(errMsg(e) || "创建失败，请重试");
    } finally {
      setSaving(false);
    }
  };

  const handleConfirmDelete = async () => {
    if (!deleteTarget) return;
    const target = deleteTarget;
    setDeleting(true);
    try {
      await wikiApi.remove(target.id);
      toast.success(`已删除 Wiki 项目「${target.name}」`);
      setDeleteTarget(null);
      await load();
    } catch (e) {
      console.error("删除 Wiki 项目失败：", e);
      toast.error("删除失败，请重试");
    } finally {
      setDeleting(false);
    }
  };

  // 行内开关：乐观更新 + 失败回滚，仅对单行标记 in-flight
  const [togglingId, setTogglingId] = useState<string | null>(null);

  const handleToggleStatus = async (p: WikiProject, next: 0 | 1) => {
    setTogglingId(p.id);
    setProjects((prev) => prev.map((x) => (x.id === p.id ? { ...x, status: next } : x)));
    try {
      await wikiApi.update(p.id, { status: next });
      toast.success(next === 1 ? "Wiki 项目已启用" : "Wiki 项目已禁用");
    } catch (e) {
      setProjects((prev) => prev.map((x) => (x.id === p.id ? { ...x, status: p.status } : x)));
      console.error("更新 Wiki 项目失败：", e);
      toast.error(errMsg(e) || "更新失败，请重试");
    } finally {
      setTogglingId(null);
    }
  };

  return (
    <div>
      <div className="flex items-center justify-between">
        <h2 className="text-sm font-medium text-muted-foreground">Wiki 项目</h2>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={load} disabled={spinning}>
            <RefreshCw className={spinning ? "animate-spin" : ""} />
            刷新
          </Button>
          <Button size="sm" onClick={openCreate} className="border border-primary/30 font-semibold shadow-sm">
            <Plus />
            新建 Wiki
          </Button>
        </div>
      </div>

      <Card className="mt-3">
        <CardContent className="pt-2">
          {loading ? (
            <div className="space-y-2 py-4">
              {Array.from({ length: 2 }).map((_, i) => (
                <Skeleton key={i} className="h-14 w-full" />
              ))}
            </div>
          ) : projects.length === 0 ? (
            <EmptyState
              icon={Globe}
              title="暂无 Wiki 项目"
              description="Wiki 会把来源资料交给模型阅读消化，沉淀成可增量维护的结构化页面。新建一个空白项目，再到详情页添加来源。"
              action={
                <Button size="sm" variant="outline" onClick={openCreate}>
                  <Plus />
                  新建第一个 Wiki
                </Button>
              }
            />
          ) : (
            <div className="space-y-3 py-2">
              {projects.map((p) => (
                <WikiProjectRow
                  key={p.id}
                  project={p}
                  busy={togglingId === p.id || deleting}
                  onOpen={(x) => navigate(`/services/wiki/${x.id}`)}
                  onDelete={setDeleteTarget}
                  onToggleStatus={handleToggleStatus}
                />
              ))}
            </div>
          )}
        </CardContent>
      </Card>

      {/* 新建 Wiki 项目：空白项目，源留到详情页添加 */}
      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="sm:max-w-lg">
          <DialogHeader>
            <DialogTitle>新建 Wiki 项目</DialogTitle>
            <DialogDescription>
              创建的是一个空白项目。来源（Git 仓库 / 网页 / 本地目录）请在项目详情页添加并触发摄入。
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-4">
            <div className="grid gap-2">
              <Label htmlFor="wiki-name">名称</Label>
              <Input
                id="wiki-name"
                placeholder="如：DongX 架构笔记"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="wiki-desc">描述</Label>
              <Input
                id="wiki-desc"
                placeholder="可选，简述这个知识体覆盖什么"
                value={form.description}
                onChange={(e) => setForm({ ...form, description: e.target.value })}
              />
            </div>
            <div className="grid gap-4 sm:grid-cols-2">
              <div className="grid gap-2">
                <Label htmlFor="wiki-channel">生成渠道</Label>
                <Select
                  value={form.channel_id || undefined}
                  onValueChange={(v) => setForm({ ...form, channel_id: v, model: "" })}
                  disabled={channels.length === 0}
                >
                  <SelectTrigger id="wiki-channel" className="w-full">
                    <SelectValue placeholder={channels.length === 0 ? "无可用渠道" : "请选择渠道"} />
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
                <Label htmlFor="wiki-model">生成模型</Label>
                <Select
                  value={form.model || undefined}
                  onValueChange={(v) => setForm({ ...form, model: v })}
                  disabled={models.length === 0}
                >
                  <SelectTrigger id="wiki-model" className="w-full">
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
            <p className="text-xs text-muted-foreground">
              模型负责阅读来源资料并生成/更新页面。渠道与模型后续可在项目设置中修改。
            </p>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDialogOpen(false)}>
              取消
            </Button>
            <Button onClick={handleCreate} disabled={saving || !form.name.trim()}>
              {saving ? "创建中..." : "创建"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 删除确认 */}
      <Dialog open={deleteTarget !== null} onOpenChange={(o) => !o && setDeleteTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              删除 Wiki 项目
            </DialogTitle>
            <DialogDescription>
              确认删除「{deleteTarget?.name}」？其下全部页面与来源配置将一并移除，操作不可恢复。
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
