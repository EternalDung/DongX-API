import { useEffect, useMemo, useState, Fragment } from "react";
import { Plus, Pencil, Trash2, Zap, RefreshCw, Network, AlertTriangle, ChevronDown, ChevronRight, X } from "lucide-react";
import { cn } from "@/lib/utils";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusBadge, type StatusTone } from "@/components/ui/status-badge";
import { useCopyToClipboard } from "@/components/ui/copy-button";
import { useToast } from "@/components/ui/toast";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { channelApi, type ChannelInput, type ChannelStats } from "@/lib/api";
import type {
  Channel,
  ChannelProtocol,
  ChannelEndpoint,
  ChannelPreset,
  ChannelProtocolPresetGroup,
} from "@/types";
import { ProviderDropdown } from "@/components/channel-form/ProviderDropdown";

// ---------------------------------------------------------------------------
// Protocol-level UI constants. Vendor URLs / models / endpoints are NOT
// hard-coded here — they come from the backend preset registry
// (channelApi.presets()), consumed via the ProviderDropdown.
// ---------------------------------------------------------------------------

const PROTOCOLS: ChannelProtocol[] = ["openai", "anthropic", "ollama"];
const PROTOCOL_LABELS: Record<ChannelProtocol, string> = {
  openai: "OpenAI",
  anthropic: "Anthropic",
  ollama: "Ollama",
};

/** Selectable endpoints per protocol (Anthropic/Ollama are fixed). */
const PROTOCOL_ENDPOINT_OPTIONS: Record<ChannelProtocol, ChannelEndpoint[]> = {
  openai: ["chat_completions", "responses"],
  anthropic: ["messages"],
  ollama: ["api_chat"],
};

const ENDPOINT_LABELS: Record<string, string> = {
  chat_completions: "Chat Completions",
  responses: "Responses",
  messages: "Messages",
  api_chat: "/api/chat",
};
const ENDPOINT_PATHS: Record<string, string> = {
  chat_completions: "/chat/completions",
  responses: "/responses",
  messages: "/messages",
  api_chat: "/api/chat",
};

/** Default checked endpoints for the custom preset of each protocol. */
function defaultEndpointsFor(protocol: ChannelProtocol): ChannelEndpoint[] {
  switch (protocol) {
    case "openai":
      return ["chat_completions"];
    case "anthropic":
      return ["messages"];
    case "ollama":
      return ["api_chat"];
  }
}

const CHANNEL_TONE: Record<number, { tone: StatusTone; label: string }> = {
  1: { tone: "success", label: "启用" },
  2: { tone: "destructive", label: "异常" },
  0: { tone: "secondary", label: "禁用" },
};

interface KeyRow {
  key: string;
  weight: number;
}
interface MappingRow {
  from: string;
  to: string;
}

interface ChannelForm {
  protocol: ChannelProtocol;
  provider: string; // ChannelProvider enum value
  legacyType: string; // adapter type written back to the DB
  native_base_url: string;
  name: string;
  keys: KeyRow[];
  native_endpoints: ChannelEndpoint[];
  modelsText: string;
  mappings: MappingRow[];
  priority: number;
  weight: number;
  timeout_secs: number;
}

function emptyForm(): ChannelForm {
  return {
    protocol: "openai",
    provider: "custom",
    legacyType: "openai",
    native_base_url: "",
    name: "",
    keys: [{ key: "", weight: 1 }],
    native_endpoints: ["chat_completions"],
    modelsText: "",
    mappings: [{ from: "", to: "" }],
    priority: 0,
    weight: 1,
    timeout_secs: 30,
  };
}

