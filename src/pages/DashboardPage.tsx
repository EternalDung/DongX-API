import { useEffect, useRef, useState } from "react";
import { sleep } from "@/lib/async";
import {
  Activity,
  Coins,
  Network,
  Timer,
  RefreshCw,
  ArrowUpRight,
  KeyRound,
  BarChart3,
  ShieldCheck,
} from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusBadge, type StatusTone } from "@/components/ui/status-badge";
import { cn } from "@/lib/utils";
import { statsApi, channelApi } from "@/lib/api";
import type { DashboardStats, Channel } from "@/types";

function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

const CHANNEL_TONE: Record<number, { tone: StatusTone; label: string }> = {
  1: { tone: "success", label: "启用" },
  2: { tone: "destructive", label: "异常" },
  0: { tone: "secondary", label: "禁用" },
};

type StatTone = "primary" | "success" | "warning" | "destructive";

interface StatDef {
  title: string;
  value: string;
  icon: typeof Activity;
  description: string;
  hint?: string;
  /** 有 tone 时数字/图标/进度条走该色，底部说明用状态点代替箭头 */
  tone?: StatTone;
  /** 0-100，存在时渲染一条占比进度条 */
  progress?: number;
}

const TONE_VALUE: Record<StatTone, string> = {
  primary: "",
  success: "text-success",
  warning: "text-warning",
  destructive: "text-destructive",
};

const TONE_BOX: Record<StatTone, string> = {
  primary: "bg-primary/10 text-primary ring-primary/15 group-hover:bg-primary/15",
  success: "bg-success/10 text-success ring-success/15 group-hover:bg-success/15",
  warning: "bg-warning/10 text-warning ring-warning/15 group-hover:bg-warning/15",
  destructive:
    "bg-destructive/10 text-destructive ring-destructive/15 group-hover:bg-destructive/15",
};

const TONE_BAR: Record<StatTone, string> = {
  primary: "bg-primary",
  success: "bg-success",
  warning: "bg-warning",
  destructive: "bg-destructive",
};

const TONE_DOT: Record<StatTone, string> = {
  primary: "bg-primary",
  success: "bg-success",
  warning: "bg-warning",
  destructive: "bg-destructive",
};

function StatCard({ stat }: { stat: StatDef }) {
  const Icon = stat.icon;
  const tone = stat.tone ?? "primary";
  return (
    <Card className="group relative overflow-hidden transition-all duration-200 hover:-translate-y-0.5 hover:shadow-md">
      <CardContent className="pt-6">
        <div className="flex items-start justify-between">
          <div className="space-y-1">
            <p className="text-sm text-muted-foreground">{stat.title}</p>
            <p
              className={cn(
                "text-3xl font-semibold tracking-tight tabular-nums",
                TONE_VALUE[tone],
              )}
            >
              {stat.value}
            </p>
          </div>
          <div
            className={cn(
              "flex h-11 w-11 items-center justify-center rounded-xl ring-1 ring-inset transition-colors",
              TONE_BOX[tone],
            )}
          >
            <Icon className="h-5 w-5" />
          </div>
        </div>

        {stat.progress !== undefined && (
          <div className="mt-3 h-1.5 w-full overflow-hidden rounded-full bg-muted">
            <div
              className={cn(
                "h-full rounded-full transition-all duration-300",
                TONE_BAR[tone],
              )}
              style={{ width: `${Math.min(100, Math.max(0, stat.progress))}%` }}
            />
          </div>
        )}

        <div className="mt-3 flex items-center gap-1.5 text-xs text-muted-foreground">
          {stat.tone ? (
            <span className={cn("h-1.5 w-1.5 rounded-full", TONE_DOT[tone])} />
          ) : (
            <ArrowUpRight className="h-3.5 w-3.5 text-success" />
          )}
          <span>{stat.description}</span>
        </div>
      </CardContent>
    </Card>
  );
}

/** 启用渠道占总渠道的百分比；无渠道时返回 null（不展示误导性的 0%） */
function calcAvailability(stats: DashboardStats | null): number | null {
  if (!stats || stats.total_channels <= 0) return null;
  return Math.round((stats.active_channels / stats.total_channels) * 100);
}

/** 可用率配色阈值：≥80 健康 / ≥50 警告 / 否则危险 */
function availabilityTone(pct: number | null): StatTone {
  if (pct === null) return "primary";
  if (pct >= 80) return "success";
  if (pct >= 50) return "warning";
  return "destructive";
}

