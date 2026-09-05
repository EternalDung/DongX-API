import { useEffect, useState } from "react";
import {
  Plus,
  Trash2,
  Copy,
  Check,
  KeyRound,
  RefreshCw,
  CheckCircle2,
  Ban,
  AlertTriangle,
  Activity,
} from "lucide-react";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusBadge, type StatusTone } from "@/components/ui/status-badge";
import { useToast } from "@/components/ui/toast";
import { ExpiryPicker } from "@/components/ExpiryPicker";
import { CallStatsCard, type CallStatsData } from "@/components/CallStatsCard";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { keyApi } from "@/lib/api";
import type { ApiKey } from "@/types";

function formatQuota(used: number, limit: number): string {
  if (limit <= 0) return `${used.toLocaleString()} / ∞`;
  return `${used.toLocaleString()} / ${limit.toLocaleString()}`;
}

const KEY_TONE: Record<number, { tone: StatusTone; label: string }> = {
  1: { tone: "success", label: "启用" },
  2: { tone: "warning", label: "过期" },
  0: { tone: "secondary", label: "禁用" },
};

export function ApiKeysPage() {
  const toast = useToast();
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [loading, setLoading] = useState(true);

  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [quota, setQuota] = useState("0");
  const [expiresAt, setExpiresAt] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);

  const [plainKey, setPlainKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const [deleteTarget, setDeleteTarget] = useState<ApiKey | null>(null);
  const [deleting, setDeleting] = useState(false);

  // 「统计」Dialog：按需拉取该密钥的运行概览。
  const [statsTarget, setStatsTarget] = useState<ApiKey | null>(null);
  const [statsData, setStatsData] = useState<CallStatsData | null>(null);
  const [statsLoading, setStatsLoading] = useState(false);

  const openStats = async (k: ApiKey) => {
    setStatsTarget(k);
    setStatsData(null);
    setStatsLoading(true);
    try {
      setStatsData(await keyApi.stats(k.name));
    } catch (e) {
      console.error("Failed to load api key stats:", e);
      toast.error("统计加载失败");
    } finally {
      setStatsLoading(false);
    }
  };

  const load = async () => {
    setLoading(true);
    try {
      setKeys(await keyApi.list());
    } catch (e) {
      console.error("Failed to load api keys:", e);
      toast.error("密钥列表加载失败");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const handleCreate = async () => {
    if (!name.trim()) return;
    setCreating(true);
    try {
      const result = await keyApi.create({
        name: name.trim(),
        allowed_models: [],
        allowed_channels: [],
        quota_limit: Number(quota) || 0,
        expires_at: expiresAt,
      });
      setCreateOpen(false);
      setPlainKey(result.key);
      setName("");
      setQuota("0");
      setExpiresAt(null);
      toast.success("密钥已生成");
      await load();
    } catch (e) {
      console.error("Failed to create api key:", e);
      toast.error("密钥生成失败");
    } finally {
      setCreating(false);
    }
  };

  const handleConfirmDelete = async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      await keyApi.remove(deleteTarget.id);
      toast.success(`已删除密钥「${deleteTarget.name}」`);
      setDeleteTarget(null);
      await load();
    } catch (e) {
      console.error("Failed to delete api key:", e);
      toast.error("删除失败");
    } finally {
      setDeleting(false);
    }
  };

  const handleCopy = async () => {
    if (!plainKey) return;
    try {
      await navigator.clipboard.writeText(plainKey);
      setCopied(true);
      setTimeout(() => setCopied(false), 2000);
    } catch {
      console.error("Copy failed");
      toast.error("复制失败");
    }
  };

  const handleCopyKey = async (key: string) => {
    try {
      await navigator.clipboard.writeText(key);
      toast.success("已复制密钥");
    } catch {
      toast.error("复制失败");
    }
  };

  const handleSetStatus = async (k: ApiKey, status: number) => {
    try {
      await keyApi.setStatus(k.id, status);
      toast.success(status === 1 ? `已启用「${k.name}」` : `已禁用「${k.name}」`);
      await load();
    } catch (e) {
      console.error("Failed to set api key status:", e);
      toast.error("操作失败");
    }
  };

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">密钥管理</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            网关密钥管理：创建 sk-dongapi-* 密钥、配额限制、绑定渠道
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={load} disabled={loading}>
            <RefreshCw className={loading ? "animate-spin" : ""} />
            刷新
          </Button>
          <Button size="sm" onClick={() => setCreateOpen(true)}>
            <Plus />
            创建密钥
          </Button>
        </div>
      </div>

      <Card className="mt-6">
        <CardContent className="pt-2">
          {loading ? (
            <div className="space-y-2 py-4">
              {Array.from({ length: 3 }).map((_, i) => (
                <Skeleton key={i} className="h-16 w-full" />
              ))}
            </div>
          ) : keys.length === 0 ? (
            <EmptyState
              icon={KeyRound}
              title="暂无密钥"
              description="创建 sk-dongapi-* 密钥后，客户端即可通过网关以 OpenAI 兼容协议调用 LLM。"
              action={
                <Button size="sm" variant="outline" onClick={() => setCreateOpen(true)}>
                  <Plus />
                  创建第一个密钥
                </Button>
              }
            />
          ) : (
            <div className="divide-y">
              {keys.map((k) => {
                const meta = KEY_TONE[k.status] ?? KEY_TONE[0];
                const quotaPct =
                  k.quota_limit > 0 ? Math.min(100, (k.quota_used / k.quota_limit) * 100) : 0;
                const barColor =
                  quotaPct >= 90
                    ? "bg-destructive"
                    : quotaPct >= 70
                      ? "bg-warning"
                      : "bg-success";
                return (
                  <div
                    key={k.id}
                    className="group flex items-center justify-between gap-4 rounded-lg px-2 py-3 transition-colors hover:bg-accent/40"
                  >
                    <div className="min-w-0 flex-1">
                      <div className="flex flex-wrap items-center gap-2">
                        <span className="font-medium">{k.name}</span>
                        <StatusBadge tone={meta.tone}>{meta.label}</StatusBadge>
                      </div>
                      <p className="mt-1 truncate font-mono text-xs text-muted-foreground">
                        {k.key}
                      </p>
                      <div className="mt-2 flex items-center gap-2">
                        <div className="h-1.5 w-40 overflow-hidden rounded-full bg-muted">
                          <div
                            className={`h-full rounded-full transition-all ${barColor}`}
                            style={{ width: `${k.quota_limit > 0 ? quotaPct : 0}%` }}
                          />
                        </div>
                        <span className="font-mono text-xs tabular-nums text-muted-foreground">
                          {formatQuota(k.quota_used, k.quota_limit)} tokens
                        </span>
                      </div>
                    </div>
                    <div className="flex shrink-0 items-center gap-1 opacity-70 transition-opacity group-hover:opacity-100">
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => openStats(k)}
                        title="查看运行统计"
                      >
                        <Activity />
                        统计
                      </Button>
                      <Button
                        variant="ghost"
                        size="sm"
                        onClick={() => handleCopyKey(k.key)}
                        title="复制密钥明文"
                      >
                        <Copy />
                        复制
                      </Button>
                      {k.status === 1 ? (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => handleSetStatus(k, 0)}
                          title="禁用该密钥"
                        >
                          <Ban />
                          禁用
                        </Button>
                      ) : (
                        <Button
                          variant="ghost"
                          size="sm"
                          onClick={() => handleSetStatus(k, 1)}
                          title="启用该密钥"
                        >
                          <CheckCircle2 />
                          启用
                        </Button>
                      )}
                      <Button
                        variant="ghost"
                        size="sm"
                        className="text-destructive hover:bg-destructive/10 hover:text-destructive"
                        onClick={() => setDeleteTarget(k)}
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

      {/* 创建密钥 Dialog */}
      <Dialog open={createOpen} onOpenChange={setCreateOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle>创建密钥</DialogTitle>
            <DialogDescription>
              生成 sk-dongapi-* 格式密钥。本地以明文存储，创建后可在列表中随时复制。
            </DialogDescription>
          </DialogHeader>

          <div className="grid gap-4">
            <div className="grid gap-2">
              <Label htmlFor="key-name">密钥名称</Label>
              <Input
                id="key-name"
                placeholder="如：Cursor / Claude Code / 自研应用"
                value={name}
                onChange={(e) => setName(e.target.value)}
              />
            </div>
            <div className="grid gap-2">
              <Label htmlFor="key-quota">
                配额限制（Token 数，0 = 不限）
              </Label>
              <Input
                id="key-quota"
                type="number"
                min={0}
                placeholder="0"
                value={quota}
                onChange={(e) => setQuota(e.target.value)}
              />
              <p className="text-xs text-muted-foreground">
                超出配额后密钥自动禁用，累计 usage.total_tokens
              </p>
            </div>
            <div className="grid gap-2">
              <Label>过期时间</Label>
              <ExpiryPicker value={expiresAt} onChange={setExpiresAt} />
              <p className="text-xs text-muted-foreground">
                留空为永久有效；也可从快捷项选择 24 小时 / 30 天 / 180 天 / 1 年。
              </p>
            </div>
          </div>

          <DialogFooter>
            <Button variant="outline" onClick={() => setCreateOpen(false)}>
              取消
            </Button>
            <Button onClick={handleCreate} disabled={creating || !name.trim()}>
              {creating ? "生成中..." : "生成密钥"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 明文密钥一次性展示 Dialog */}
      <Dialog open={plainKey !== null} onOpenChange={(open) => !open && setPlainKey(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <CheckCircle2 className="h-5 w-5 text-success" />
              密钥创建成功
            </DialogTitle>
            <DialogDescription>
              请立即复制保存。密钥以明文存储在本地，也可稍后在列表中复制。
            </DialogDescription>
          </DialogHeader>

          <div className="flex items-center gap-2 rounded-lg border bg-muted/40 p-3">
            <code className="flex-1 break-all font-mono text-sm">{plainKey}</code>
            <Button variant="outline" size="sm" onClick={handleCopy}>
              {copied ? <Check className="text-success" /> : <Copy />}
              {copied ? "已复制" : "复制"}
            </Button>
          </div>

          <DialogFooter>
            <Button onClick={() => setPlainKey(null)}>我已保存，关闭</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>

      {/* 删除确认 Dialog */}
      <Dialog open={deleteTarget !== null} onOpenChange={(o) => !o && setDeleteTarget(null)}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              删除密钥
            </DialogTitle>
            <DialogDescription>
              确认删除密钥「{deleteTarget?.name}」？使用该密钥的客户端将立即失去访问权限。
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

      {/* 运行统计 Dialog */}
      <Dialog
        open={statsTarget !== null}
        onOpenChange={(o) => {
          if (!o) {
            setStatsTarget(null);
            setStatsData(null);
          }
        }}
      >
        <DialogContent className="sm:max-w-2xl">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <Activity className="h-5 w-5 text-muted-foreground" />
              密钥「{statsTarget?.name}」运行统计
            </DialogTitle>
            <DialogDescription>近 30 天该密钥的调用情况</DialogDescription>
          </DialogHeader>
          {statsLoading ? (
            <div className="rounded-lg border bg-card p-4 text-sm text-muted-foreground">
              加载中…
            </div>
          ) : (
            <CallStatsCard
              stats={
                statsData ?? {
                  total: 0,
                  successes: 0,
                  success_rate: 0,
                  avg_latency_ms: 0,
                  prompt_tokens_sum: 0,
                  completion_tokens_sum: 0,
                  last_called_at: null,
                }
              }
            />
          )}
        </DialogContent>
      </Dialog>
    </div>
  );
}
