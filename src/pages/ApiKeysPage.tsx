import { useEffect, useState } from "react";
import {
  Plus,
  Trash2,
  Copy,
  Check,
  KeyRound,
  RefreshCw,
  CheckCircle2,
} from "lucide-react";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Badge } from "@/components/ui/badge";
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

export function ApiKeysPage() {
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [loading, setLoading] = useState(true);

  // 创建 Dialog
  const [createOpen, setCreateOpen] = useState(false);
  const [name, setName] = useState("");
  const [quota, setQuota] = useState("0"); // 0 = 不限
  const [creating, setCreating] = useState(false);

  // 明文展示 Dialog（创建成功后一次性展示）
  const [plainKey, setPlainKey] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);

  const load = async () => {
    setLoading(true);
    try {
      setKeys(await keyApi.list());
    } catch (e) {
      console.error("Failed to load api keys:", e);
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
        expires_at: null,
      });
      setCreateOpen(false);
      setPlainKey(result.key);
      setName("");
      setQuota("0");
      await load();
    } catch (e) {
      console.error("Failed to create api key:", e);
    } finally {
      setCreating(false);
    }
  };

  const handleDelete = async (id: string, keyName: string) => {
    if (!window.confirm(`确认删除密钥「${keyName}」？使用该密钥的客户端将立即失去访问权限。`))
      return;
    try {
      await keyApi.remove(id);
      await load();
    } catch (e) {
      console.error("Failed to delete api key:", e);
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
    }
  };

  return (
    <div className="p-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">密钥管理</h1>
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
            <div className="py-12 text-center text-sm text-muted-foreground">
              加载中...
            </div>
          ) : keys.length === 0 ? (
            <div className="flex flex-col items-center gap-2 py-12 text-center">
              <KeyRound className="h-10 w-10 text-muted-foreground" />
              <p className="text-sm text-muted-foreground">暂无密钥</p>
              <p className="text-xs text-muted-foreground">
                创建 sk-dongapi-* 密钥后，客户端即可通过网关调用 LLM
              </p>
            </div>
          ) : (
            <div className="divide-y">
              {keys.map((k) => {
                const quotaPct =
                  k.quota_limit > 0 ? Math.min(100, (k.quota_used / k.quota_limit) * 100) : 0;
                return (
                  <div key={k.id} className="flex items-center justify-between gap-4 py-3">
                    <div className="min-w-0 flex-1">
                      <div className="flex items-center gap-2">
                        <span className="font-medium">{k.name}</span>
                        {k.status === 1 ? (
                          <Badge variant="success">启用</Badge>
                        ) : (
                          <Badge variant="secondary">
                            {k.status === 2 ? "过期" : "禁用"}
                          </Badge>
                        )}
                      </div>
                      <p className="mt-0.5 font-mono text-xs text-muted-foreground">{k.key}</p>
                      {/* 配额进度条 */}
                      <div className="mt-1.5 flex items-center gap-2">
                        <div className="h-1.5 w-40 overflow-hidden rounded-full bg-muted">
                          <div
                            className={`h-full rounded-full ${
                              quotaPct >= 90
                                ? "bg-destructive"
                                : quotaPct >= 70
                                  ? "bg-amber-500"
                                  : "bg-emerald-500"
                            }`}
                            style={{ width: `${k.quota_limit > 0 ? quotaPct : 0}%` }}
                          />
                        </div>
                        <span className="text-xs text-muted-foreground">
                          {formatQuota(k.quota_used, k.quota_limit)} tokens
                        </span>
                      </div>
                    </div>
                    <div className="flex shrink-0 items-center gap-1">
                      <Button
                        variant="ghost"
                        size="sm"
                        className="text-destructive hover:text-destructive"
                        onClick={() => handleDelete(k.id, k.name)}
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
              生成 sk-dongapi-* 格式密钥，明文仅展示一次，请妥善保存。
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
              <CheckCircle2 className="h-5 w-5 text-emerald-500" />
              密钥创建成功
            </DialogTitle>
            <DialogDescription>
              请立即复制保存，关闭后无法再次查看明文。
            </DialogDescription>
          </DialogHeader>

          <div className="flex items-center gap-2 rounded-md border bg-muted/50 p-3">
            <code className="flex-1 break-all font-mono text-sm">{plainKey}</code>
            <Button variant="outline" size="sm" onClick={handleCopy}>
              {copied ? <Check className="text-emerald-500" /> : <Copy />}
              {copied ? "已复制" : "复制"}
            </Button>
          </div>

          <DialogFooter>
            <Button onClick={() => setPlainKey(null)}>我已保存，关闭</Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
