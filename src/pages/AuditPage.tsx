import { useEffect, useState } from "react";
import { ShieldAlert, RefreshCw } from "lucide-react";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/ui/empty-state";
import { StatusBadge, type StatusTone } from "@/components/ui/status-badge";
import { useToast } from "@/components/ui/toast";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { auditApi } from "@/lib/api";
import type { AuditEvent, AuditEventType, AuditSeverity } from "@/types";

const SEVERITY_TONE: Record<AuditSeverity, StatusTone> = {
  info: "info",
  warning: "warning",
  critical: "destructive",
};

const TYPE_LABEL: Record<AuditEventType, string> = {
  rate_limit: "限流触发",
  invalid_key: "无效密钥",
  quota_exhaust: "配额耗尽",
  suspicious: "可疑行为",
  config_change: "配置变更",
};

function formatTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export function AuditPage() {
  const toast = useToast();
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [loading, setLoading] = useState(true);

  const load = async () => {
    setLoading(true);
    try {
      setEvents(await auditApi.list());
    } catch (e) {
      console.error("Failed to load audit events:", e);
      toast.error("审计事件加载失败");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">安全审计</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            风控规则命中、异常访问与配置变更记录
          </p>
        </div>
        <Button variant="outline" size="sm" onClick={load} disabled={loading}>
          <RefreshCw className={loading ? "animate-spin" : ""} />
          刷新
        </Button>
      </div>

      <Card className="mt-6 overflow-hidden">
        <CardContent className="pt-2">
          {loading ? (
            <div className="space-y-2 py-4">
              {Array.from({ length: 5 }).map((_, i) => (
                <Skeleton key={i} className="h-10 w-full" />
              ))}
            </div>
          ) : events.length === 0 ? (
            <EmptyState
              icon={ShieldAlert}
              title="暂无审计事件"
              description="当网关触发限流、拦截可疑请求或发生配置变更时，相关事件会记录在这里。"
            />
          ) : (
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead>时间</TableHead>
                  <TableHead>级别</TableHead>
                  <TableHead>类型</TableHead>
                  <TableHead>来源</TableHead>
                  <TableHead>详情</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {events.map((ev) => (
                  <TableRow key={ev.id}>
                    <TableCell className="whitespace-nowrap font-mono text-xs text-muted-foreground">
                      {formatTime(ev.timestamp)}
                    </TableCell>
                    <TableCell>
                      <StatusBadge tone={SEVERITY_TONE[ev.severity]}>
                        {ev.severity}
                      </StatusBadge>
                    </TableCell>
                    <TableCell>{TYPE_LABEL[ev.type] ?? ev.type}</TableCell>
                    <TableCell className="text-muted-foreground">
                      {ev.actor ?? "-"}
                    </TableCell>
                    <TableCell className="max-w-md text-sm">{ev.message}</TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>
    </div>
  );
}