export function DashboardPage() {
  const [stats, setStats] = useState<DashboardStats | null>(null);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [loading, setLoading] = useState(true);

  // 方案A：骨架仅在「确无数据」(首屏) 显示；刷新时保留旧卡片，只让按钮图标旋转。
  const [spinning, setSpinning] = useState(false);
  const loadedRef = useRef(false);

  const load = async () => {
    const showSkeleton = !loadedRef.current;
    if (showSkeleton) setLoading(true);
    setSpinning(true);
    const started = Date.now();
    try {
      const [s, c] = await Promise.all([statsApi.getDashboard(), channelApi.list()]);
      setStats(s);
      setChannels(c);
    } catch (e) {
      console.error("Failed to load dashboard:", e);
    } finally {
      loadedRef.current = true;
      if (showSkeleton) setLoading(false);
      const elapsed = Date.now() - started;
      if (elapsed < 400) await sleep(400 - elapsed);
      setSpinning(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const availability = calcAvailability(stats);

  const cards: StatDef[] = [
    {
      title: "今日请求",
      value: stats ? formatNumber(stats.today_requests) : "--",
      icon: Activity,
      description: "过去 24 小时代理请求数",
    },
    {
      title: "今日 Token",
      value: stats ? formatNumber(stats.today_total_tokens) : "--",
      icon: Coins,
      description: "prompt + completion 总量",
    },
    {
      title: "活跃渠道",
      value: stats ? `${stats.active_channels}/${stats.total_channels}` : "--",
      icon: Network,
      description: "启用中 / 总渠道数",
    },
    {
      title: "服务可用率",
      value: availability === null ? "--" : `${availability}%`,
      icon: ShieldCheck,
      description:
        availability === null || !stats
          ? "暂无渠道，前往渠道管理添加"
          : `活跃 ${stats.active_channels} / 总 ${stats.total_channels} 渠道`,
      tone: availabilityTone(availability),
      progress: availability ?? undefined,
    },
    {
      title: "平均延迟",
      value: stats ? `${stats.avg_latency_ms}ms` : "--",
      icon: Timer,
      description: "上游响应耗时（均值）",
    },
    {
      title: "累计请求",
      value: stats ? formatNumber(stats.total_requests) : "--",
      icon: BarChart3,
      description: "历史代理请求总数",
    },
    {
      title: "累计 Token",
      value: stats ? formatNumber(stats.total_tokens) : "--",
      icon: Coins,
      description: "历史 Token 累计消耗",
    },
    {
      title: "密钥总数",
      value: stats ? String(stats.total_api_keys) : "--",
      icon: KeyRound,
      description: "已创建网关密钥",
    },
  ];

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">仪表盘</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            请求统计、Token 消耗、渠道状态概览
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={load} disabled={spinning}>
          <RefreshCw className={spinning ? "animate-spin" : ""} />
          刷新
        </Button>
      </div>

      {/* 统计卡片 */}
      <div className="mt-6 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {loading
          ? Array.from({ length: 8 }).map((_, i) => (
              <Card key={i}>
                <CardContent className="pt-6">
                  <div className="flex items-start justify-between">
                    <div className="space-y-2">
                      <Skeleton className="h-4 w-20" />
                      <Skeleton className="h-8 w-24" />
                    </div>
                    <Skeleton className="h-11 w-11 rounded-xl" />
                  </div>
                  <Skeleton className="mt-3 h-3 w-32" />
                </CardContent>
              </Card>
            ))
          : cards.map((card) => <StatCard key={card.title} stat={card} />)}
      </div>

      {/* 渠道状态列表 */}
      <Card className="mt-6">
        <CardHeader>
          <CardTitle>渠道状态</CardTitle>
          <CardDescription>各上游渠道的运行情况</CardDescription>
        </CardHeader>
        <CardContent>
          {loading ? (
            <div className="space-y-3">
              {Array.from({ length: 3 }).map((_, i) => (
                <Skeleton key={i} className="h-12 w-full" />
              ))}
            </div>
          ) : channels.length === 0 ? (
            <EmptyState
              icon={Network}
              title="暂无渠道"
              description="前往「渠道管理」添加第一个上游供应商渠道，网关即可开始代理请求。"
            />
          ) : (
            <div className="divide-y">
              {channels.map((ch) => {
                const meta = CHANNEL_TONE[ch.status] ?? CHANNEL_TONE[0];
                return (
                  <div
                    key={ch.id}
                    className="flex items-center justify-between gap-4 rounded-lg px-2 py-3 transition-colors hover:bg-accent/40"
                  >
                    <div className="flex min-w-0 items-center gap-3">
                      <span className="font-medium">{ch.name}</span>
                      <span className="font-mono text-xs text-muted-foreground">
                        {ch.type} · {ch.models.length} 模型
                      </span>
                    </div>
                    <StatusBadge tone={meta.tone}>{meta.label}</StatusBadge>
                  </div>
                );
              })}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
