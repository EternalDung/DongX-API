import { useEffect, useMemo, useState } from "react";
import {
  Bot,
  Boxes,
  CheckCircle2,
  CircleAlert,
  Code2,
  Copy,
  FolderOpen,
  Plug,
  RefreshCw,
  RotateCcw,
  Terminal,
  Wrench,
} from "lucide-react";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { useToast } from "@/components/ui/toast";
import { clientConfigApi } from "@/lib/api";
import { openPath, revealItemInDir } from "@tauri-apps/plugin-opener";
import { cn } from "@/lib/utils";
import { CodeBlock } from "@/components/CodeBlock";
import type { ApiKey, ClientInfo, ConfigContent } from "@/types";

// 客户端 → lucide 图标映射（品牌图标后续可替换）
const CLIENT_ICONS: Record<string, typeof Terminal> = {
  "claude-code": Terminal,
  codex: Code2,
  opencode: Wrench,
  openclaw: Bot,
  hermes: Boxes,
};

/** 下拉框里以「别名」为主、密钥掩码显示，避免长明文把名称挤没了 */
function maskKeyForDisplay(full: string): string {
  const m = full.match(/^(sk-dongapi-)(.{4}).*(.{4})$/);
  if (m) return `${m[1]}${m[2]}••••${m[3]}`;
  return full.length > 16 ? `${full.slice(0, 10)}••••${full.slice(-4)}` : full;
}

/** 密钥状态角标：正常 / 已禁用 / 剩余 X% / 已用尽（纯前端，数据来自 ApiKey 现有字段） */
function keyStatusInfo(k: ApiKey): { label: string; color: string } {
  if (k.status !== 1) return { label: "已禁用", color: "#A32D2D" };
  if (k.quota_limit > 0) {
    const remain = Math.max(0, k.quota_limit - k.quota_used);
    const pct = Math.round((remain / k.quota_limit) * 100);
    if (pct <= 0) return { label: "已用尽", color: "#A32D2D" };
    return { label: `剩余 ${pct}%`, color: "#BA7517" };
  }
  return { label: "正常", color: "#3B6D11" };
}

/** 取配置文件所在目录（跨平台：兼容 / 与 \\ 分隔符） */
function dirnameOf(p: string): string {
  const i = Math.max(p.lastIndexOf("/"), p.lastIndexOf("\\"));
  return i > 0 ? p.slice(0, i) : p;
}

interface Props {
  client: ClientInfo;
  /** 网关地址（运行态 http://127.0.0.1:port/v1），只读展示 */
  gatewayUrl: string;
  keys: ApiKey[];
  modelOptions: string[];
  /** 刷新后由父组件重新拉取安装/接入状态 */
  onRefresh?: () => void;
}

