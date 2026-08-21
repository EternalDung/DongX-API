import { useEffect, useState } from "react";
import {
  Plus,
  Pencil,
  Trash2,
  Zap,
  RefreshCw,
  Network,
} from "lucide-react";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Select } from "@/components/ui/select";
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

/** 空表单 */
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

export function ChannelsPage() {
  const [channels, setChannels] = useState<Channel[]>([]);
  const [presets, setPresets] = useState<ProviderPreset[]>([]);
  const [loading, setLoading] = useState(true);

  // Dialog 状态
  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState<ChannelInput>(emptyForm());
  const [modelsText, setModelsText] = useState("");
  const [saving, setSaving] = useState(false);

  // 测试中的渠道 id
  const [testingId, setTestingId] = useState<string | null>(null);
  // 测试结果: channelId -> ok / fail
  const [testResults, setTestResults] = useState<Record<string, boolean>>({});

  const load = async () => {
    setLoading(true);
    try {
      const [list, ps] = await Promise.all([channelApi.list(), channelApi.presets()]);
      setChannels(list);
      setPresets(ps);
    } catch (e) {
      console.error("Failed to load channels:", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  // ---------- 新建 ----------
  const openCreate = () => {
    setEditingId(null);
    setForm(emptyForm());
    setModelsText("");
    setDialogOpen(true);
  };

  // ---------- 编辑 ----------
  const openEdit = (ch: Channel) => {
    setEditingId(ch.id);
    setForm({
      name: ch.name,
      protocol: ch.protocol,
      type: ch.type,
      base_url: ch.base_url,
      api_key: "", // 编辑时不回填密钥（后端只返回掩码）
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

  // ---------- 保存（新建 / 编辑） ----------
  const handleSave = async () => {
    if (!form.name.trim() || !form.base_url.trim()) {
      return;
    }
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
      } else {
        await channelApi.create(input);
      }
      setDialogOpen(false);
      await load();
    } catch (e) {
      console.error("Failed to save channel:", e);
    } finally {
      setSaving(false);
    }
  };

  // ---------- 删除 ----------
  const handleDelete = async (id: string, name: string) => {
    if (!window.confirm(`确认删除渠道「${name}」？此操作不可恢复。`)) return;
    try {
      await channelApi.remove(id);
      await load();
    } catch (e) {
      console.error("Failed to delete channel:", e);
    }
  };

  // ---------- 测试连通性 ----------
  const handleTest = async (id: string) => {
    setTestingId(id);
    try {
      const ok = await channelApi.test(id);
      setTestResults((prev) => ({ ...prev, [id]: ok }));
    } catch {
      setTestResults((prev) => ({ ...prev, [id]: false }));
    } finally {
      setTestingId(null);
    }
  };

  // 选择渠道类型时自动填充 base_url 和协议
  const handleTypeChange = (type: string) => {
    const preset = presets.find((p) => p.type === type);
    setForm((f) => ({
      ...f,
      type,
      protocol: type === "claude" ? "anthropic" : type === "ollama" ? "ollama" : "openai",
      // 仅当 base_url 为空或等于其他预设默认值时自动填充
      base_url:
        !f.base_url || presets.some((p) => p.default_base_url === f.base_url)
          ? preset?.default_base_url ?? ""
          : f.base_url,
    }));
  };

  return (
    <div className="p-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">渠道管理</h1>
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
            <div className="py-12 text-center text-sm text-muted-foreground">
              加载中...
            </div>
          ) : channels.length === 0 ? (
            <div className="flex flex-col items-center gap-2 py-12 text-center">
              <Network className="h-10 w-10 text-muted-foreground" />
              <p className="text-sm text-muted-foreground">暂无渠道</p>
              <Button size="sm" variant="outline" onClick={openCreate}>
                <Plus />
                添加第一个渠道
              </Button>
            </div>
          ) : (
            <div className="divide-y">
              {channels.map((ch) => (
                <div
                  key={ch.id}
                  className="flex items-center justify-between gap-4 py-3"
                >
                  <div className="min-w-0 flex-1">
                    <div className="flex items-center gap-2">
                      <span className="font-medium">{ch.name}</span>
                      <Badge variant="outline">{ch.type}</Badge>
                      {testResults[ch.id] !== undefined && (
                        <Badge variant={testResults[ch.id] ? "success" : "destructive"}>
                          {testResults[ch.id] ? "连通" : "失败"}
                        </Badge>
                      )}
                    </div>
                    <p className="mt-0.5 truncate text-xs text-muted-foreground">
                      {ch.base_url} · {ch.models.length} 模型 · 权重 {ch.weight} · 优先级 {ch.priority}
                    </p>
                  </div>
                  <div className="flex shrink-0 items-center gap-1">
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
                      className="text-destructive hover:text-destructive"
                      onClick={() => handleDelete(ch.id, ch.name)}
                    >
                      <Trash2 />
                      删除
                    </Button>
                  </div>
                </div>
              ))}
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
            {/* 渠道类型 */}
            <div className="grid grid-cols-2 gap-4">
              <div className="grid gap-2">
                <Label htmlFor="ch-type">渠道类型</Label>
                <Select
                  id="ch-type"
                  value={form.type}
                  onChange={(e) => handleTypeChange(e.target.value)}
                >
                  {presets.map((p) => (
                    <option key={p.type} value={p.type}>
                      {p.label}
                    </option>
                  ))}
                </Select>
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

            {/* Base URL */}
            <div className="grid gap-2">
              <Label htmlFor="ch-url">Base URL</Label>
              <Input
                id="ch-url"
                placeholder="https://api.openai.com/v1"
                value={form.base_url}
                onChange={(e) => setForm({ ...form, base_url: e.target.value })}
              />
            </div>

            {/* API Key */}
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

            {/* 模型列表 */}
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

            {/* 权重 / 优先级 */}
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
            <Button onClick={handleSave} disabled={saving || !form.name.trim() || !form.base_url.trim()}>
              {saving ? "保存中..." : "保存"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
