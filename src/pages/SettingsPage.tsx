import { useEffect, useState } from "react";
import { RefreshCw, Save, RotateCw, Play, Square } from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Label } from "@/components/ui/label";
import { Switch } from "@/components/ui/switch";
import { Select } from "@/components/ui/select";
import { Tabs, TabsContent, TabsList, TabsTrigger } from "@/components/ui/tabs";
import { settingsApi, serverApi, type SettingsUpdate } from "@/lib/api";
import { applyTheme } from "@/lib/theme";
import { formatListenUrl } from "@/lib/utils";
import { Skeleton } from "@/components/ui/skeleton";
import { useToast } from "@/components/ui/toast";
import { useSearchParams } from "react-router-dom";
import type { Settings, ThemeMode, SecurityMode, ServerStatus } from "@/types";

/** 安全审计 Tab 内 6 个检测项的复用卡片（标签 + 右上角开关） */
function SecurityToggleCard({
  label,
  checked,
  onChange,
}: {
  label: string;
  checked: boolean;
  onChange: (v: boolean) => void;
}) {
  return (
    <div className="flex items-start justify-between gap-2 rounded-lg border bg-card/40 px-3 py-3">
      <p className="text-sm font-medium leading-tight">{label}</p>
      <Switch checked={checked} onCheckedChange={onChange} />
    </div>
  );
}

