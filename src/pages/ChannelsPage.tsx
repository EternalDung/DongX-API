import { useEffect, useState } from "react";
import {
  Plus,
  Pencil,
  Trash2,
  Zap,
  RefreshCw,
  Network,
  AlertTriangle,
} from "lucide-react";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusBadge, type StatusTone } from "@/components/ui/status-badge";
import { useToast } from "@/components/ui/toast";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { channelApi, type ChannelInput } from "@/lib/api";
import type { Channel, ProviderPreset } from "@/types";

const emptyForm = (): ChannelInput => ({
  name: "",
  protocol: "openai",
  type: "openai",
  base_url: "",
  api_key: "",
  models: [],
  priority: 0,
  weight: 1,
  config: {},
  model_mapping: {},
  endpoints: [],
});

const CHANNEL_TONE: Record<number, { tone: StatusTone; label: string }> = {
  1: { tone: "success", label: "启用" },
  2: { tone: "destructive", label: "异常" },
  0: { tone: "secondary", label: "禁用" },
};

export function ChannelsPage() {
  const toast = useToast();
  const [channels, setChannels] = useState<Channel[]>([]);
  const [presets, setPresets] = useState<ProviderPreset[]>([]);
  const [loading, setLoading] = useState(true);

  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState<ChannelInput>(emptyForm());
  const [modelsText, setModelsText] = useState("");
  const [saving, setSaving] = useState(false);

  const [testingId, setTestingId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, boolean>>({});

  const [deleteTarget, setDeleteTarget] = useState<Channel | null>(null);
  const [deleting, setDeleting] = useState(false);

  const load = async () => {
    setLoading(true);
    try {
      const [list, ps] = await Promise.all([channelApi.list(), channelApi.presets()]);
      setChannels(list);
      setPresets(ps);
    } catch (e) {
      console.error("Failed to load channels:", e);
      toast.error("渠道列表加载失败");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const openCreate = () => {
    setEditingId(null);
    setForm(emptyForm());
    setModelsText("");
    setDialogOpen(true);
  };

  const openEdit = (ch: Channel) => {
    setEditingId(ch.id);
    setForm({
      name: ch.name,
      protocol: ch.protocol,
      type: ch.type,
      base_url: ch.base_url,
      api_key: "",
      models: ch.models,
      priority: ch.priority,
      weight: ch.weight,
      config: ch.config,
      model_mapping: ch.model_mapping,
      endpoints: ch.endpoints,
    });
    setModelsText(ch.models.join(", "));
    setDialogOpen(true);
  };

  const handleSave = async () => {
    if (!form.name.trim() || !form.base_url.trim()) return;
    setSaving(true);
    try {
      const input: ChannelInput = {
        ...form,
        models: modelsText
          .split(/[,\n]/)
          .map((s) => s.trim())
          .filter(Boolean),
      };
      if (editingId) {
        await channelApi.update(editingId, input);
        toast.success("渠道已更新");
      } else {
        await channelApi.create(input);
        toast.success("渠道已创建");
      }
      setDialogOpen(false);
      await load();
    } catch (e) {
      console.error("Failed to save channel:", e);
      toast.error("保存失败，请检查配置");
    } finally {
      setSaving(false);
    }
  };

  const handleConfirmDelete = async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      await channelApi.remove(deleteTarget.id);
      toast.success(`已删除渠道「${deleteTarget.name}」`);
      setDeleteTarget(null);
      await load();
    } catch (e) {
      console.error("Failed to delete channel:", e);
      toast.error("删除失败");
    } finally {
      setDeleting(false);
    }
  };

  const handleTest = async (id: string) => {
    setTestingId(id);
    try {
      const ok = await channelApi.test(id);
      setTestResults((prev) => ({ ...prev, [id]: ok }));
      toast[ok ? "success" : "error"](ok ? "连通性测试通过" : "连通性测试失败");
    } catch {
      setTestResults((prev) => ({ ...prev, [id]: false }));
      toast.error("连通性测试失败");
    } finally {
      setTestingId(null);
    }
  };

  const handleTypeChange = (type: string) => {
    const preset = presets.find((p) => p.type === type);
    setForm((f) => ({
      ...f,
      type,
      protocol: type === "claude" ? "anthropic" : type === "ollama" ? "ollama" : "openai",
      base_url:
        !f.base_url || presets.some((p) => p.default_base_url === f.base_url)
          ? preset?.default_base_url ?? ""
          : f.base_url,
    }));
  };

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">渠道管理</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            管理上游供应商渠道：OpenAI / DeepSeek / Claude / Gemini 等
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={load} disabled={loading}>
            <RefreshCw className={loading ? "animate-spin" : ""} />
            刷新
          </Button>
          <Button size="sm" onClick={openCreate}>
            <Plus />
            添加渠道
          </Button>
        </div>
      </div>

      <Card className="mt-6">
        <CardContent className="pt-2">
          {loading ? (
            <div className="space-y-2 py-4">
              {Array.from({ length: 4 }).map((_, i) => (
                <Skeleton key={i} className="h-14 w-full" />
              ))}
            </div>
          ) : channels.length === 0 ? (
            <EmptyState
              icon={Network}
              title="暂无渠道"
              description="添加第一个上游 LLM 供应商，网关即可开始统一代理请求。"
              action={
                <Button size="sm" variant="outline" onClick={openCreate}>
                  <Plus />
                  添加第一个渠道
                </Button>
              }
            />
          ) : (
            <div className="divide-y">
              {channels.map((ch) => {
                const meta = CHANNEL_TONE[ch.status] ?? CHANNEL_TONE[0];
                const tested = testResults[ch.id];
                return (
                  <div
                    key={ch.id}
                    className="group flex items-center justify-between gap-4 rounded-lg px-2 py-3 transition-colors hover:bg-accent/40"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-medium">{ch.name}</span>
                        <Badge variant="outline" className="font-mono text-[11px]">
                          {ch.type}
                        </Badge>
                        <StatusBadge tone={meta.tone}>{meta.label}</StatusBadge>
                        {tested !== undefined && (
                          <StatusBadge tone={tested ? "success" : "destructive"}>
                            {tested ? "连通" : "失败"}
                          </StatusBadge>
                        )}
                      </div>
                      <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
                        {ch.base_url} · {ch.models.length} 模型 · 权重 {ch.weight} · 优先级{" "}
                        {ch.priority}
                      </p>
                    </div>
                    <div className="flex shrink-0 items-center gap-1 opacity-70 transition-opacity group-hover:opacity-100">
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => handleTest(ch.id)}
                        disabled={testingId === ch.id}
                      >
                        <Zap className={testingId === ch.id ? "animate-pulse" : ""} />
                        测试
                      </Button>
                      <Button variant="ghost" size="sm" onClick={() => openEdit(ch)}>
                        <Pencil />
                        编辑
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                        onClick={() => setDeleteTarget(ch)}
                      >
                        <Trash2 />
                        删除
                      </Button>
                    </div>
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>

      {/* 新建 / 编辑 Dialog */}
      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle>{editingId ? "编辑渠道" : "添加渠道"}</DialogTitle>
            <DialogDescription>
              {editingId
                ? "修改渠道配置。密钥留空表示保持不变。"
                : "配置上游 LLM 供应商渠道。"}
            </DialogDescription>
          </DialogHeader>

          <div className="grid gap-4">
            <div className="grid grid-cols-2 gap-4">
              <div className="grid gap-2">
                <Label htmlFor="ch-type">渠道类型</Label>
                <Input
                  id="ch-type"
                  value={form.type}
                  readOnly
                  className="bg-muted/50 font-mono text-xs"
                />
                <SelectType value={form.type} presets={presets} onChange={handleTypeChange} />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="ch-name">渠道名称</Label>
                <Input
                  id="ch-name"
                  placeholder="如：OpenAI 官方"
                  value={form.name}
                  onChange={(e) => setForm({ ...form, name: e.target.value })}
                />
              </div>
            </div>

            <div className="grid gap-2">
              <Label htmlFor="ch-url">Base URL</Label>
              <Input
                id="ch-url"
                placeholder="https://api.openai.com/v1"
                value={form.base_url}
                onChange={(e) => setForm({ ...form, base_url: e.target.value })}
              />
            </div>

            <div className="grid gap-2">
              <Label htmlFor="ch-key">
                API Key{editingId && <span className="text-xs text-muted-foreground">（留空保持不变）</span>}
              </Label>
              <Input
                id="ch-key"
                type="password"
                placeholder={editingId ? "••••••••" : "sk-..."}
                value={form.api_key}
                onChange={(e) => setForm({ ...form, api_key: e.target.value })}
              />
            </div>

            <div className="grid gap-2">
              <Label htmlFor="ch-models">
                模型列表<span className="text-xs text-muted-foreground">（逗号或换行分隔）</span>
              </Label>
              <Input
                id="ch-models"
                placeholder="gpt-4o, gpt-4o-mini, o3"
                value={modelsText}
                onChange={(e) => setModelsText(e.target.value)}
              />
            </div>

            <div className="grid grid-cols-2 gap-4">
              <div className="grid gap-2">
                <Label htmlFor="ch-weight">
                  权重<span className="text-xs text-muted-foreground">（同优先级负载均衡）</span>
                </Label>
                <Input
                  id="ch-weight"
                  type="number"
                  min={1}
                  max={100}
                  value={form.weight}
                  onChange={(e) =>
                    setForm({ ...form, weight: Number(e.target.value) || 1 })
                  }
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="ch-priority">
                  优先级<span className="text-xs text-muted-foreground">（越大越优先）</span>
                </Label>
                <Input
                  id="ch-priority"
                  type="number"
                  value={form.priority}
                  onChange={(e) =>
                    setForm({ ...form, priority: Number(e.target.value) || 0 })
                  }
                />
              </div>
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setDialogOpen(false)}>
              取消
            </Button>
            <Button
              onClick={handleSave}
              disabled={saving || !form.name.trim() || !form.base_url.trim()}
            >
              {saving ? "保存中..." : "保存"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 删除确认 Dialog */}
      <Dialog open={deleteTarget !== null} onOpenChange={(o) => !o && setDeleteTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              删除渠道
            </DialogTitle>
            <DialogDescription>
              确认删除渠道「{deleteTarget?.name}」？此操作不可恢复，相关路由配置将一并移除。
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

/** 渠道类型选择：用带品牌感的按钮组代替原生下拉 */
function SelectType({
  value,
  presets,
  onChange,
}: {
  value: string;
  presets: ProviderPreset[];
  onChange: (v: string) => void;
}) {
  const list = presets.length
    ? presets
    : [
        { type: "openai", label: "OpenAI" },
        { type: "deepseek", label: "DeepSeek" },
        { type: "claude", label: "Claude" },
        { type: "gemini", label: "Gemini" },
        { type: "custom", label: "自定义" },
      ];
  return (
    <div className="flex flex-wrap gap-1.5">
      {list.map((p) => (
        <button
          key={p.type}
          type="button"
          onClick={() => onChange(p.type)}
          className={
            "rounded-md border px-2.5 py-1 text-xs transition-colors " +
            (value === p.type
              ? "border-primary bg-primary/10 text-primary"
              : "text-muted-foreground hover:bg-accent/50")
          }
        >
          {p.label}
        </button>
      ))}
    </div>
  );
}
