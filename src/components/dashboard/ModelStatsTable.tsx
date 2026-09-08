import { useCallback, useEffect, useState } from "react";
import { BarChart3, RefreshCw } from "lucide-react";
import {
  Card,
  CardContent,
  CardDescription,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Badge } from "@/components/ui/badge";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/ui/empty-state";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { cn } from "@/lib/utils";
import { sleep } from "@/lib/async";
import { statsApi } from "@/lib/api";
import type { ModelStat } from "@/types";

type Range = "today" | "7d" | "30d";

const RANGES: { key: Range; label: string }[] = [
  { key: "today", label: "今日" },
  { key: "7d", label: "近 7 天" },
  { key: "30d", label: "近 30 天" },
];

// 确定性调色板：同一模型名永远映射到同一颜色（无需外部依赖）。
const PALETTE = [
  "bg-sky-500/70",
  "bg-emerald-500/70",
  "bg-violet-500/70",
  "bg-amber-500/70",
  "bg-rose-500/70",
  "bg-cyan-500/70",
  "bg-fuchsia-500/70",
  "bg-lime-500/70",
  "bg-orange-500/70",
  "bg-indigo-500/70",
];

// 同一模型名在多次渲染里保持同一颜色，但若与上一行撞色则顺延到下一个色，避免相邻同色。
function colorFor(model: string, prevColor?: string): string {
  let h = 0;
  for (let i = 0; i < model.length; i++) {
    h = (h * 31 + model.charCodeAt(i)) >>> 0;
  }
  const idx = h % PALETTE.length;
  const candidate = PALETTE[idx];
  if (prevColor && candidate === prevColor) {
    return PALETTE[(idx + 1) % PALETTE.length];
  }
  return candidate;
}

function formatNumber(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return String(n);
}

/** 平均延迟单位自适应：>=1s 显示秒，否则显示毫秒。 */
function formatLatency(ms: number): string {
  if (ms >= 1000) return `${(ms / 1000).toFixed(2)}s`;
  if (ms >= 1) return `${Math.round(ms)}ms`;
  return `${ms.toFixed(0)}ms`;
}

function rangeToBounds(range: Range): { from?: string; to?: string } {
  const to = new Date();
  const from = new Date();
  if (range === "today") {
    from.setHours(0, 0, 0, 0);
  } else if (range === "7d") {
    from.setDate(from.getDate() - 7);
  } else {
    from.setDate(from.getDate() - 30);
  }
  return { from: from.toISOString(), to: to.toISOString() };
}

function successRate(s: ModelStat): number {
  if (s.total_count <= 0) return 0;
  return (s.success_count / s.total_count) * 100;
}

function rateTone(pct: number): string {
  if (pct >= 95) return "text-success";
  if (pct >= 80) return "text-warning";
  return "text-destructive";
}

// mode 展示顺序：先数据面（chat/responses/messages），后 RAG（rag/wiki），其余排末尾；组内按次数降序。
const MODE_ORDER = ["chat", "responses", "messages", "rag", "wiki"];
function modeChipVariant(mode: string): "pink" | "warning" | "secondary" {
  if (mode === "rag") return "pink";
  if (mode === "wiki") return "warning";
  return "secondary";
}
function sortedModes(breakdown: Record<string, number>): [string, number][] {
  return Object.entries(breakdown).sort((a, b) => {
    const ia = MODE_ORDER.indexOf(a[0]);
    const ib = MODE_ORDER.indexOf(b[0]);
    const ka = ia === -1 ? 999 : ia;
    const kb = ib === -1 ? 999 : ib;
    if (ka !== kb) return ka - kb;
    return b[1] - a[1];
  });
}

