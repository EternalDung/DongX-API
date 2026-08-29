import { Fragment, useEffect, useState } from "react";
import { ShieldAlert, RefreshCw, ChevronDown, ChevronRight, Settings } from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
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
import { Select } from "@/components/ui/select";
import { auditApi, type AuditQuery } from "@/lib/api";
import type { AuditEvent, AuditEventType, AuditSeverity } from "@/types";
import { useNavigate } from "react-router-dom";

const PAGE_SIZE = 20;

const SEVERITIES: { value: string; label: string }[] = [
  { value: "", label: "全部级别" },
  { value: "info", label: "信息" },
  { value: "warning", label: "警告" },
  { value: "critical", label: "严重" },
];

const TYPES: { value: string; label: string }[] = [
  { value: "", label: "全部类型" },
  { value: "rate_limit", label: "限流触发" },
  { value: "invalid_key", label: "无效密钥" },
  { value: "quota_exhaust", label: "配额耗尽" },
  { value: "suspicious", label: "可疑行为" },
  { value: "config_change", label: "配置变更" },
];

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
  const navigate = useNavigate();
  const [events, setEvents] = useState<AuditEvent[]>([]);
  const [loading, setLoading] = useState(true);
  const [severity, setSeverity] = useState("");
  const [type, setType] = useState("");
  const [page, setPage] = useState(0);
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const load = async (nextPage = page, sev = severity, typ = type) => {
    setLoading(true);
    try {
      const query: AuditQuery = {
        severity: sev || undefined,
        event_type: typ || undefined,
        page: nextPage + 1,
        page_size: PAGE_SIZE,
      };
      const list = await auditApi.list(query);
      setEvents(list);
    } catch (e) {
      console.error("Failed to load audit events:", e);
      toast.error("审计事件加载失败");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load(0, "", "");
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  const applyFilter = (sev: string, typ: string) => {
    setSeverity(sev);
    setType(typ);
    setExpandedId(null);
    load(0, sev, typ);
  };

  const goPrev = () => {
    if (page <= 0) return;
    const p = page - 1;
    setPage(p);
    load(p);
  };

  const goNext = () => {
    if (events.length < PAGE_SIZE) return;
    const p = page + 1;
    setPage(p);
    load(p);
  };

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">安全审计</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            风控规则命中、异常访问与配置变更记录
          </p>
        </div>
        <div className="flex gap-2">
          <Button
            variant="outline"
            size="sm"
            onClick={() => navigate("/settings?tab=security")}
          >
            <Settings />
            设置
          </Button>
          <Button
            variant="outline"
            size="sm"
            onClick={() => load()}
            disabled={loading}
          >
            <RefreshCw className={loading ? "animate-spin" : ""} />
            刷新
          </Button>
        </div>
      </div>

      {/* 过滤 */}
      <div className="mt-4 flex flex-wrap items-center gap-2">
        <Select
          value={severity}
          onChange={(e) => applyFilter(e.target.value, type)}
          className="w-32"
        >
          {SEVERITIES.map((s) => (
            <option key={s.value} value={s.value}>
              {s.label}
            </option>
          ))}
        </Select>
        <Select
          value={type}
          onChange={(e) => applyFilter(severity, e.target.value)}
          className="w-36"
        >
          {TYPES.map((t) => (
            <option key={t.value} value={t.value}>
              {t.label}
            </option>
          ))}
        </Select>
        <span className="ml-auto text-xs text-muted-foreground">第 {page + 1} 页</span>
      </div>

      <Card className="mt-3 overflow-hidden">
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
                  <TableHead className="w-8"></TableHead>
                  <TableHead>时间</TableHead>
                  <TableHead>级别</TableHead>
                  <TableHead>类型</TableHead>
                  <TableHead>来源</TableHead>
                  <TableHead>详情</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {events.map((ev) => {
                  const expanded = expandedId === ev.id;
                  return (
                    <Fragment key={ev.id}>
                      <TableRow
                        className="cursor-pointer"
                        onClick={() => setExpandedId(expanded ? null : ev.id)}
                      >
                        <TableCell className="w-8 text-muted-foreground">
                          {expanded ? <ChevronDown size={16} /> : <ChevronRight size={16} />}
                        </TableCell>
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
                        <TableCell className="max-w-md truncate text-sm">
                          {ev.message}
                        </TableCell>
                      </TableRow>
                      {expanded && (
                        <TableRow className="hover:bg-transparent">
                          <TableCell colSpan={6} className="bg-muted/30">
                            <div className="grid gap-3 p-1 text-sm">
                              <div>
                                <p className="mb-1 text-xs font-medium text-muted-foreground">
                                  详情
                                </p>
                                <p className="whitespace-pre-wrap leading-relaxed">
                                  {ev.message}
                                </p>
                              </div>
                              {ev.meta && Object.keys(ev.meta).length > 0 && (
                                <div>
                                  <p className="mb-1 text-xs font-medium text-muted-foreground">
                                    元数据
                                  </p>
                                  <pre className="max-h-64 overflow-auto rounded-lg border bg-zinc-950 p-3 font-mono text-[12px] leading-relaxed text-zinc-100">
                                    {JSON.stringify(ev.meta, null, 2)}
                                  </pre>
                                </div>
                              )}
                            </div>
                          </TableCell>
                        </TableRow>
                      )}
                    </Fragment>
                  );
                })}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      {/* 分页 */}
      {!loading && events.length > 0 && (
        <div className="mt-4 flex items-center justify-between">
          <Button
            variant="outline"
            size="sm"
            onClick={goPrev}
            disabled={page === 0}
          >
            上一页
          </Button>
          <span className="text-xs text-muted-foreground">
            {events.length < PAGE_SIZE ? "已到最后一页" : "还有更多"}
          </span>
          <Button
            variant="outline"
            size="sm"
            onClick={goNext}
            disabled={events.length < PAGE_SIZE}
          >
            下一页
          </Button>
        </div>
      )}
    </div>
  );
}