export function ClientConfigView({
  client,
  gatewayUrl,
  keys,
  modelOptions,
  onRefresh,
}: Props) {
  const toast = useToast();

  const [content, setContent] = useState<ConfigContent | null>(null);
  const [loadingContent, setLoadingContent] = useState(false);
  const [refreshing, setRefreshing] = useState(false);
  const [applying, setApplying] = useState(false);

  const [selectedKeyId, setSelectedKeyId] = useState("");
  const [model, setModel] = useState("");

  const Icon = CLIENT_ICONS[client.name] ?? Code2;
  const availableKeys = useMemo(() => keys.filter((k) => k.status === 1), [keys]);
  const selectedKey = useMemo(
    () => availableKeys.find((k) => k.id === selectedKeyId),
    [availableKeys, selectedKeyId],
  );
  const selectedKeyStatus = useMemo(
    () => (selectedKey ? keyStatusInfo(selectedKey) : null),
    [selectedKey],
  );

  // 切换客户端时重新加载配置文件内容
  const loadContent = async () => {
    setLoadingContent(true);
    try {
      const c = await clientConfigApi.content(client.name);
      setContent(c);
    } catch {
      setContent({ exists: false, content: "", error: "读取失败" });
    } finally {
      setLoadingContent(false);
    }
  };

  useEffect(() => {
    let cancelled = false;
    setContent(null);
    loadContent().finally(() => {
      if (cancelled) return;
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [client.name]);

  // 默认选中第一个密钥 / 模型
  useEffect(() => {
    if (!selectedKeyId && availableKeys.length > 0) {
      setSelectedKeyId(availableKeys[0].id);
    }
  }, [availableKeys, selectedKeyId]);

  useEffect(() => {
    if (!model && modelOptions.length > 0) setModel(modelOptions[0]);
  }, [modelOptions, model]);

  // 右上角刷新：重新拉取本页内容，并通知父组件刷新安装/接入状态
  // （用于「打开页面后才安装客户端」的场景）
  const handleRefresh = async () => {
    setRefreshing(true);
    try {
      await loadContent();
      onRefresh?.();
    } finally {
      setRefreshing(false);
    }
  };

  const copy = async (text: string, what: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(`已复制${what}`);
    } catch {
      toast.error("复制失败");
    }
  };

  // 打开配置文件所在目录（已装的 tauri-plugin-opener，无需新后端命令）
  const handleOpenDir = async () => {
    if (!client.config_path) {
      toast.error("无法确定配置路径，请刷新后重试");
      return;
    }
    const dir = dirnameOf(client.config_path);
    try {
      // 优先：在资源管理器中定位到该配置文件（打开所在目录并高亮 config.toml）
      await revealItemInDir(client.config_path);
    } catch {
      try {
        // 兜底：配置文件尚不存在时，直接打开所在目录
        await openPath(dir);
      } catch {
        toast.error(`无法打开目录，请手动定位：${dir}`);
      }
    }
  };

  const handleApply = async () => {
    if (!selectedKey || !model) {
      toast.error("请先选择密钥与模型");
      return;
    }
    setApplying(true);
    try {
      const res = await clientConfigApi.apply(
        client.name,
        selectedKey.key,
        model,
      );
      if (res.success) {
        toast.success("配置已写入，重启客户端即可生效");
        await loadContent();
        onRefresh?.();
      } else {
        toast.error(res.message || "写入失败");
      }
    } catch (e: any) {
      toast.error(e?.message || String(e));
    } finally {
      setApplying(false);
    }
  };

  const handleRestore = async () => {
    try {
      const res = await clientConfigApi.restore(client.name);
      if (res.success) {
        toast.success(res.message || "已恢复原始配置");
        await loadContent();
        onRefresh?.();
      } else {
        toast.error(res.message || "恢复失败");
      }
    } catch (e: any) {
      toast.error(e?.message || String(e));
    }
  };

  const canWrite = !!selectedKey && !!model;

  return (
    <div className="space-y-4">
      {/* ── 头部：图标 + 名称 + 安装/接入徽标 + 刷新按钮 ── */}
      <Card>
        <CardContent className="flex items-start justify-between gap-4 py-5">
          <div className="flex items-center gap-3">
            <div className="flex h-11 w-11 items-center justify-center rounded-xl bg-primary/10 text-primary ring-1 ring-inset ring-primary/15">
              <Icon className="h-6 w-6" />
            </div>
            <div>
              <div className="flex items-center gap-2">
                <h2 className="text-lg font-semibold tracking-tight">
                  {client.label}
                </h2>
                {client.available ? (
                  <Badge variant="success" className="text-[11px]">
                    已安装
                  </Badge>
                ) : (
                  <Badge variant="secondary" className="text-[11px]">
                    未安装
                  </Badge>
                )}
                {client.applied && (
                  <Badge variant="secondary" className="text-[11px]">
                    已接入
                  </Badge>
                )}
              </div>
              <p className="mt-0.5 text-sm text-muted-foreground">
                {client.description}
              </p>
            </div>
          </div>

          {/* 右上角刷新：用于「打开页面后才安装」的场景 */}
          <Button
            variant="outline"
            size="sm"
            onClick={handleRefresh}
            disabled={refreshing || loadingContent}
            className="h-8 gap-1.5"
            title="刷新配置内容"
          >
            <RefreshCw
              className={cn("h-3.5 w-3.5", refreshing && "animate-spin")}
            />
            刷新
          </Button>
        </CardContent>
      </Card>

      {/* ── 接入信息 + 密钥/模型 ── */}
      <Card>
        <CardHeader className="pb-3">
          <CardTitle className="text-base">接入信息</CardTitle>
        </CardHeader>
        <CardContent className="space-y-4">
          {/* 网关地址（只读文本，不可修改） */}
          <div className="space-y-1.5">
            <div className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
              <Plug className="h-3.5 w-3.5" />
              网关地址
            </div>
            <div className="flex items-center gap-1">
              <code className="flex-1 truncate rounded-md border bg-muted/50 px-3 py-2 font-mono text-xs text-foreground/80">
                {gatewayUrl}
              </code>
              <Button
                variant="outline"
                size="icon"
                onClick={() => copy(gatewayUrl, "网关地址")}
                className="h-9 w-9 shrink-0"
                title="复制网关地址"
              >
                <Copy className="h-3.5 w-3.5" />
              </Button>
            </div>
          </div>

          {/* API KEY + MODEL 同一行（窄屏自动堆叠） */}
          <div className="grid grid-cols-1 gap-4 sm:grid-cols-2">
            {/* API KEY（下拉，主要显示别名 + 状态角标） */}
            <div className="space-y-1.5">
              <div className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
                <span className="font-mono text-[10px]">SK</span>
                API KEY
              </div>
              {availableKeys.length === 0 ? (
                <div className="rounded-md border border-dashed px-3 py-2 text-xs text-muted-foreground">
                  暂无可用密钥，请先到「密钥管理」创建
                </div>
              ) : (
                <div className="flex items-center gap-2">
                  <Select
                    value={selectedKeyId}
                    onChange={(e) => setSelectedKeyId(e.target.value)}
                    className="font-mono text-xs"
                  >
                    {availableKeys.map((k) => (
                      <option key={k.id} value={k.id}>
                        {k.name}（{maskKeyForDisplay(k.key)}） ·{" "}
                        {keyStatusInfo(k).label}
                      </option>
                    ))}
                  </Select>
                  {selectedKeyStatus && (
                    <span
                      className="flex shrink-0 items-center gap-1.5 text-[11px]"
                      style={{ color: selectedKeyStatus.color }}
                    >
                      <span
                        className="inline-block h-1.5 w-1.5 rounded-full"
                        style={{ background: selectedKeyStatus.color }}
                      />
                      {selectedKeyStatus.label}
                    </span>
                  )}
                </div>
              )}
              <p className="text-[11px] leading-relaxed text-muted-foreground">
                选择网关密钥（本地明文存储），写入时以明文合并进客户端配置。
              </p>
            </div>

            {/* MODEL（下拉） */}
            <div className="space-y-1.5">
              <div className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
                <span className="font-mono text-[10px]">M</span>
                MODEL
              </div>
              {modelOptions.length === 0 ? (
                <div className="rounded-md border border-dashed px-3 py-2 text-xs text-muted-foreground">
                  暂无可用模型，请先在「渠道管理」启用至少一个渠道
                </div>
              ) : (
                <Select
                  value={model}
                  onChange={(e) => setModel(e.target.value)}
                  className="font-mono text-xs"
                >
                  {modelOptions.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </Select>
              )}
            </div>
          </div>

          {/* 操作按钮 */}
          <div className="flex flex-wrap items-center gap-2 pt-1">
            <Button
              onClick={handleApply}
              disabled={!canWrite || applying}
              className="gap-1.5"
            >
              {applying ? (
                <RefreshCw className="h-3.5 w-3.5 animate-spin" />
              ) : (
                <CheckCircle2 className="h-3.5 w-3.5" />
              )}
              一键写入配置
            </Button>
            <Button
              variant="outline"
              onClick={handleRestore}
              className="gap-1.5"
              title="恢复到本网关修改前的原始配置"
            >
              <RotateCcw className="h-3.5 w-3.5" />
              恢复原始配置
            </Button>
          </div>
        </CardContent>
      </Card>

      {/* ── 配置文件内容 ── */}
      <Card>
        <CardHeader className="pb-3">
          <div className="flex items-center justify-between gap-2">
            <CardTitle className="text-base">配置文件</CardTitle>
            <div className="flex items-center gap-1.5">
              <code className="max-w-[280px] truncate rounded-md bg-muted/50 px-2 py-1 font-mono text-[11px] text-muted-foreground">
                {client.config_path ?? "—"}
              </code>
              <Button
                variant="outline"
                size="icon"
                onClick={handleOpenDir}
                className="h-8 w-8 shrink-0"
                title="打开配置所在目录"
              >
                <FolderOpen className="h-3.5 w-3.5" />
              </Button>
              <Button
                variant="outline"
                size="icon"
                onClick={() => copy(client.config_path ?? "", "配置路径")}
                className="h-8 w-8 shrink-0"
                title="复制配置路径"
              >
                <Copy className="h-3.5 w-3.5" />
              </Button>
              <Button
                variant="outline"
                size="icon"
                onClick={handleRefresh}
                disabled={refreshing || loadingContent}
                className="h-8 w-8 shrink-0"
                title="刷新文件内容"
              >
                <RefreshCw
                  className={cn("h-3.5 w-3.5", refreshing && "animate-spin")}
                />
              </Button>
            </div>
          </div>
        </CardHeader>
        <CardContent>
          {!client.available && (
            <div className="mb-3 flex items-start gap-2 rounded-md border border-dashed border-amber-500/40 bg-amber-500/5 px-3 py-2 text-xs text-amber-600 dark:text-amber-400">
              <CircleAlert className="mt-0.5 h-3.5 w-3.5 shrink-0" />
              <span>
                未检测到 {client.label} 安装痕迹（{client.config_format}
                ）。若你刚安装，请点击右上角「刷新」重新检测；或先安装后重试。
              </span>
            </div>
          )}

          {loadingContent ? (
            <div className="space-y-2">
              <Skeleton className="h-4 w-2/3" />
              <Skeleton className="h-4 w-1/2" />
              <Skeleton className="h-4 w-3/4" />
            </div>
          ) : !content || !content.exists ? (
            <div className="rounded-md border border-dashed px-3 py-6 text-center text-xs text-muted-foreground">
              配置文件尚不存在（写入后将自动创建）
            </div>
          ) : content.error ? (
            <div className="rounded-md border border-destructive/30 bg-destructive/5 px-3 py-3 text-xs text-destructive">
              {content.error}
            </div>
          ) : (
            <CodeBlock
              code={content.content}
              lang={client.config_path?.endsWith(".toml") ? "toml" : "json"}
            />
          )}
        </CardContent>
      </Card>
    </div>
  );
}
