import { useEffect, useState } from "react";
import {
  Activity,
  Coins,
  Network,
  Timer,
  RefreshCw,
} from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { statsApi, channelApi } from "@/lib/api";
import type { DashboardStats, Channel } from "@/types";

function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
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

  const cards = [
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
  ];

  return (
    <div className="p-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">仪表盘</h1>
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
      <div className="mt-6 grid grid-cols-1 gap-4 md:grid-cols-2 lg:grid-cols-4">
        {cards.map((card) => {
          const Icon = card.icon;
          return (
            <Card key={card.title}>
              <CardHeader className="flex-row items-center justify-between space-y-0">
                <CardTitle className="text-sm font-medium text-muted-foreground">
                  {card.title}
                </CardTitle>
                <Icon className="h-4 w-4 text-muted-foreground" />
              </CardHeader>
              <CardContent>
                <div className="text-2xl font-bold">{card.value}</div>
                <p className="mt-1 text-xs text-muted-foreground">
                  {card.description}
                </p>
              </CardContent>
            </Card>
          );
        })}
      </div>

      {/* 渠道状态列表 */}
      <Card className="mt-6">
        <CardHeader>
          <CardTitle>渠道状态</CardTitle>
          <CardDescription>各上游渠道的运行情况</CardDescription>
        </CardHeader>
        <CardContent>
          {channels.length === 0 ? (
            <div className="flex flex-col items-center gap-2 py-8 text-center">
              <Network className="h-8 w-8 text-muted-foreground" />
              <p className="text-sm text-muted-foreground">
                暂无渠道，前往「渠道管理」添加第一个渠道
              </p>
            </div>
          ) : (
            <div className="space-y-2">
              {channels.map((ch) => (
                <div
                  key={ch.id}
                  className="flex items-center justify-between rounded-md border px-4 py-2"
                >
                  <div className="flex items-center gap-3">
                    <span className="font-medium">{ch.name}</span>
                    <span className="text-xs text-muted-foreground">
                      {ch.type} · {ch.models.length} 模型
                    </span>
                  </div>
                  <Badge
                    variant={
                      ch.status === 1 ? "success" : ch.status === 2 ? "destructive" : "secondary"
                    }
                  >
                    {ch.status === 1 ? "启用" : ch.status === 2 ? "异常" : "禁用"}
                  </Badge>
                </div>
              ))}
            </div>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