export function ModelStatsTable() {
  const [range, setRange] = useState<Range>("today");
  const [data, setData] = useState<ModelStat[]>([]);
  const [loading, setLoading] = useState(true);
  const [spinning, setSpinning] = useState(false);

  const load = useCallback(async () => {
    setSpinning(true);
    const started = Date.now();
    try {
      const bounds = rangeToBounds(range);
      const rows = await statsApi.getModelStats(bounds);
      setData(rows);
    } catch (e) {
      console.error("Failed to load model stats:", e);
    } finally {
      setLoading(false);
      // 加载与动画分离：数据再快也保证旋转至少 400ms，避免「加载太快没有动画」
      const elapsed = Date.now() - started;
      if (elapsed < 400) await sleep(400 - elapsed);
      setSpinning(false);
    }
  }, [range]);

  useEffect(() => {
    load();
  }, [load]);

  const grandTotal = data.reduce((s, r) => s + r.total_tokens, 0);

  return (
    <Card className="mt-6">
      <CardHeader className="flex flex-row items-center justify-between gap-4 space-y-0">
        <div>
          <CardTitle className="flex items-center gap-2">
            <BarChart3 className="h-4 w-4 text-primary" />
            模型调用统计
          </CardTitle>
          <CardDescription>
            按模型聚合的请求数、Token 消耗与成功率
          </CardDescription>
        </div>
        <div className="flex items-center gap-2">
          <div className="flex rounded-md border p-0.5">
            {RANGES.map((r) => (
              <button
                key={r.key}
                type="button"
                onClick={() => setRange(r.key)}
                className={cn(
                  "rounded px-2.5 py-1 text-xs font-medium transition-colors",
                  range === r.key
                    ? "bg-primary text-primary-foreground"
                    : "text-muted-foreground hover:text-foreground",
                )}
              >
                {r.label}
              </button>
            ))}
          </div>
          <Button variant="outline" size="sm" onClick={load} disabled={spinning}>
            <RefreshCw className={spinning ? "animate-spin" : ""} />
          </Button>
        </div>
      </CardHeader>
      <CardContent>
        {loading ? (
          <div className="space-y-2">
            {Array.from({ length: 5 }).map((_, i) => (
              <Skeleton key={i} className="h-10 w-full" />
            ))}
          </div>
        ) : data.length === 0 ? (
          <EmptyState
            icon={BarChart3}
            title="暂无调用数据"
            description="当前时间范围内没有模型调用记录，发起请求后这里会展示各模型的用量明细。"
          />
        ) : (
          <Table>
            <TableHeader>
              <TableRow>
                <TableHead>模型</TableHead>
                <TableHead className="text-right">请求数</TableHead>
                <TableHead className="text-right">输入Token</TableHead>
                <TableHead className="text-right">输出Token</TableHead>
                <TableHead className="text-right">缓存命中</TableHead>
                <TableHead className="text-right">总Token</TableHead>
                <TableHead className="w-36">Token占比</TableHead>
                <TableHead className="text-right">成功率</TableHead>
                <TableHead className="text-right">平均延迟</TableHead>
              </TableRow>
            </TableHeader>
            <TableBody>
              {(() => {
                const rowColors: string[] = [];
                for (let i = 0; i < data.length; i++) {
                  rowColors.push(colorFor(data[i].model, rowColors[i - 1]));
                }
                return data.map((s, i) => {
                const rate = successRate(s);
                const share = grandTotal > 0 ? (s.total_tokens / grandTotal) * 100 : 0;
                const cachePct =
                  s.prompt_tokens > 0
                    ? (s.cached_tokens / s.prompt_tokens) * 100
                    : 0;
                return (
                  <TableRow key={s.model}>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <span
                          className={cn(
                            "h-2.5 w-2.5 shrink-0 rounded-full",
                            rowColors[i],
                          )}
                        />
                  <div className="flex min-w-0 flex-wrap items-center gap-1.5">
                    <div className="truncate font-medium">{s.model}</div>
                    <div className="flex shrink-0 flex-wrap items-center gap-1">
                      {sortedModes(s.mode_breakdown).map(([mode, cnt]) => (
                        <Badge
                          key={mode}
                          variant={modeChipVariant(mode)}
                          className="rounded-full px-2 py-0 text-[10px] font-medium"
                        >
                          {mode} {cnt}
                        </Badge>
                      ))}
                    </div>
                  </div>
                      </div>
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {formatNumber(s.request_count)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums text-muted-foreground">
                      {formatNumber(s.prompt_tokens)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums text-muted-foreground">
                      {formatNumber(s.completion_tokens)}
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      <span>{formatNumber(s.cached_tokens)}</span>
                      {cachePct > 0 && (
                        <span className="ml-1 text-[11px] text-muted-foreground">
                          {cachePct.toFixed(0)}%
                        </span>
                      )}
                    </TableCell>
                    <TableCell className="text-right tabular-nums font-medium">
                      {formatNumber(s.total_tokens)}
                    </TableCell>
                    <TableCell>
                      <div className="flex items-center gap-2">
                        <div className="h-1.5 flex-1 overflow-hidden rounded-full bg-muted">
                          <div
                            className="h-full rounded-full bg-primary transition-all duration-300"
                            style={{ width: `${share}%` }}
                          />
                        </div>
                        <span className="w-12 text-right text-xs tabular-nums text-muted-foreground">
                          {share.toFixed(1)}%
                        </span>
                      </div>
                    </TableCell>
                    <TableCell
                      className={cn(
                        "text-right tabular-nums font-medium",
                        rateTone(rate),
                      )}
                    >
                      {rate.toFixed(1)}%
                    </TableCell>
                    <TableCell className="text-right tabular-nums">
                      {formatLatency(s.avg_latency_ms)}
                    </TableCell>
                  </TableRow>
                );
              })})()}
            </TableBody>
          </Table>
        )}
      </CardContent>
    </Card>
  );
}
