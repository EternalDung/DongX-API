import { useEffect, useMemo, useState } from "react";
import {
  Search,
  RefreshCw,
  ScrollText,
  Trash2,
  AlertTriangle,
} from "lucide-react";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Select } from "@/components/ui/select";
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
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { logApi, channelApi } from "@/lib/api";
import type { RequestLog, Channel } from "@/types";

function statusTone(code: number): StatusTone {
  if (code >= 500) return "destructive";
  if (code >= 400) return "warning";
  return "success";
}
function statusLabel(code: number): string {
  if (code >= 500) return "服务端错误";
  if (code >= 400) return "客户端错误";
  return "成功";
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export function LogsPage() {
  const toast = useToast();
  const [logs, setLogs] = useState<RequestLog[]>([]);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [loading, setLoading] = useState(true);

  const [keyword, setKeyword] = useState("");
  const [channelFilter, setChannelFilter] = useState("all");
  const [modelFilter, setModelFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState("all");

  const [clearOpen, setClearOpen] = useState(false);
  const [clearing, setClearing] = useState(false);

  const load = async () => {
    setLoading(true);
    try {
      const [logList, chList] = await Promise.all([logApi.list(), channelApi.list()]);
      setLogs(logList);
      setChannels(chList);
    } catch (e) {
      console.error("Failed to load logs:", e);
      toast.error("日志加载失败");
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  const modelOptions = useMemo(() => {
    const set = new Set(logs.map((l) => l.model));
    return Array.from(set).sort();
  }, [logs]);

  const filtered = useMemo(() => {
    const kw = keyword.trim().toLowerCase();
    return logs.filter((l) => {
      if (channelFilter !== "all" && l.channel_name !== channelFilter) return false;
      if (modelFilter !== "all" && l.model !== modelFilter) return false;
      if (statusFilter !== "all" && !String(l.status_code).startsWith(statusFilter)) return false;
      if (kw) {
        const haystack = [
          l.model,
          l.channel_name ?? "",
          l.api_key_name ?? "",
          l.error_message ?? "",
          l.upstream_model ?? "",
        ]
          .join(" ")
          .toLowerCase();
        if (!haystack.includes(kw)) return false;
      }
      return true;
    });
  }, [logs, keyword, channelFilter, modelFilter, statusFilter]);

  const handleClear = async () => {
    setClearing(true);
    try {
      await logApi.clear();
      toast.success("日志已清空");
      setClearOpen(false);
      await load();
    } catch (e) {
      console.error("Failed to clear logs:", e);
      toast.error("清空失败");
    } finally {
      setClearing(false);
    }
  };

  const resetFilters = () => {
    setKeyword("");
    setChannelFilter("all");
    setModelFilter("all");
    setStatusFilter("all");
  };

  const hasFilter =
    keyword.trim() !== "" ||
    channelFilter !== "all" ||
    modelFilter !== "all" ||
    statusFilter !== "all";

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">请求日志</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            逐条请求明细：状态、Token、延迟
            {filtered.length !== logs.length && (
              <span className="ml-1 font-mono text-xs">
                （{filtered.length}/{logs.length}）
              </span>
            )}
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={load} disabled={loading}>
            <RefreshCw className={loading ? "animate-spin" : ""} />
            刷新
          </Button>
          <Button
            variant="outline"
            size="sm"
            className="text-destructive hover:bg-destructive/10 hover:text-destructive"
            onClick={() => setClearOpen(true)}
          >
            <Trash2 />
            清空
          </Button>
        </div>
      </div>

      {/* 筛选栏 */}
      <div className="mt-6 flex flex-wrap items-center gap-2">
        <div className="relative">
          <Search className="absolute top-1/2 left-2.5 h-4 w-4 -translate-y-1/2 text-muted-foreground" />
          <Input
            className="w-64 pl-8"
            placeholder="关键词：模型 / 渠道 / 错误信息"
            value={keyword}
            onChange={(e) => setKeyword(e.target.value)}
          />
        </div>
        <Select className="w-40" value={channelFilter} onChange={(e) => setChannelFilter(e.target.value)}>
          <option value="all">全部渠道</option>
          {channels.map((c) => (
            <option key={c.id} value={c.name}>
              {c.name}
            </option>
          ))}
        </Select>
        <Select className="w-44" value={modelFilter} onChange={(e) => setModelFilter(e.target.value)}>
          <option value="all">全部模型</option>
          {modelOptions.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </Select>
        <Select className="w-32" value={statusFilter} onChange={(e) => setStatusFilter(e.target.value)}>
          <option value="all">全部状态</option>
          <option value="2">2xx 成功</option>
          <option value="4">4xx 客户端</option>
          <option value="5">5xx 服务端</option>
        </Select>
      </div>

      {/* 日志表格 */}
      <Card className="mt-4 overflow-hidden">
        <CardContent className="pt-2">
          {loading ? (
            <div className="space-y-2 py-4">
              {Array.from({ length: 6 }).map((_, i) => (
                <Skeleton key={i} className="h-10 w-full" />
              ))}
            </div>
          ) : filtered.length === 0 ? (
            <EmptyState
              icon={ScrollText}
              title={hasFilter ? "没有匹配的日志" : "暂无请求日志"}
              description={
                hasFilter
                  ? "试着放宽筛选条件，或清除筛选后查看全部记录。"
                  : "网关开始代理请求后，这里会逐条记录每次调用的明细。"
              }
              action={
                hasFilter ? (
                  <Button size="sm" variant="outline" onClick={resetFilters}>
                    清除筛选条件
                  </Button>
                ) : undefined
              }
            />
          ) : (
            <Table>
              <TableHeader>
                <TableRow className="hover:bg-transparent">
                  <TableHead>时间</TableHead>
                  <TableHead>渠道</TableHead>
                  <TableHead>模型</TableHead>
                  <TableHead>状态</TableHead>
                  <TableHead className="text-right">Tokens</TableHead>
                  <TableHead className="text-right">延迟</TableHead>
                  <TableHead>标记</TableHead>
                </TableRow>
              </TableHeader>
              <TableBody>
                {filtered.map((l) => (
                  <TableRow key={l.id}>
                    <TableCell className="whitespace-nowrap text-muted-foreground">
                      {formatTime(l.created_at)}
                    </TableCell>
                    <TableCell>{l.channel_name ?? "-"}</TableCell>
                    <TableCell>
                      <span className="font-mono text-xs">{l.model}</span>
                    </TableCell>
                    <TableCell>
                      <StatusBadge tone={statusTone(l.status_code)}>
                        {l.status_code} {statusLabel(l.status_code)}
                      </StatusBadge>
                    </TableCell>
                    <TableCell className="text-right font-mono text-xs tabular-nums">
                      {l.total_tokens > 0 ? (
                        <span title={`P:${l.prompt_tokens} / C:${l.completion_tokens}`}>
                          {l.total_tokens.toLocaleString()}
                        </span>
                      ) : (
                        "-"
                      )}
                    </TableCell>
                    <TableCell className="text-right font-mono text-xs tabular-nums">
                      {l.duration_ms}ms
                    </TableCell>
                    <TableCell>
                      <div className="flex gap-1">
                        {l.is_stream && <Badge variant="outline">流式</Badge>}
                        {l.is_retry && <Badge variant="warning">重试</Badge>}
                        {l.security_action === "block" && (
                          <Badge variant="destructive">拦截</Badge>
                        )}
                      </div>
                    </TableCell>
                  </TableRow>
                ))}
              </TableBody>
            </Table>
          )}
        </CardContent>
      </Card>

      {/* 清空确认 Dialog */}
      <Dialog open={clearOpen} onOpenChange={setClearOpen}>
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              清空请求日志
            </DialogTitle>
            <DialogDescription>
              确认清空全部请求日志？此操作不可恢复，审计事件不受影响。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setClearOpen(false)}>
              取消
            </Button>
            <Button variant="destructive" onClick={handleClear} disabled={clearing}>
              {clearing ? "清空中..." : "确认清空"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
