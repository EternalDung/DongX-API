import { Activity } from "lucide-react";
import { cn } from "@/lib/utils";

/** 渠道 / 网关密钥 共用的「调用统计」卡片数据契约。
 *  后端 ChannelStatsRow / ApiKeyStatsRow 序列化字段对齐。 */
export interface CallStatsData {
  total: number;
  successes: number;
  /** 0-100 整数 */
  success_rate: number;
  avg_latency_ms: number;
  prompt_tokens_sum: number;
  completion_tokens_sum: number;
  /** 最后调用时间（"YYYY-MM-DD HH:MM:SS" 或 RFC3339），无记录时为 null */
  last_called_at: string | null;
}

interface CallStatsCardProps {
  stats: CallStatsData;
  /** 可选：在卡片下方追加内容（如渠道的「最近测试」行）。 */
  footer?: React.ReactNode;
  className?: string;
}

/** 把数字格式化成 K / M 紧凑形式（参考图风格：49.9K、132.7K）。 */
function fmtTokens(n: number): string {
  if (n >= 1_000_000) return `${(n / 1_000_000).toFixed(1)}M`;
  if (n >= 1_000) return `${(n / 1_000).toFixed(1)}K`;
  return n.toLocaleString();
}

/** 把 SQLite 的 "YYYY-MM-DD HH:MM:SS" 或 RFC3339 格式化成 "YYYY/M/D HH:MM:SS" 本地时间。
 *  容错：解析失败返回原串。 */
function formatLast(v: string | null): string {
  if (!v) return "—";
  // SQLite strftime 产出 "YYYY-MM-DD HH:MM:SS"（无 T、无时区）；补成 ISO 让 Date 解析。
  const isoLike = v.includes("T") ? v : v.replace(" ", "T") + "Z";
  const d = new Date(isoLike);
  if (Number.isNaN(d.getTime())) return v;
  const p = (n: number) => (n < 10 ? `0${n}` : `${n}`);
  return `${d.getFullYear()}/${d.getMonth() + 1}/${d.getDate()} ${p(d.getHours())}:${p(d.getMinutes())}:${p(d.getSeconds())}`;
}

/** 平均延迟秒数展示：<1s 显示毫秒，否则显示秒。 */
function fmtLatency(ms: number): string {
  if (ms < 1000) return `${ms} ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

function SuccessRateRing({ rate }: { rate: number }) {
  const r = 22;
  const c = 2 * Math.PI * r; // ≈ 138.23
  const pct = Math.max(0, Math.min(100, rate));
  const ringColor =
    pct >= 90
      ? "text-emerald-500"
      : pct >= 70
        ? "text-amber-500"
        : "text-rose-500";
  return (
    <div className="relative h-14 w-14 shrink-0">
      <svg className="h-14 w-14 -rotate-90" viewBox="0 0 56 56" aria-hidden>
        <circle
          cx={28}
          cy={28}
          r={r}
          fill="none"
          strokeWidth={4}
          className="text-muted/30"
          stroke="currentColor"
        />
        <circle
          cx={28}
          cy={28}
          r={r}
          fill="none"
          strokeWidth={4}
          strokeLinecap="round"
          strokeDasharray={`${(pct / 100) * c} ${c}`}
          className={cn("transition-all", ringColor)}
          stroke="currentColor"
        />
      </svg>
      <div className="absolute inset-0 flex items-center justify-center text-sm font-semibold tabular-nums">
        {pct}%
      </div>
    </div>
  );
}

function LatencyBar({ ms }: { ms: number }) {
  // 0–5000ms 映射到 5%–100% 宽度，>5s 视为满格
  const pct = Math.min(100, Math.max(5, (ms / 5000) * 100));
  const color =
    ms < 1000
      ? "bg-emerald-500"
      : ms < 3000
        ? "bg-amber-500"
        : "bg-rose-500";
  return (
    <div className="mt-1 h-1.5 w-full overflow-hidden rounded-full bg-muted">
      <div
        className={cn("h-full rounded-full transition-all", color)}
        style={{ width: `${pct}%` }}
      />
    </div>
  );
}

export function CallStatsCard({ stats, footer, className }: CallStatsCardProps) {
  const failures = stats.total - stats.successes;
  const isEmpty = stats.total === 0;

  return (
    <div className={cn("rounded-lg border bg-card p-4", className)}>
      <div className="mb-3 flex items-center justify-between">
        <div className="flex items-center gap-1.5 text-sm font-medium text-foreground">
          <Activity className="h-4 w-4 text-muted-foreground" />
          调用统计
        </div>
        <span className="text-xs text-muted-foreground tabular-nums">
          最后调用 {formatLast(stats.last_called_at)}
        </span>
      </div>

      {isEmpty ? (
        <div className="py-4 text-center text-sm text-muted-foreground">
          暂无请求记录（近 30 天）
        </div>
      ) : (
        <div className="flex items-center gap-5">
          <SuccessRateRing rate={stats.success_rate} />

          <div className="flex-1 grid grid-cols-3 gap-4">
            {/* 调用 */}
            <div className="min-w-0">
              <div className="text-xs text-muted-foreground">调用</div>
              <div className="text-xl font-semibold tabular-nums text-foreground">
                {stats.total.toLocaleString()}
              </div>
              <div className="text-[11px] text-muted-foreground tabular-nums">
                成功 {stats.successes} / 失败 {failures}
              </div>
            </div>

            {/* Token */}
            <div className="min-w-0">
              <div className="text-xs text-muted-foreground">Token</div>
              <div className="text-xl font-semibold tabular-nums text-foreground">
                {fmtTokens(stats.prompt_tokens_sum + stats.completion_tokens_sum)}
              </div>
              <div className="text-[11px] text-muted-foreground tabular-nums">
                {fmtTokens(stats.prompt_tokens_sum)} /{" "}
                {fmtTokens(stats.completion_tokens_sum)}
              </div>
            </div>

            {/* 延迟 */}
            <div className="min-w-0">
              <div className="text-xs text-muted-foreground">延迟</div>
              <div className="text-xl font-semibold tabular-nums text-foreground">
                {fmtLatency(stats.avg_latency_ms)}
              </div>
              <LatencyBar ms={stats.avg_latency_ms} />
            </div>
          </div>
        </div>
      )}

      {footer}
    </div>
  );
}