export function SettingsPage() {
  const toast = useToast();
  const [settings, setSettings] = useState<Settings | null>(null);
  const [loading, setLoading] = useState(true);
  const [saving, setSaving] = useState(false);
  const [saved, setSaved] = useState(false);
  const [serverStatus, setServerStatus] = useState<ServerStatus | null>(null);
  const [serverBusy, setServerBusy] = useState(false);

  const [searchParams] = useSearchParams();
  const initialTab = searchParams.get("tab") === "security" ? "security" : "server";

  /** 刷新网关服务运行态（实际监听地址 + 是否需重启） */
  const loadStatus = async () => {
    try {
      setServerStatus(await serverApi.status());
    } catch (e) {
      console.error("Failed to load server status:", e);
    }
  };

  const load = async () => {
    setLoading(true);
    try {
      setSettings(await settingsApi.get());
    } catch (e) {
      console.error("Failed to load settings:", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
    loadStatus();
  }, []);

  /** 局部更新字段 */
  const patch = (partial: Partial<Settings>) => {
    setSettings((s) => (s ? { ...s, ...partial } : s));
    setSaved(false);
  };

  const handleSave = async () => {
    if (!settings) return;
    setSaving(true);
    try {
      const update: SettingsUpdate = {
        server_port: settings.server_port,
        server_host: settings.server_host,
        ui_theme: settings.ui_theme,
        ui_language: settings.ui_language,
        minimize_to_tray: settings.minimize_to_tray,
        close_to_tray: settings.close_to_tray,
        auto_start: settings.auto_start,
        retry_enabled: settings.retry_enabled,
        retry_times: settings.retry_times,
        log_retention_days: settings.log_retention_days,
        log_raw_body: settings.log_raw_body,
        security_enabled: settings.security_enabled,
        security_mode: settings.security_mode,
        security_detect_unicode_stego: settings.security_detect_unicode_stego,
        security_detect_tool_risk: settings.security_detect_tool_risk,
        security_detect_outbound_tracking: settings.security_detect_outbound_tracking,
        security_scan_response: settings.security_scan_response,
        security_redact_request: settings.security_redact_request,
        security_block_critical: settings.security_block_critical,
      };
      const result = await settingsApi.update(update);
      setSettings(result);
      setSaved(true);
      // 重新拉运行态，据此判断监听地址改动是否还需重启
      const status = await serverApi.status().catch(() => null);
      if (status) setServerStatus(status);

      const changed =
        !!status &&
        (result.server_host !== status.host || result.server_port !== status.port);
      toast.success(changed ? "设置已保存，重启服务后生效" : "设置已保存");
      setTimeout(() => setSaved(false), 2000);
    } catch (e) {
      console.error("Failed to save settings:", e);
      toast.error("保存失败");
    } finally {
      setSaving(false);
    }
  };

  /** 运行态与当前表单配置不一致 → 改动尚未生效 */
  const pendingChange =
    !!settings &&
    !!serverStatus?.running &&
    (settings.server_host !== serverStatus.host ||
      settings.server_port !== serverStatus.port);

  /** 统一的运行结果反馈：成功刷新状态，失败提示原因 */
  const runServerAction = async (action: () => Promise<ServerStatus>, okMsg: (s: ServerStatus) => string) => {
    setServerBusy(true);
    try {
      const s = await action();
      setServerStatus(s);
      toast.success(okMsg(s));
    } catch (e) {
      // 启动/重启失败时服务可能已停，拉一次真实状态避免显示成旧地址
      setServerStatus(await serverApi.status().catch(() => null));
      toast.error(String(e));
    } finally {
      setServerBusy(false);
    }
  };

  const handleRestart = () =>
    runServerAction(
      serverApi.restart,
      (s) => `服务已重启：${formatListenUrl(s.host, s.port)}`,
    );

  const handleStop = () =>
    runServerAction(serverApi.stop, () => "服务已停止");

  const handleStart = () =>
    runServerAction(
      serverApi.start,
      (s) => `服务已启动：${formatListenUrl(s.host, s.port)}`,
    );

  if (loading || !settings) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-9 w-44" />
        <Skeleton className="h-5 w-72" />
        <Skeleton className="mt-6 h-72 w-full rounded-xl" />
      </div>
    );
  }

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">设置</h1>
          <p className="mt-1 text-sm text-muted-foreground">服务配置、通用设置、界面、重试策略</p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={load} disabled={loading}>
            <RefreshCw className={loading ? "animate-spin" : ""} />
            重置
          </Button>
          <Button size="sm" onClick={handleSave} disabled={saving}>
            <Save />
            {saving ? "保存中..." : saved ? "已保存" : "保存"}
          </Button>
        </div>
      </div>

      <Tabs defaultValue={initialTab} className="mt-6 gap-4">
        <TabsList>
          <TabsTrigger value="server">服务配置</TabsTrigger>
          <TabsTrigger value="general">通用设置</TabsTrigger>
          <TabsTrigger value="appearance">界面设置</TabsTrigger>
          <TabsTrigger value="retry">重试策略</TabsTrigger>
          <TabsTrigger value="security">安全审计</TabsTrigger>
        </TabsList>

        {/* ================= 服务配置 ================= */}
        <TabsContent value="server">
          <Card>
            <CardHeader>
              <CardTitle>网关服务</CardTitle>
              <CardDescription>
                Axum 数据面 HTTP 服务监听地址，修改后需重启服务生效
              </CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4">
              <div className="grid max-w-sm grid-cols-[100px_1fr] items-center gap-4">
                <Label htmlFor="st-host">监听地址</Label>
                <Select
                  id="st-host"
                  value={settings.server_host}
                  onChange={(e) => patch({ server_host: e.target.value })}
                >
                  <option value="127.0.0.1">127.0.0.1（仅本机）</option>
                  <option value="0.0.0.0">0.0.0.0（局域网可访问）</option>
                </Select>
              </div>
              <div className="grid max-w-sm grid-cols-[100px_1fr] items-center gap-4">
                <Label htmlFor="st-port">监听端口</Label>
                <Input
                  id="st-port"
                  type="number"
                  min={1024}
                  max={65535}
                  value={settings.server_port}
                  onChange={(e) =>
                    patch({ server_port: Number(e.target.value) || 9842 })
                  }
                />
              </div>
              <p className="text-xs text-muted-foreground">
                配置端点：
                <code className="rounded bg-muted px-1.5 py-0.5 font-mono">
                  {formatListenUrl(settings.server_host, settings.server_port)}
                </code>
              </p>

              <div className="h-px bg-border" />

              {/* 运行状态：展示服务实际监听的地址，与上方配置值对照 */}
              <div className="flex items-start justify-between gap-4 max-w-md">
                <div className="min-w-0">
                  <p className="text-sm font-medium">运行状态</p>
                  <p className="mt-0.5 text-xs text-muted-foreground">
                    {!serverStatus
                      ? "加载中..."
                      : serverStatus.running
                        ? `实际监听 ${formatListenUrl(serverStatus.host, serverStatus.port)}`
                        : `服务未运行（配置端口 ${serverStatus.configured_port}）`}
                  </p>
                  {pendingChange && (
                    <p className="mt-1 text-xs text-warning">
                      监听地址/端口已修改，重启服务后生效
                    </p>
                  )}
                </div>
                <div className="flex shrink-0 gap-2">
                  <Button
                    variant="outline"
                    size="sm"
                    onClick={handleRestart}
                    disabled={serverBusy}
                    title="按当前配置重启服务"
                  >
                    <RotateCw className={serverBusy ? "animate-spin" : ""} />
                    重启
                  </Button>
                  {serverStatus?.running ? (
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={handleStop}
                      disabled={serverBusy}
                    >
                      <Square />
                      停止
                    </Button>
                  ) : (
                    <Button
                      variant="outline"
                      size="sm"
                      onClick={handleStart}
                      disabled={serverBusy}
                    >
                      <Play />
                      启动
                    </Button>
                  )}
                </div>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        {/* ================= 通用设置 ================= */}
        <TabsContent value="general">
          <Card>
            <CardHeader>
              <CardTitle>通用</CardTitle>
              <CardDescription>系统行为与日志策略</CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4">
              <div className="flex items-center justify-between max-w-md">
                <div>
                  <p className="text-sm font-medium">开机自启动</p>
                  <p className="text-xs text-muted-foreground">登录系统后自动运行 DongX</p>
                </div>
                <Switch
                  checked={settings.auto_start}
                  onCheckedChange={(v) => patch({ auto_start: v })}
                />
              </div>
              <div className="flex items-center justify-between max-w-md">
                <div>
                  <p className="text-sm font-medium">最小化到托盘</p>
                  <p className="text-xs text-muted-foreground">点击最小化时隐藏到系统托盘</p>
                </div>
                <Switch
                  checked={settings.minimize_to_tray}
                  onCheckedChange={(v) => patch({ minimize_to_tray: v })}
                />
              </div>
              <div className="flex items-center justify-between max-w-md">
                <div>
                  <p className="text-sm font-medium">关闭时到托盘</p>
                  <p className="text-xs text-muted-foreground">点击关闭按钮时隐藏而非退出</p>
                </div>
                <Switch
                  checked={settings.close_to_tray}
                  onCheckedChange={(v) => patch({ close_to_tray: v })}
                />
              </div>
              <div className="h-px bg-border" />
              <div className="grid max-w-sm grid-cols-[140px_1fr] items-center gap-4">
                <Label htmlFor="gt-retention">日志保留天数</Label>
                <Input
                  id="gt-retention"
                  type="number"
                  min={1}
                  max={365}
                  value={settings.log_retention_days}
                  onChange={(e) =>
                    patch({ log_retention_days: Number(e.target.value) || 30 })
                  }
                />
              </div>
              <div className="flex items-center justify-between max-w-md">
                <div>
                  <p className="text-sm font-medium">记录原始请求体</p>
                  <p className="text-xs text-muted-foreground">
                    保存请求/响应完整 body（占用更多磁盘，含敏感信息自动脱敏）
                  </p>
                </div>
                <Switch
                  checked={settings.log_raw_body}
                  onCheckedChange={(v) => patch({ log_raw_body: v })}
                />
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        {/* ================= 界面设置 ================= */}
        <TabsContent value="appearance">
          <Card>
            <CardHeader>
              <CardTitle>界面</CardTitle>
              <CardDescription>主题与语言</CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4">
              <div className="grid max-w-sm grid-cols-[100px_1fr] items-center gap-4">
                <Label htmlFor="ap-theme">主题</Label>
                <Select
                  id="ap-theme"
                  value={settings.ui_theme}
                  onChange={(e) => {
                    const mode = e.target.value as ThemeMode;
                    patch({ ui_theme: mode });
                    applyTheme(mode);
                  }}
                >
                  <option value="system">跟随系统</option>
                  <option value="light">浅色</option>
                  <option value="dark">深色</option>
                </Select>
              </div>
              <div className="grid max-w-sm grid-cols-[100px_1fr] items-center gap-4">
                <Label htmlFor="ap-lang">语言</Label>
                <Select
                  id="ap-lang"
                  value={settings.ui_language}
                  onChange={(e) => patch({ ui_language: e.target.value })}
                >
                  <option value="zh-CN">简体中文</option>
                  <option value="en-US">English</option>
                </Select>
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        {/* ================= 重试策略 ================= */}
        <TabsContent value="retry">
          <Card>
            <CardHeader>
              <CardTitle>重试策略</CardTitle>
              <CardDescription>
                上游请求失败时的自动重试与故障转移
              </CardDescription>
            </CardHeader>
            <CardContent className="grid gap-4">
              <div className="flex items-center justify-between max-w-md">
                <div>
                  <p className="text-sm font-medium">启用自动重试</p>
                  <p className="text-xs text-muted-foreground">
                    5xx / 超时 / 网络错误时自动重试或切换渠道
                  </p>
                </div>
                <Switch
                  checked={settings.retry_enabled}
                  onCheckedChange={(v) => patch({ retry_enabled: v })}
                />
              </div>
              <div className="grid max-w-sm grid-cols-[140px_1fr] items-center gap-4">
                <Label htmlFor="rt-times">最大重试次数</Label>
                <Input
                  id="rt-times"
                  type="number"
                  min={0}
                  max={10}
                  disabled={!settings.retry_enabled}
                  value={settings.retry_times}
                  onChange={(e) =>
                    patch({ retry_times: Number(e.target.value) || 0 })
                  }
                />
              </div>
            </CardContent>
          </Card>
        </TabsContent>

        {/* ================= 安全审计 ================= */}
        <TabsContent value="security">
          <Card>
            <CardHeader>
              <CardTitle>安全审计</CardTitle>
              <CardDescription>
                对请求内容进行敏感信息扫描与风险分级，按模式处置
              </CardDescription>
            </CardHeader>
            <CardContent className="grid gap-6">
              {/*** 第一行：启用安全审计 + 安全模式 左右分栏 ***/}
              <div className="grid gap-6 md:grid-cols-2 md:gap-8">
                {/* 左：启用安全审计 */}
                <div className="flex items-start justify-between gap-4">
                  <div>
                    <p className="text-sm font-medium">启用安全审计</p>
                    <p className="mt-0.5 text-xs text-muted-foreground">
                      扫描请求体中的密钥、身份证、手机号等敏感信息
                    </p>
                  </div>
                  <Switch
                    checked={settings.security_enabled}
                    onCheckedChange={(v) => patch({ security_enabled: v })}
                  />
                </div>

                {/* 右：安全模式 */}
                <div className="grid gap-2 md:border-l md:pl-8">
                  <Label htmlFor="sec-mode">安全模式</Label>
                  <Select
                    id="sec-mode"
                    disabled={!settings.security_enabled}
                    value={settings.security_mode}
                    onChange={(e) =>
                      patch({ security_mode: e.target.value as SecurityMode })
                    }
                  >
                    <option value="permissive">宽松（仅记录）</option>
                    <option value="warning">警告（中高风险标记告警）</option>
                    <option value="redact">脱敏（高风险脱敏转发）</option>
                    <option value="strict">严格（高风险直接阻断）</option>
                  </Select>
                </div>
              </div>

              {/*** 分隔 ***/}
              <div className="h-px bg-border" />

              {/*** 检测项开关：6 个独立开关，控制扫描哪类风险 ***/}
              <div className="grid gap-3 sm:grid-cols-2 lg:grid-cols-3 xl:grid-cols-6">
                <SecurityToggleCard
                  label="Unicode 隐写检测"
                  checked={settings.security_detect_unicode_stego}
                  onChange={(v) => patch({ security_detect_unicode_stego: v })}
                />
                <SecurityToggleCard
                  label="工具/命令风险检测"
                  checked={settings.security_detect_tool_risk}
                  onChange={(v) => patch({ security_detect_tool_risk: v })}
                />
                <SecurityToggleCard
                  label="外联/追踪风险检测"
                  checked={settings.security_detect_outbound_tracking}
                  onChange={(v) => patch({ security_detect_outbound_tracking: v })}
                />
                <SecurityToggleCard
                  label="响应侧安全扫描"
                  checked={settings.security_scan_response}
                  onChange={(v) => patch({ security_scan_response: v })}
                />
                <SecurityToggleCard
                  label="请求脱敏转发"
                  checked={settings.security_redact_request}
                  onChange={(v) => patch({ security_redact_request: v })}
                />
                <SecurityToggleCard
                  label="严重风险强制阻断"
                  checked={settings.security_block_critical}
                  onChange={(v) => patch({ security_block_critical: v })}
                />
              </div>

              {/*** 底部说明 ***/}
              <p className="text-xs text-muted-foreground">
                「请求脱敏转发」开启后，请求体中的 API
                Key、Token、私钥等敏感信息会在转发上游前被替换为脱敏值。「响应侧安全扫描」开启后，上游返回内容也会被扫描并记录风险。
              </p>
            </CardContent>
          </Card>
        </TabsContent>
      </Tabs>
    </div>
  );
}