function fmtTime(iso: string | null): string {
  if (!iso) return "";
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

// ---------------------------------------------------------------------------
// Page
// ---------------------------------------------------------------------------

export function ChannelsPage() {
  const toast = useToast();
  const [channels, setChannels] = useState<Channel[]>([]);
  const [loading, setLoading] = useState(true);

  const [dialogOpen, setDialogOpen] = useState(false);
  const [editingId, setEditingId] = useState<string | null>(null);
  const [form, setForm] = useState<ChannelForm>(emptyForm());
  const [saving, setSaving] = useState(false);
  const [modelInput, setModelInput] = useState("");
  const [syncOpen, setSyncOpen] = useState(false);
  const [syncLoading, setSyncLoading] = useState(false);
  const [syncList, setSyncList] = useState<string[]>([]);
  const [syncChecked, setSyncChecked] = useState<Set<string>>(new Set());
  const [syncQuery, setSyncQuery] = useState("");

  const [testingId, setTestingId] = useState<string | null>(null);
  const [testResults, setTestResults] = useState<Record<string, boolean>>({});

  const [deleteTarget, setDeleteTarget] = useState<Channel | null>(null);
  const [deleting, setDeleting] = useState(false);

  const [expandedId, setExpandedId] = useState<string | null>(null);

  // 展开渠道时拉取的运行概览（近 30 天）
  const [chanStats, setChanStats] = useState<ChannelStats | null>(null);
  const [chanStatsLoading, setChanStatsLoading] = useState(false);

  // ── preset registry (single source of truth for the picker) ──────────────
  const [presetGroups, setPresetGroups] = useState<ChannelProtocolPresetGroup[]>([]);
  const [presetsLoading, setPresetsLoading] = useState(true);
  useEffect(() => {
    channelApi
      .presets()
      .then(setPresetGroups)
      .catch(() => setPresetGroups([]))
      .finally(() => setPresetsLoading(false));
  }, []);

  const load = async () => {
    setLoading(true);
    try {
      const list = await channelApi.list();
      setChannels(list);
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

  // 跟随展开状态拉取该渠道近 30 天运行概览（成功率 / 平均延迟 / 总请求）
  useEffect(() => {
    if (!expandedId) {
      setChanStats(null);
      return;
    }
    const ch = channels.find((c) => c.id === expandedId);
    if (!ch) return;
    let cancelled = false;
    setChanStatsLoading(true);
    channelApi
      .stats(ch.name)
      .then((s) => {
        if (!cancelled) setChanStats(s);
      })
      .catch(() => {
        if (!cancelled) setChanStats(null);
      })
      .finally(() => {
        if (!cancelled) setChanStatsLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [expandedId, channels]);

  const openCreate = () => {
    setEditingId(null);
    setForm(emptyForm());
    setDialogOpen(true);
  };

  const openEdit = (ch: Channel) => {
    setEditingId(ch.id);
    const mappings: MappingRow[] = Object.entries(ch.model_mapping ?? {}).map(
      ([from, to]) => ({ from, to: to as string }),
    );
    const protocol = (PROTOCOLS.includes(ch.protocol) ? ch.protocol : "openai") as ChannelProtocol;
    setForm({
      protocol,
      // provider is restored via currentPreset (matched by legacy_type) for highlight
      provider: "custom",
      legacyType: ch.type,
      native_base_url: ch.base_url,
      name: ch.name,
      // Echo the decrypted upstream keys back (local gateway: not secrets).
      keys:
        ch.keys && ch.keys.length > 0
          ? ch.keys.map((k) => ({ key: k.key, weight: k.weight || 1 }))
          : [{ key: "", weight: 1 }],
      native_endpoints: (ch.endpoints?.length
        ? ch.endpoints
        : defaultEndpointsFor(protocol)) as ChannelEndpoint[],
      modelsText: (ch.models ?? []).join(", "),
      mappings: mappings.length ? mappings : [{ from: "", to: "" }],
      priority: ch.priority,
      weight: ch.weight,
      timeout_secs: (ch.config as Record<string, number>)?.timeout_secs ?? 30,
    });
    setDialogOpen(true);
  };

  // The preset currently reflected by the form (used for highlight + defaults).
  const currentPreset = useMemo<ChannelPreset | null>(() => {
    const group = presetGroups.find((g) => g.protocol === form.protocol);
    if (!group) return null;
    return (
      group.presets.find((p) => p.provider === form.provider) ??
      group.presets.find((p) => p.legacy_type === form.legacyType && p.provider !== "custom") ??
      group.presets[0] ??
      null
    );
  }, [presetGroups, form.protocol, form.provider, form.legacyType]);

  const authScheme = currentPreset?.auth_scheme ?? "bearer";
  const keyRequired = authScheme !== "optional_bearer";

  const requestProtocolSwitch = (protocol: ChannelProtocol) => {
    if (protocol === form.protocol) return;
    const group = presetGroups.find((g) => g.protocol === protocol);
    const custom = group?.presets.find((p) => p.provider === "custom");
    setForm((f) => ({
      ...f,
      protocol,
      provider: "custom",
      legacyType: custom?.legacy_type ?? (protocol === "anthropic" ? "claude" : "openai"),
      native_base_url: custom?.native_base_url ?? "",
      native_endpoints: defaultEndpointsFor(protocol),
      modelsText: "",
    }));
  };

  const selectProvider = (provider: string) => {
    if (provider === form.provider) return;
    const preset = presetGroups
      .find((g) => g.protocol === form.protocol)
      ?.presets.find((p) => p.provider === provider);
    if (!preset) return;
    setForm((f) => ({
      ...f,
      provider: preset.provider,
      legacyType: preset.legacy_type,
      native_base_url: preset.native_base_url,
      native_endpoints: [...preset.default_checked_endpoints],
      modelsText: preset.model_suggestions.map((m) => m.id).join(", "),
    }));
  };

  const updateKey = (i: number, field: keyof KeyRow, val: string | number) =>
    setForm((f) => ({
      ...f,
      keys: f.keys.map((k, idx) => (idx === i ? { ...k, [field]: val } : k)),
    }));
  const addKey = () => setForm((f) => ({ ...f, keys: [...f.keys, { key: "", weight: 1 }] }));
  const removeKey = (i: number) =>
    setForm((f) => {
      const next = f.keys.filter((_, idx) => idx !== i);
      // Always keep at least one (possibly empty) key row so the form stays valid.
      return { ...f, keys: next.length ? next : [{ key: "", weight: 1 }] };
    });

  const updateMapping = (i: number, field: keyof MappingRow, val: string) =>
    setForm((f) => ({
      ...f,
      mappings: f.mappings.map((m, idx) => (idx === i ? { ...m, [field]: val } : m)),
    }));
  const addMapping = () =>
    setForm((f) => ({ ...f, mappings: [...f.mappings, { from: "", to: "" }] }));
  const removeMapping = (i: number) =>
    setForm((f) => ({ ...f, mappings: f.mappings.filter((_, idx) => idx !== i) }));

  const toggleEndpoint = (ep: ChannelEndpoint) =>
    setForm((f) => ({
      ...f,
      native_endpoints: f.native_endpoints.includes(ep)
        ? f.native_endpoints.filter((e) => e !== ep)
        : [...f.native_endpoints, ep],
    }));

  const modelsList = form.modelsText
    .split(/[,\n]/)
    .map((s) => s.trim())
    .filter(Boolean);

  const filteredSync = syncList.filter((m) =>
    m.toLowerCase().includes(syncQuery.trim().toLowerCase()),
  );

  const addModelFromInput = () => {
    const parts = modelInput
      .split(/[,\n]/)
      .map((s) => s.trim())
      .filter(Boolean);
    if (parts.length === 0) return;
    setForm((f) => {
      const existing = new Set(
        f.modelsText
          .split(/[,\n]/)
          .map((s) => s.trim())
          .filter(Boolean),
      );
      const next = f.modelsText
        ? f.modelsText.split(/[,\n]/).map((s) => s.trim()).filter(Boolean)
        : [];
      for (const p of parts) if (!existing.has(p)) next.push(p);
      return { ...f, modelsText: next.join(", ") };
    });
    setModelInput("");
  };

  const removeModel = (m: string) =>
    setForm((f) => {
      const next = f.modelsText
        .split(/[,\n]/)
        .map((s) => s.trim())
        .filter(Boolean)
        .filter((x) => x !== m);
      return { ...f, modelsText: next.join(", ") };
    });

  const openSyncDialog = async () => {
    setSyncOpen(true);
    setSyncLoading(true);
    try {
      const firstKey = form.keys.find((k) => k.key.trim())?.key ?? "";
      const list = await channelApi.fetchModels({
        type: form.legacyType || form.protocol,
        base_url: form.native_base_url,
        api_key: firstKey,
      });
      setSyncList(list);
      const current = new Set(
        form.modelsText
          .split(/[,\n]/)
          .map((s) => s.trim())
          .filter(Boolean),
      );
      setSyncChecked(new Set(list.filter((m) => current.has(m))));
      setSyncQuery("");
    } catch (e) {
      const msg =
        (e && typeof e === "object" && "message" in e
          ? String((e as { message: unknown }).message)
          : String(e)) || "拉取模型失败";
      toast.error(msg);
    } finally {
      setSyncLoading(false);
    }
  };

  const confirmSync = () => {
    setForm((f) => {
      const manual = f.modelsText
        .split(/[,\n]/)
        .map((s) => s.trim())
        .filter(Boolean)
        .filter((m) => !syncList.includes(m));
      const next = [...manual, ...syncChecked];
      return { ...f, modelsText: next.join(", ") };
    });
    setSyncOpen(false);
  };

  const handleSave = async () => {
    if (!form.name.trim() || !form.native_base_url.trim()) return;
    setSaving(true);
    try {
      const input: ChannelInput = {
        name: form.name,
        protocol: form.protocol,
        type: currentPreset?.legacy_type ?? form.legacyType,
        base_url: form.native_base_url,
        keys: form.keys
          .filter((k) => k.key.trim())
          .map((k) => ({ key: k.key.trim(), weight: k.weight || 1 })),
        endpoints: form.native_endpoints,
        models: modelsList,
        priority: form.priority,
        weight: form.weight,
        config: {},
        model_mapping: Object.fromEntries(
          form.mappings
            .filter((m) => m.from.trim() && m.to.trim())
            .map((m) => [m.from.trim(), m.to.trim()]),
        ),
        timeout_secs: form.timeout_secs,
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

  const presetOptions = presetGroups.find((g) => g.protocol === form.protocol)?.presets ?? [];

  // 复制逻辑复用共享 hook（三处页面同一份实现：剪贴板 + toast + copied 态）。
  const { copy: copyText } = useCopyToClipboard();

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
          <Button
            size="sm"
            onClick={openCreate}
            className="border border-primary/30 font-semibold shadow-sm"
          >
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
                const memOk = testResults[ch.id];
                const persistedOk =
                  memOk !== undefined
                    ? memOk
                    : ch.last_test_ok === 1
                      ? true
                      : ch.last_test_ok === 0
                        ? false
                        : undefined;
                const expanded = expandedId === ch.id;
                return (
                  <Fragment key={ch.id}>
                    <div
                      className={cn(
                        "group flex items-center justify-between gap-3 rounded-lg px-2 py-3 transition-colors hover:bg-accent/40",
                        expanded && "bg-accent/40",
                      )}
                    >
                      <button
                        type="button"
                        onClick={() => setExpandedId(expanded ? null : ch.id)}
                        className="shrink-0 text-muted-foreground transition-colors hover:text-foreground"
                        aria-label={expanded ? "收起" : "展开"}
                      >
                        {expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
                      </button>
                      <div
                        className="min-w-0 flex-1 cursor-pointer"
                        onClick={() => setExpandedId(expanded ? null : ch.id)}
                      >
                        <div className="flex flex-wrap items-center gap-2">
                          <span className="font-medium">{ch.name}</span>
                          <Badge variant="outline" className="font-mono text-[11px]">
                            {ch.type}
                          </Badge>
                          <StatusBadge tone={meta.tone}>{meta.label}</StatusBadge>
                          {persistedOk !== undefined && (
                            <StatusBadge tone={persistedOk ? "success" : "destructive"}>
                              {persistedOk ? "连通" : "失败"}
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
                          size="icon"
                          title="删除渠道"
                          className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                          onClick={() => setDeleteTarget(ch)}
                        >
                          <Trash2 />
                        </Button>
                      </div>
                    </div>
                    {expanded && (
                      <div className="px-2 pb-4 pl-10">
                        <div className="grid gap-4 rounded-lg border bg-muted/30 p-4 text-sm sm:grid-cols-2">
                          {/* 模型列表 */}
                          <div className="grid gap-1.5">
                            <p className="text-xs font-medium text-muted-foreground">
                              模型列表（点击复制）
                            </p>
                            <div className="flex flex-wrap gap-1.5">
                              {ch.models.length === 0 ? (
                                <span className="text-muted-foreground">未配置</span>
                              ) : (
                                ch.models.map((m) => (
                                  <button
                                    key={m}
                                    type="button"
                                    onClick={() => copyText(m, "模型名")}
                                    className="rounded-md border bg-background px-2 py-1 font-mono text-[11px] hover:border-primary/50"
                                  >
                                    {m}
                                  </button>
                                ))
                              )}
                            </div>
                          </div>
                          {/* 模型映射 */}
                          <div className="grid gap-1.5">
                            <p className="text-xs font-medium text-muted-foreground">模型映射</p>
                            {Object.keys(ch.model_mapping ?? {}).length === 0 ? (
                              <span className="text-muted-foreground">无</span>
                            ) : (
                              <div className="flex flex-col gap-1">
                                {Object.entries(ch.model_mapping).map(([from, to]) => (
                                  <span key={from} className="font-mono text-[11px]">
                                    {from}{" "}
                                    <span className="text-muted-foreground">→</span> {to}
                                  </span>
                                ))}
                              </div>
                            )}
                          </div>
                          {/* 端点 */}
                          <div className="grid gap-1.5">
                            <p className="text-xs font-medium text-muted-foreground">端点</p>
                            <div className="flex flex-wrap gap-1.5">
                              {(ch.endpoints ?? []).map((ep) => (
                                <span
                                  key={ep}
                                  className="rounded-md border bg-background px-2 py-1 font-mono text-[11px]"
                                >
                                  {ENDPOINT_LABELS[ep] ?? ep}
                                </span>
                              ))}
                            </div>
                          </div>
                          {/* 最近测试 */}
                          <div className="grid gap-1.5">
                            <p className="text-xs font-medium text-muted-foreground">最近测试</p>
                            {ch.last_test_at ? (
                              <span className="text-[13px]">
                                {fmtTime(ch.last_test_at)} ·{" "}
                                <span
                                  className={
                                    ch.last_test_ok === 1
                                      ? "text-success"
                                      : ch.last_test_ok === 0
                                        ? "text-destructive"
                                        : "text-muted-foreground"
                                  }
                                >
                                  {ch.last_test_ok === 1
                                    ? "成功"
                                    : ch.last_test_ok === 0
                                      ? "失败"
                                      : "未知"}
                                </span>
                              </span>
                            ) : (
                              <span className="text-muted-foreground">尚未测试</span>
                            )}
                          </div>
                          {/* 运行概览（近 30 天） */}
                          <div className="grid gap-1.5">
                            <p className="text-xs font-medium text-muted-foreground">
                              运行概览（近 30 天）
                            </p>
                            {chanStatsLoading ? (
                              <span className="text-muted-foreground">加载中…</span>
                            ) : chanStats && chanStats.total > 0 ? (
                              <div className="flex flex-col gap-0.5 text-[13px]">
                                <span>
                                  成功率{" "}
                                  <span className="font-medium text-foreground">
                                    {chanStats.success_rate}%
                                  </span>{" "}
                                  · 平均延迟{" "}
                                  <span className="font-medium text-foreground">
                                    {chanStats.avg_latency_ms} ms
                                  </span>
                                </span>
                                <span className="text-xs text-muted-foreground">
                                  总请求 {chanStats.total} 次
                                </span>
                              </div>
                            ) : (
                              <span className="text-muted-foreground">暂无请求记录</span>
                            )}
                          </div>
                        </div>
                      </div>
                    )}
                  </Fragment>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>

      {/* 新建 / 编辑 Dialog */}
      <Dialog open={dialogOpen} onOpenChange={setDialogOpen}>
        <DialogContent className="sm:max-w-2xl max-h-[90vh] overflow-y-auto">
          <DialogHeader>
            <DialogTitle>{editingId ? "编辑渠道" : "添加渠道"}</DialogTitle>
            <DialogDescription>
              {editingId
                ? "修改渠道配置。密钥留空表示保持不变。"
                : "配置上游 LLM 供应商渠道。"}
            </DialogDescription>
          </DialogHeader>

          <div className="grid gap-5">
            {/* 协议（主动向三选一） */}
            <div className="grid gap-2">
              <Label>协议</Label>
              <div className="grid grid-cols-3 gap-2">
                {PROTOCOLS.map((p) => {
                  const active = form.protocol === p;
                  return (
                    <button
                      key={p}
                      type="button"
                      onClick={() => requestProtocolSwitch(p)}
                      className={cn(
                        "rounded-lg border px-3 py-2 text-center text-sm font-medium transition-colors",
                        active
                          ? "border-primary bg-primary/10 text-primary"
                          : "border-border text-muted-foreground hover:border-primary/50",
                      )}
                    >
                      {PROTOCOL_LABELS[p]}
                    </button>
                  );
                })}
              </div>
              <p className="text-xs text-muted-foreground">
                先选协议，再在下方选择该协议下的提供商。同一厂商可出现在多个协议下（如 DeepSeek 同时支持 OpenAI 与 Anthropic 接口）。
              </p>
            </div>

            {/* 名称 */}
            <div className="grid gap-2">
              <Label htmlFor="ch-name">名称</Label>
              <Input
                id="ch-name"
                placeholder="如：OpenAI 官方"
                value={form.name}
                onChange={(e) => setForm({ ...form, name: e.target.value })}
              />
            </div>

            {/* 渠道提供商（按协议过滤的分组下拉，真实品牌图标） */}
            <div className="grid gap-2">
              <Label>渠道提供商</Label>
              {presetsLoading ? (
                <div className="flex items-center gap-2 rounded-lg border border-dashed border-border bg-background/40 px-3 py-4 text-sm text-muted-foreground">
                  <RefreshCw size={14} className="animate-spin" /> 正在加载提供商模板…
                </div>
              ) : presetOptions.length === 0 ? (
                <div className="rounded-lg border border-red-200 bg-red-50 px-3 py-2 text-sm text-red-700">
                  提供商模板加载失败，请刷新后重试。
                </div>
              ) : (
                <ProviderDropdown
                  presets={presetOptions}
                  current={currentPreset?.provider ?? "custom"}
                  onSelect={selectProvider}
                />
              )}
            </div>

            {/* 端点（按协议多选；Anthropic / Ollama 端点固定） */}
            <div className="grid gap-2">
              <Label>
                端点<span className="ml-1 text-xs text-muted-foreground">（可多选）</span>
              </Label>
              <div className="flex flex-wrap gap-5">
                {PROTOCOL_ENDPOINT_OPTIONS[form.protocol].map((ep) => {
                  const checked = form.native_endpoints.includes(ep);
                  const fixed = form.protocol !== "openai";
                  return (
                    <label
                      key={ep}
                      className={cn(
                        "flex items-center gap-2 text-sm",
                        fixed && "opacity-70",
                      )}
                    >
                      <input
                        type="checkbox"
                        checked={checked}
                        disabled={fixed}
                        onChange={() => !fixed && toggleEndpoint(ep)}
                        className="h-4 w-4 accent-primary"
                      />
                      <span className="font-medium">{ENDPOINT_LABELS[ep]}</span>
                      <span className="font-mono text-xs text-muted-foreground">
                        {ENDPOINT_PATHS[ep]}
                      </span>
                    </label>
                  );
                })}
              </div>
              <p className="text-xs text-muted-foreground">
                Anthropic 固定为 Messages、Ollama 固定为 /api/chat；OpenAI 可同时启用 Chat Completions 与 Responses。
              </p>
            </div>

            {/* Base URL（由提供商带出，可覆盖） */}
            <div className="grid gap-2">
              <Label htmlFor="ch-base">Base URL</Label>
              <Input
                id="ch-base"
                placeholder="https://api.example.com"
                value={form.native_base_url}
                onChange={(e) => setForm({ ...form, native_base_url: e.target.value })}
                className="font-mono"
              />
              <p className="text-xs text-muted-foreground">
                选自提供商后自动带出，可按实际部署修改。
              </p>
            </div>

            {/* API 负载均衡（多 key + 权重） */}
            <div className="grid gap-2">
              <Label>
                API 负载均衡
                <span className="ml-1 text-xs text-muted-foreground">（多 key + 权重）</span>
              </Label>
              <div className="grid gap-2">
                {form.keys.map((k, i) => (
                  <div key={i} className="flex items-center gap-2">
                    <div className="flex shrink-0 items-center gap-1.5">
                      <span
                        className={cn(
                          "flex h-6 w-6 items-center justify-center rounded-full text-xs font-semibold",
                          i === 0
                            ? "bg-primary/15 text-primary"
                            : "bg-muted text-muted-foreground",
                        )}
                      >
                        {i + 1}
                      </span>
                      {i === 0 && (
                        <Badge className="bg-primary/15 text-primary hover:bg-primary/15">
                          主
                        </Badge>
                      )}
                    </div>
                    <Input
                      placeholder={keyRequired ? "sk-..." : "可留空（本地/自管 Ollama）"}
                      type="text"
                      value={k.key}
                      onChange={(e) => updateKey(i, "key", e.target.value)}
                      className="flex-1"
                    />
                    <Input
                      type="number"
                      min={1}
                      step={1}
                      value={k.weight}
                      onChange={(e) => updateKey(i, "weight", Number(e.target.value) || 1)}
                      className="w-20"
                    />
                    <span className="text-xs text-muted-foreground">权重</span>
                    <Button
                      variant="ghost"
                      size="icon"
                      onClick={() => removeKey(i)}
                      title="删除此密钥"
                      className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                    >
                      <Trash2 />
                    </Button>
                  </div>
                ))}
                <Button variant="ghost" size="sm" onClick={addKey} className="w-fit">
                  <Plus />
                  添加 key
                </Button>
              </div>
            </div>

            {/* 模型列表 */}
            <div className="grid gap-2">
              <Label>
                模型列表
                <span className="ml-1 text-xs text-muted-foreground">（回车新增 / 可删除）</span>
              </Label>
              <div className="flex gap-2">
                <Input
                  placeholder="输入模型名后回车新增，如 gpt-4o"
                  value={modelInput}
                  onChange={(e) => setModelInput(e.target.value)}
                  onKeyDown={(e) => {
                    if (e.key === "Enter" || e.key === ",") {
                      e.preventDefault();
                      addModelFromInput();
                    }
                  }}
                  className="flex-1"
                />
                <Button
                  variant="outline"
                  size="sm"
                  onClick={openSyncDialog}
                  disabled={syncLoading}
                >
                  <RefreshCw className={syncLoading ? "animate-spin" : ""} />
                  同步上游模型
                </Button>
              </div>
              <div className="flex flex-wrap gap-2">
                {modelsList.length === 0 ? (
                  <span className="text-xs text-muted-foreground">尚未添加模型</span>
                ) : (
                  modelsList.map((m) => (
                    <span
                      key={m}
                      className="flex items-center gap-1 rounded-md border bg-background px-2 py-1 font-mono text-[11px]"
                    >
                      {m}
                      <button
                        type="button"
                        onClick={() => removeModel(m)}
                        className="text-muted-foreground transition-colors hover:text-destructive"
                        aria-label={`删除 ${m}`}
                      >
                        <X size={12} />
                      </button>
                    </span>
                  ))
                )}
              </div>
            </div>

            {/* 模型映射（本地 → 上游，多对多） */}
            <div className="grid gap-2">
              <Label>
                模型映射
                <span className="ml-1 text-xs text-muted-foreground">
                  （本地模型 → 上游模型，多对多）
                </span>
              </Label>
              <div className="grid gap-2">
                {form.mappings.map((m, i) => (
                  <div key={i} className="flex items-center gap-2">
                    <Input
                      placeholder="本地模型名"
                      value={m.from}
                      onChange={(e) => updateMapping(i, "from", e.target.value)}
                      className="flex-1"
                    />
                    <span className="text-muted-foreground">→</span>
                    <select
                      value={m.to}
                      onChange={(e) => updateMapping(i, "to", e.target.value)}
                      disabled={!modelsList.length}
                      className="flex-1 rounded-lg border border-border bg-background px-3 py-2 text-sm outline-none focus:border-primary disabled:opacity-50"
                    >
                      <option value="">
                        {modelsList.length ? "选择上游模型" : "先填写模型列表"}
                      </option>
                      {modelsList.map((md) => (
                        <option key={md} value={md}>
                          {md}
                        </option>
                      ))}
                    </select>
                    {form.mappings.length > 1 && (
                      <Button variant="ghost" size="sm" onClick={() => removeMapping(i)}>
                        <Trash2 />
                      </Button>
                    )}
                  </div>
                ))}
                <Button variant="ghost" size="sm" onClick={addMapping} className="w-fit">
                  <Plus />
                  添加映射
                </Button>
              </div>
            </div>

            {/* 优先级 / 权重 / 超时 */}
            <div className="grid grid-cols-3 gap-4">
              <div className="grid gap-2">
                <Label htmlFor="ch-priority">优先级</Label>
                <Input
                  id="ch-priority"
                  type="number"
                  value={form.priority}
                  onChange={(e) =>
                    setForm({ ...form, priority: Number(e.target.value) || 0 })
                  }
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="ch-weight">权重</Label>
                <Input
                  id="ch-weight"
                  type="number"
                  min={1}
                  value={form.weight}
                  onChange={(e) =>
                    setForm({ ...form, weight: Number(e.target.value) || 1 })
                  }
                />
              </div>
              <div className="grid gap-2">
                <Label htmlFor="ch-timeout">超时（秒）</Label>
                <Input
                  id="ch-timeout"
                  type="number"
                  min={1}
                  value={form.timeout_secs}
                  onChange={(e) =>
                    setForm({ ...form, timeout_secs: Number(e.target.value) || 30 })
                  }
                />
              </div>
            </div>

            <p className="text-xs leading-relaxed text-muted-foreground">
              优先级高的渠道优先被选中；同优先级内按权重加权随机分发（权重越大命中概率越高）。
              超时仅对<span className="font-medium text-foreground">非流式（一次性）</span>请求生效，限制完整响应返回的总时长；
              流式请求仅限制<span className="font-medium text-foreground">连接建立时间</span>（TCP/TLS 握手），不限制整条流时长，长对话可放心调大。
            </p>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setDialogOpen(false)}>
              取消
            </Button>
            <Button
              onClick={handleSave}
              disabled={saving || !form.name.trim() || !form.native_base_url.trim()}
            >
              {saving ? "保存中..." : "保存"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 同步上游模型 Dialog */}
      <Dialog open={syncOpen} onOpenChange={setSyncOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>同步上游模型</DialogTitle>
            <DialogDescription>
              勾选要加入模型列表的上游模型，已添加的项默认选中。确认后将与手动输入的模型合并。
            </DialogDescription>
          </DialogHeader>
          <div className="grid gap-3">
            <Input
              placeholder="搜索模型…"
              value={syncQuery}
              onChange={(e) => setSyncQuery(e.target.value)}
            />
            <label className="flex items-center gap-2 text-sm font-medium">
              <input
                type="checkbox"
                checked={
                  syncList.length > 0 &&
                  filteredSync.length > 0 &&
                  filteredSync.every((m) => syncChecked.has(m))
                }
                onChange={(e) => {
                  const next = new Set(syncChecked);
                  if (e.target.checked) {
                    for (const m of filteredSync) next.add(m);
                  } else {
                    for (const m of filteredSync) next.delete(m);
                  }
                  setSyncChecked(next);
                }}
                className="h-4 w-4 accent-primary"
              />
              全选（当前匹配 {filteredSync.length} 项）
            </label>
            <div className="max-h-64 overflow-y-auto rounded-lg border">
              {syncLoading ? (
                <div className="flex items-center gap-2 px-3 py-6 text-sm text-muted-foreground">
                  <RefreshCw size={14} className="animate-spin" /> 正在拉取模型…
                </div>
              ) : syncList.length === 0 ? (
                <div className="px-3 py-6 text-center text-sm text-muted-foreground">
                  无可用模型，请检查 Base URL 与密钥。
                </div>
              ) : (
                <div className="divide-y">
                  {filteredSync.map((m) => {
                    const checked = syncChecked.has(m);
                    const already = modelsList.includes(m);
                    return (
                      <label
                        key={m}
                        className="flex items-center gap-2 px-3 py-2 text-sm"
                      >
                        <input
                          type="checkbox"
                          checked={checked}
                          onChange={() => {
                            const next = new Set(syncChecked);
                            if (checked) next.delete(m);
                            else next.add(m);
                            setSyncChecked(next);
                          }}
                          className="h-4 w-4 accent-primary"
                        />
                        <span className="flex-1 font-mono text-[12px]">{m}</span>
                        {already && (
                          <Badge variant="outline" className="text-[10px]">
                            已添加
                          </Badge>
                        )}
                      </label>
                    );
                  })}
                </div>
              )}
            </div>
            <p className="text-xs text-muted-foreground">
              已选 {syncChecked.size} / {syncList.length}
            </p>
          </div>
          <DialogFooter>
            <Button variant="outline" onClick={() => setSyncOpen(false)}>
              取消
            </Button>
            <Button onClick={confirmSync} disabled={syncLoading}>
              确认同步
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
