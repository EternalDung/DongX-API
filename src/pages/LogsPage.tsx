import { useEffect, useMemo, useState } from "react";
import {
  Search,
  RefreshCw,
  ScrollText,
  Trash2,
} from "lucide-react";
import {
  Card,
  CardContent,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Select } from "@/components/ui/select";
import {
  Table,
  TableBody,
  TableCell,
  TableHead,
  TableHeader,
  TableRow,
} from "@/components/ui/table";
import { logApi, channelApi } from "@/lib/api";
import type { RequestLog, Channel } from "@/types";

function statusVariant(code: number): "success" | "destructive" | "warning" {
  if (code >= 500) return "destructive";
  if (code >= 400) return "warning";
  return "success";
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

export function LogsPage() {
  const [logs, setLogs] = useState<RequestLog[]>([]);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [loading, setLoading] = useState(true);

  // 筛选条件
  const [keyword, setKeyword] = useState("");
  const [channelFilter, setChannelFilter] = useState("all");
  const [modelFilter, setModelFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState("all");

  const load = async () => {
    setLoading(true);
    try {
      const [logList, chList] = await Promise.all([logApi.list(), channelApi.list()]);
      setLogs(logList);
      setChannels(chList);
    } catch (e) {
      console.error("Failed to load logs:", e);
    } finally {
      setLoading(false);
    }
  };

  useEffect(() => {
    load();
  }, []);

  // 模型去重列表（用于下拉）
  const modelOptions = useMemo(() => {
    const set = new Set(logs.map((l) => l.model));
    return Array.from(set).sort();
  }, [logs]);

  // 客户端过滤（关键词跨字段模糊匹配）
  const filtered = useMemo(() => {
    const kw = keyword.trim().toLowerCase();
    return logs.filter((l) => {
      if (channelFilter !== "all" && l.channel_name !== channelFilter) return false;
      if (modelFilter !== "all" && l.model !== modelFilter) return false;
      if (statusFilter !== "all") {
        const prefix = statusFilter; // "2" "4" "5"
        if (!String(l.status_code).startsWith(prefix)) return false;
      }
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
    if (!window.confirm("确认清空全部请求日志？此操作不可恢复。")) return;
    try {
      await logApi.clear();
      await load();
    } catch (e) {
      console.error("Failed to clear logs:", e);
    }
  };

  const hasFilter =
    keyword.trim() !== "" ||
    channelFilter !== "all" ||
    modelFilter !== "all" ||
    statusFilter !== "all";

  return (
    <div className="p-6">
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-bold">请求日志</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            逐条请求明细：状态、Token、延迟
            {filtered.length !== logs.length && `（${filtered.length}/${logs.length}）`}
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
            className="text-destructive hover:text-destructive"
            onClick={handleClear}
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
        <Select
          className="w-40"
          value={channelFilter}
          onChange={(e) => setChannelFilter(e.target.value)}
        >
          <option value="all">全部渠道</option>
          {channels.map((c) => (
            <option key={c.id} value={c.name}>
              {c.name}
            </option>
          ))}
        </Select>
        <Select
          className="w-44"
          value={modelFilter}
          onChange={(e) => setModelFilter(e.target.value)}
        >
          <option value="all">全部模型</option>
          {modelOptions.map((m) => (
            <option key={m} value={m}>
              {m}
            </option>
          ))}
        </Select>
        <Select
          className="w-32"
          value={statusFilter}
          onChange={(e) => setStatusFilter(e.target.value)}
        >
          <option value="all">全部状态</option>
          <option value="2">2xx 成功</option>
          <option value="4">4xx 客户端</option>
          <option value="5">5xx 服务端</option>
        </Select>
      </div>

      {/* 日志表格 */}
      <Card className="mt-4">
        <CardContent className="pt-2">
          {loading ? (
            <div className="py-12 text-center text-sm text-muted-foreground">加载中...</div>
          ) : filtered.length === 0 ? (
            <div className="flex flex-col items-center gap-2 py-12 text-center">
              <ScrollText className="h-10 w-10 text-muted-foreground" />
              <p className="text-sm text-muted-foreground">
                {hasFilter ? "没有匹配的日志" : "暂无请求日志"}
              </p>
              {hasFilter && (
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => {
                    setKeyword("");
                    setChannelFilter("all");
                    setModelFilter("all");
                    setStatusFilter("all");
                  }}
                >
                  清除筛选条件
                </Button>
              )}
            </div>
          ) : (
            <Table>
              <TableHeader>
                <TableRow>
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
                    <TableCell className="text-muted-foreground">
                      {formatTime(l.created_at)}
                    </TableCell>
                    <TableCell>{l.channel_name ?? "-"}</TableCell>
                    <TableCell>
                      <span className="font-mono text-xs">{l.model}</span>
                    </TableCell>
                    <TableCell>
                      <Badge variant={statusVariant(l.status_code)}>{l.status_code}</Badge>
                    </TableCell>
                    <TableCell className="text-right font-mono text-xs">
                      {l.total_tokens > 0 ? (
                        <span title={`P:${l.prompt_tokens} / C:${l.completion_tokens}`}>
                          {l.total_tokens.toLocaleString()}
                        </span>
                      ) : (
                        "-"
                      )}
                    </TableCell>
                    <TableCell className="text-right font-mono text-xs">
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
    </div>
  );
}
