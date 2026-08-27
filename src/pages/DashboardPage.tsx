import { useEffect, useState } from "react";
import {
  Activity,
  Coins,
  Network,
  Timer,
  RefreshCw,
  ArrowUpRight,
  KeyRound,
  BarChart3,
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

interface StatDef {
  title: string;
  value: string;
  icon: typeof Activity;
  description: string;
  hint?: string;
}

function StatCard({ stat }: { stat: StatDef }) {
  const Icon = stat.icon;
  return (
    <Card className="group relative overflow-hidden transition-all duration-200 hover:-translate-y-0.5 hover:shadow-md">
      <CardContent className="pt-6">
        <div className="flex items-start justify-between">
          <div className="space-y-1">
            <p className="text-sm text-muted-foreground">{stat.title}</p>
            <p className="text-3xl font-semibold tracking-tight tabular-nums">
              {stat.value}
            </p>
          </div>
          <div className="flex h-11 w-11 items-center justify-center rounded-xl bg-primary/10 text-primary ring-1 ring-inset ring-primary/15 transition-colors group-hover:bg-primary/15">
            <Icon className="h-5 w-5" />
          </div>
        </div>
        <div className="mt-3 flex items-center gap-1.5 text-xs text-muted-foreground">
          <ArrowUpRight className="h-3.5 w-3.5 text-success" />
          <span>{stat.description}</span>
        </div>
      </CardContent>
    </Card>
  );
}

export function DashboardPage() {
  const [stats, setStats] = useState<DashboardStats | null>(null);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [loading, setLoading] = useState(true);

  const load = async () => {
    setLoading(true);
    try {
      const [s, c] = await Promise.all([statsApi.getDashboard(), channelApi.list()]);
      setStats(s);
      setChannels(c);
    } catch (e) {
      console.error("Failed to load dashboard:", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

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
        <Button variant="outline" size="sm" onClick={load} disabled={loading}>
          <RefreshCw className={loading ? "animate-spin" : ""} />
          刷新
        </Button>
      </div>

      {/* 统计卡片 */}
      <div className="mt-6 grid grid-cols-1 gap-4 sm:grid-cols-2 lg:grid-cols-4">
        {loading
          ? Array.from({ length: 4 }).map((_, i) => (
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
