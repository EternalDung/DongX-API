import { Fragment, useEffect, useMemo, useState } from "react";
import {
  Search,
  RefreshCw,
  ScrollText,
  Trash2,
  AlertTriangle,
  ChevronDown,
  ChevronRight,
  ChevronUp,
  Copy,
  Check,
  User as UserIcon,
  Bot,
  Lightbulb,
  ShieldAlert,
  ShieldCheck,
} from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Select } from "@/components/ui/select";
import { Skeleton } from "@/components/ui/skeleton";
import { EmptyState } from "@/components/ui/empty-state";
import { Badge } from "@/components/ui/badge";
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
import { cn } from "@/lib/utils";
import type { RequestLog, Channel, SecurityFinding } from "@/types";

const PAGE_SIZE = 20;

function statusTone(code: number): StatusTone {
  if (code >= 500) return "destructive";
  if (code >= 400) return "warning";
  return "success";
}

// ─── 安全审计展示（列表行聚合 + 明细逐条） ────────────────────────────────────
// 两层粒度：列表行只显示最高等级徽章；展开明细里逐条列出全部 findings，
// 每条自带自己的 severity。MAX 只决定动作与汇总徽章，不决定展示条数。

type RiskVariant = "destructive" | "warning" | "outline" | "secondary";

/** 风险等级 → 徽章文案与样式。汇总头与逐条明细共用同一套色板。 */
const RISK_META: Record<
  string,
  { label: string; variant: RiskVariant; className?: string }
> = {
  critical: { label: "严重", variant: "destructive" },
  high: { label: "高风险", variant: "warning" },
  medium: {
    label: "中风险",
    variant: "outline",
    className: "border-warning/40 text-warning",
  },
  low: { label: "低风险", variant: "secondary" },
  info: { label: "提示", variant: "secondary" },
  none: { label: "安全", variant: "secondary" },
};

/** 闸门动作 → 徽章文案与样式（与后端 SecurityAction::as_str 对齐）。 */
const ACTION_META: Record<string, { label: string; variant: RiskVariant }> = {
  allow: { label: "放行", variant: "secondary" },
  warn: { label: "告警", variant: "warning" },
  redact: { label: "脱敏", variant: "warning" },
  block: { label: "阻断", variant: "destructive" },
};

function riskMeta(level: string) {
  return RISK_META[level] ?? RISK_META.none;
}

function RiskBadge({
  level,
  score,
  className,
}: {
  level: string;
  score?: number;
  className?: string;
}) {
  const meta = riskMeta(level);
  return (
    <Badge variant={meta.variant} className={cn("gap-1", meta.className, className)}>
      <ShieldAlert className="h-3 w-3" />
      {meta.label}
      {score ? ` ${score}` : ""}
    </Badge>
  );
}

function formatTime(iso: string): string {
  const d = new Date(iso);
  if (isNaN(d.getTime())) return iso;
  const pad = (n: number) => String(n).padStart(2, "0");
  return `${pad(d.getMonth() + 1)}-${pad(d.getDate())} ${pad(d.getHours())}:${pad(d.getMinutes())}:${pad(d.getSeconds())}`;
}

function formatDuration(ms: number): string {
  if (ms < 1000) return `${ms}ms`;
  return `${(ms / 1000).toFixed(2)}s`;
}

export function LogsPage() {
  const toast = useToast();
  const [logs, setLogs] = useState<RequestLog[]>([]);
  const [channels, setChannels] = useState<Channel[]>([]);
  const [loading, setLoading] = useState(true);
  const [page, setPage] = useState(0); // 0-indexed
  const [expandedId, setExpandedId] = useState<string | null>(null);

  const [keyword, setKeyword] = useState("");
  const [channelFilter, setChannelFilter] = useState("all");
  const [modelFilter, setModelFilter] = useState("all");
  const [statusFilter, setStatusFilter] = useState("all");

  const [clearOpen, setClearOpen] = useState(false);
  const [clearing, setClearing] = useState(false);

  const [deleteTarget, setDeleteTarget] = useState<RequestLog | null>(null);
  const [deleting, setDeleting] = useState(false);

  const handleDelete = async () => {
    if (!deleteTarget) return;
    setDeleting(true);
    try {
      await logApi.delete(deleteTarget.id);
      toast.success("日志已删除");
      setDeleteTarget(null);
      setExpandedId(null);
      await load(page);
    } catch (e) {
      console.error("Failed to delete log:", e);
      toast.error("删除失败");
    } finally {
      setDeleting(false);
    }
  };

  // Load one page from the backend with current filters.
  const load = async (p: number) => {
    setLoading(true);
    try {
      const list = await logApi.list({
        keyword: keyword.trim() || undefined,
        channel_name: channelFilter !== "all" ? channelFilter : undefined,
        model: modelFilter !== "all" ? modelFilter : undefined,
        page: p + 1, // backend is 1-indexed
        page_size: PAGE_SIZE,
      });
      setLogs(list);
      setExpandedId(null);
    } catch (e) {
      console.error("Failed to load logs:", e);
      toast.error("日志加载失败");
    } finally {
      setLoading(false);
    }
  };

  // Load channels once (for the filter dropdown).
  useEffect(() => {
    channelApi.list().then(setChannels).catch(() => {});
  }, []);

  // Reload (reset to page 0) when keyword / channel / model filters change.
  // Debounced so typing in the keyword box doesn't spam the backend.
  useEffect(() => {
    const t = setTimeout(() => {
      setPage(0);
      load(0);
    }, 300);
    return () => clearTimeout(t);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [keyword, channelFilter, modelFilter]);

  const modelOptions = useMemo(
    () => Array.from(new Set(channels.flatMap((c) => c.models))).sort(),
    [channels],
  );

  // status filter is applied client-side on the current page.
  const filtered = useMemo(() => {
    if (statusFilter === "all") return logs;
    return logs.filter((l) => String(l.status_code).startsWith(statusFilter));
  }, [logs, statusFilter]);

  const handleClear = async () => {
    setClearing(true);
    try {
      await logApi.clear();
      toast.success("日志已清空");
      setClearOpen(false);
      setPage(0);
      await load(0);
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

  const goPrev = () => {
    const p = Math.max(0, page - 1);
    setPage(p);
    load(p);
  };
  const goNext = () => {
    const p = page + 1;
    setPage(p);
    load(p);
  };

  return (
    <div>
      <div className="flex items-center justify-between">
        <div>
          <h1 className="text-2xl font-semibold tracking-tight">请求日志</h1>
          <p className="mt-1 text-sm text-muted-foreground">
            逐条请求明细：状态、Token、耗时
            {filtered.length !== logs.length && (
              <span className="ml-1 font-mono text-xs">
                （{filtered.length}/{logs.length}）
              </span>
            )}
          </p>
        </div>
        <div className="flex gap-2">
          <Button variant="outline" size="sm" onClick={() => load(page)} disabled={loading}>
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
        <CardContent className="p-0">
          {loading ? (
            <div className="space-y-2 p-4">
              {Array.from({ length: 8 }).map((_, i) => (
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
            <>
              <Table>
                <TableHeader>
                  <TableRow className="hover:bg-transparent">
                    <TableHead className="w-8" />
                    <TableHead className="w-12 text-center">序号</TableHead>
                    <TableHead>时间</TableHead>
                    <TableHead>密钥</TableHead>
                    <TableHead>上游</TableHead>
                    <TableHead>模型</TableHead>
                    <TableHead>状态</TableHead>
                    <TableHead>风险</TableHead>
                    <TableHead className="text-right">Tokens</TableHead>
                    <TableHead className="text-right">耗时</TableHead>
                    <TableHead className="w-10 text-center">操作</TableHead>
                  </TableRow>
                </TableHeader>
                <TableBody>
                  {filtered.map((l) => {
                    const expanded = expandedId === l.id;
                    return (
                      <Fragment key={l.id}>
                        <TableRow
                          className="cursor-pointer"
                          onClick={() => setExpandedId(expanded ? null : l.id)}
                        >
                          <TableCell className="w-8">
                            {expanded ? (
                              <ChevronDown className="h-4 w-4 text-muted-foreground" />
                            ) : (
                              <ChevronRight className="h-4 w-4 text-muted-foreground" />
                            )}
                          </TableCell>
                          <TableCell className="w-12 text-center font-mono text-xs text-muted-foreground tabular-nums">
                            {l.seq ?? "-"}
                          </TableCell>
                          <TableCell className="whitespace-nowrap text-muted-foreground">
                            {formatTime(l.created_at)}
                          </TableCell>
                          <TableCell>{l.api_key_name ?? "-"}</TableCell>
                          <TableCell>{l.channel_name ?? "-"}</TableCell>
                          <TableCell>
                            <span className="font-mono text-xs">{l.model}</span>
                          </TableCell>
                          <TableCell>
                            <StatusBadge tone={statusTone(l.status_code)}>
                              {l.status_code}
                            </StatusBadge>
                          </TableCell>
                          <TableCell>
                            {l.risk_level && l.risk_level !== "none" ? (
                              <RiskBadge level={l.risk_level} score={l.risk_score} />
                            ) : (
                              <span className="text-xs text-muted-foreground">-</span>
                            )}
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
                            {formatDuration(l.duration_ms)}
                          </TableCell>
                          <TableCell className="w-10 text-center">
                            <button
                              type="button"
                              title="删除此日志"
                              onClick={(e) => {
                                e.stopPropagation();
                                setDeleteTarget(l);
                              }}
                              className="inline-flex h-7 w-7 items-center justify-center rounded-md text-muted-foreground transition-colors hover:bg-destructive/10 hover:text-destructive"
                            >
                              <Trash2 className="h-4 w-4" />
                            </button>
                          </TableCell>
                        </TableRow>
                        {expanded && (
                          <TableRow className="hover:bg-transparent">
                            <TableCell colSpan={11} className="bg-muted/30 p-4">
                              <LogDetail id={l.id} />
                            </TableCell>
                          </TableRow>
                        )}
                      </Fragment>
                    );
                  })}
                </TableBody>
              </Table>

              {/* 分页 */}
              <div className="flex items-center justify-between border-t px-4 py-2.5">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={goPrev}
                  disabled={page === 0 || loading}
                >
                  上一页
                </Button>
                <span className="text-sm text-muted-foreground">第 {page + 1} 页</span>
                <Button
                  variant="outline"
                  size="sm"
                  onClick={goNext}
                  disabled={logs.length < PAGE_SIZE || loading}
                >
                  下一页
                </Button>
              </div>
            </>
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

      {/* 删除单条日志确认 Dialog */}
      <Dialog
        open={deleteTarget !== null}
        onOpenChange={(o) => {
          if (!o) setDeleteTarget(null);
        }}
      >
        <DialogContent className="sm:max-w-md">
          <DialogHeader>
            <DialogTitle className="flex items-center gap-2">
              <AlertTriangle className="h-5 w-5 text-destructive" />
              删除日志
            </DialogTitle>
            <DialogDescription>
              确认删除序号 {deleteTarget?.seq ?? ""} 的这条请求日志？此操作不可恢复。
            </DialogDescription>
          </DialogHeader>
          <DialogFooter>
            <Button variant="outline" onClick={() => setDeleteTarget(null)}>
              取消
            </Button>
            <Button variant="destructive" onClick={handleDelete} disabled={deleting}>
              {deleting ? "删除中..." : "确认删除"}
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}

// ─── 展开明细卡片 ────────────────────────────────────────────────────────────
// 点击列表行后展开，调用 logApi.detail(id) 取完整行（含 request/response body）。

function LogDetail({ id }: { id: string }) {
  const [detail, setDetail] = useState<RequestLog | null>(null);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    setLoading(true);
    logApi
      .detail(id)
      .then((d) => {
        if (!cancelled) setDetail(d);
      })
      .catch(() => {
        if (!cancelled) setDetail(null);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [id]);

  if (loading) {
    return <Skeleton className="h-32 w-full" />;
  }
  if (!detail) {
    return <div className="text-sm text-muted-foreground">加载详情失败</div>;
  }

  const modelMapping =
    detail.upstream_model && detail.upstream_model !== detail.model
      ? `${detail.model} → ${detail.upstream_model}`
      : null;

  return (
    <div className="space-y-4">
      {/* 基本信息：卡片网格，每行多个；列表已展示的字段不再重复 */}
      <div className="grid grid-cols-2 gap-2 sm:grid-cols-3">
        <StatCard label="模式" value={modeLabel(detail.mode)} />
        <StatCard label="状态">
          <StatusBadge tone={statusTone(detail.status_code)}>
            {detail.status_code}
          </StatusBadge>
        </StatCard>
        <StatCard label="耗时" value={formatDuration(detail.duration_ms)} />
        <StatCard label="模型映射" mono>
          {modelMapping ?? detail.model}
        </StatCard>
        <StatCard label="输入 (Prompt)" value={String(detail.prompt_tokens)} />
        <StatCard label="输出 (Completion)" value={String(detail.completion_tokens)} />
        <StatCard label="总计 (Total)" value={String(detail.total_tokens)} />
        <StatCard label="流式" value={detail.is_stream ? "是" : "否"} />
        <StatCard label="重试" value={detail.is_retry ? "是" : "否"} />
      </div>

      {/* 错误信息 */}
      {detail.error_message && (
        <div className="rounded-md border border-destructive/30 bg-destructive/5 p-3 text-sm text-destructive">
          <span className="font-medium">错误：</span> {detail.error_message}
        </div>
      )}

      {/* 安全审计：汇总头（只取最高等级）+ 逐条明细（全部 findings） */}
      <SecurityAuditSection detail={detail} />

      {/* 请求/响应 tab + 缩略/JSON 双视图 */}
      <BodySection requestBody={detail.request_body} responseBody={detail.response_body} />
    </div>
  );
}

// ─── 安全审计明细区块 ────────────────────────────────────────────────────────
// 汇总头展示 MAX 决策结果（risk_level / security_action / blocked_reason），
// 下方逐条列出**全部** findings，每条自带独立 severity，不折叠、不取最高级。
// findings 仅在 risk_score > 0 时懒加载，避免列表展开即触发 N+1 查询。

function SecurityAuditSection({ detail }: { detail: RequestLog }) {
  const [findings, setFindings] = useState<SecurityFinding[]>([]);
  const [loading, setLoading] = useState(false);

  const hasRisk = detail.risk_score > 0 || !!detail.risk_summary;

  useEffect(() => {
    if (detail.risk_score <= 0) {
      setFindings([]);
      return;
    }
    let cancelled = false;
    setLoading(true);
    logApi
      .securityFindings(detail.id)
      .then((fs) => {
        if (!cancelled) setFindings(fs);
      })
      .catch(() => {
        if (!cancelled) setFindings([]);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, [detail.id, detail.risk_score]);

  if (!hasRisk) {
    return (
      <div className="flex items-center gap-2 rounded-lg border bg-background px-3 py-2 text-sm text-muted-foreground">
        <ShieldCheck className="h-4 w-4 text-success" />
        安全审计：未发现风险
      </div>
    );
  }

  const actionMeta = ACTION_META[detail.security_action] ?? ACTION_META.allow;

  return (
    <div className="rounded-lg border bg-background p-3">
      <div className="flex flex-wrap items-center gap-2">
        <ShieldAlert className="h-4 w-4 text-warning" />
        <span className="text-sm font-medium">安全审计</span>
        <RiskBadge level={detail.risk_level} score={detail.risk_score} />
        <Badge variant={actionMeta.variant}>动作：{actionMeta.label}</Badge>
        {detail.sanitized && <Badge variant="secondary">已脱敏</Badge>}
      </div>

      {detail.risk_summary && (
        <p className="mt-2 text-xs text-muted-foreground">{detail.risk_summary}</p>
      )}
      {detail.blocked_reason && (
        <p className="mt-1 text-xs text-destructive">
          阻断原因：{detail.blocked_reason}
        </p>
      )}

      <div className="mt-3">
        {loading ? (
          <Skeleton className="h-16 w-full" />
        ) : findings.length > 0 ? (
          <div className="max-h-[240px] space-y-2 overflow-y-auto pr-1">
            {findings.map((f) => (
              <FindingRow key={f.id} finding={f} />
            ))}
          </div>
        ) : (
          <p className="text-xs text-muted-foreground">无命中明细</p>
        )}
      </div>
    </div>
  );
}

/** 单条 finding：用自己的 severity（不是汇总的最高等级）+ 阶段 + 规则 + 证据。 */
function FindingRow({ finding }: { finding: SecurityFinding }) {
  const meta = riskMeta(finding.severity);
  return (
    <div className="rounded-lg border bg-muted/30 px-3 py-2">
      <div className="flex flex-wrap items-center gap-2">
        <Badge variant={meta.variant} className={cn(meta.className)}>
          {meta.label}
        </Badge>
        <span className="text-sm font-medium">{finding.title}</span>
        <Badge variant="outline" className="text-xs">
          {finding.phase === "response" ? "响应侧" : "请求侧"}
        </Badge>
        <span className="font-mono text-xs text-muted-foreground">
          {finding.rule_id}
        </span>
      </div>
      {finding.description && (
        <p className="mt-1 text-xs text-muted-foreground">{finding.description}</p>
      )}
      {(finding.location || finding.evidence_masked) && (
        <div className="mt-1 flex flex-wrap gap-x-3 gap-y-1 text-xs text-muted-foreground">
          {finding.location && <span>位置：{finding.location}</span>}
          {finding.evidence_masked && <span>证据：{finding.evidence_masked}</span>}
        </div>
      )}
    </div>
  );
}

// 参数卡片（详情卡用，网格每行多个；列表已展示的字段不再重复）
function StatCard({
  label,
  value,
  mono,
  children,
}: {
  label: string;
  value?: string;
  mono?: boolean;
  children?: React.ReactNode;
}) {
  return (
    <div className="rounded-lg border bg-background px-3 py-2">
      <div className="text-[11px] uppercase tracking-wide text-muted-foreground">
        {label}
      </div>
      <div className={`mt-0.5 text-sm ${mono ? "font-mono" : ""}`}>
        {children ?? value ?? "-"}
      </div>
    </div>
  );
}

function modeLabel(mode: string): string {
  switch (mode) {
    case "chat":
      return "对话 (chat)";
    case "completion":
      return "补全 (completion)";
    case "embedding":
      return "向量 (embedding)";
    default:
      return mode || "-";
  }
}

// ─── 请求/响应 Tab + 双视图 ──────────────────────────────────────────────────

type BodyTab = "request" | "response";
type ViewMode = "preview" | "json";

function BodySection({
  requestBody,
  responseBody,
}: {
  requestBody: string | null;
  responseBody: string | null;
}) {
  const [tab, setTab] = useState<BodyTab>("response");
  const [view, setView] = useState<ViewMode>("preview");
  const reqMessages = useMemo(() => parseRequestMessages(requestBody), [requestBody]);
  const respChoices = useMemo(() => parseResponseChoices(responseBody), [responseBody]);

  const currentBody = tab === "request" ? requestBody : responseBody;
  const currentList = tab === "request" ? reqMessages : respChoices;
  const currentCount = currentList.length;

  return (
    <div className="overflow-hidden rounded-md border bg-background">
      {/* Tab bar */}
      <div className="flex items-center justify-between border-b bg-muted/30">
        <div className="flex">
          <TabButton
            active={tab === "request"}
            onClick={() => {
              setTab("request");
              setView("preview");
            }}
            color="blue"
          >
            请求
            {reqMessages.length > 0 && (
              <span className="ml-1 rounded-full bg-blue-500/15 px-1.5 font-mono text-[10px] text-blue-600">
                {reqMessages.length}
              </span>
            )}
          </TabButton>
          <TabButton
            active={tab === "response"}
            onClick={() => {
              setTab("response");
              setView("preview");
            }}
            color="emerald"
          >
            响应
            {respChoices.length > 0 && (
              <span className="ml-1 rounded-full bg-emerald-500/15 px-1.5 font-mono text-[10px] text-emerald-600">
                {respChoices.length}
              </span>
            )}
          </TabButton>
        </div>
        {currentCount > 0 && (
          <button
            type="button"
            onClick={() => setView(view === "preview" ? "json" : "preview")}
            className="mr-3 text-xs font-medium text-primary hover:underline"
          >
            {view === "preview" ? "查看原始 JSON" : "返回对话视图"}
          </button>
        )}
      </div>

      {/* Body */}
      <div className="p-3">
        {!currentBody ? (
          <div className="rounded-md border border-dashed p-3 text-xs italic text-muted-foreground">
            未记录原始报文（在「设置 → 通用」开启"记录原始报文"后生效）
          </div>
        ) : view === "preview" ? (
          currentList.length > 0 ? (
            tab === "request" ? (
              <MessageList messages={currentList as ParsedMessage[]} />
            ) : (
              <ChoiceList choices={currentList as ParsedChoice[]} />
            )
          ) : (
            <div className="rounded-md border border-dashed p-3 text-xs italic text-muted-foreground">
              无法解析对话结构，自动回退 JSON 视图
            </div>
          )
        ) : (
          <JsonBlock body={currentBody} />
        )}
      </div>
    </div>
  );
}

function TabButton({
  active,
  onClick,
  color,
  children,
}: {
  active: boolean;
  onClick: () => void;
  color: "blue" | "emerald";
  children: React.ReactNode;
}) {
  const activeCls =
    color === "blue"
      ? "text-blue-600 border-blue-500"
      : "text-emerald-600 border-emerald-500";
  const inactiveCls = "text-muted-foreground hover:text-foreground border-transparent";
  return (
    <button
      type="button"
      onClick={onClick}
      className={`relative flex items-center px-4 py-2 text-sm font-medium transition-colors ${active ? activeCls : inactiveCls}`}
    >
      {children}
      {active && (
        <span
          className={`absolute right-0 bottom-0 left-0 h-0.5 ${color === "blue" ? "bg-blue-500" : "bg-emerald-500"}`}
        />
      )}
    </button>
  );
}

// ─── 解析器 ─────────────────────────────────────────────────────────────────

interface ParsedMessage {
  role: string;
  content: string;
}

interface ParsedChoice {
  role: string;
  content: string;
  reasoning: string;
}

// 把 `data: {...}\n\n` 形式的 SSE 报文拆成 JSON 对象数组（跳过 [DONE] 与非 JSON 行）。
// 非 SSE 的单条 JSON 返回空数组，由调用方走单对象解析分支。
function extractSsePayloads(body: string): Record<string, unknown>[] {
  const out: Record<string, unknown>[] = [];
  for (const line of body.split("\n")) {
    const t = line.trim();
    if (!t.startsWith("data:")) continue;
    const data = t.slice(5).trim();
    if (!data || data === "[DONE]") continue;
    try {
      out.push(JSON.parse(data) as Record<string, unknown>);
    } catch {
      // 忽略无法解析的 data 行
    }
  }
  return out;
}

// 从 Responses 的 content 数组（[{type:"output_text",text}]）拼接纯文本
function extractResponsesText(content: unknown): string {
  if (typeof content === "string") return content;
  if (Array.isArray(content)) {
    return (content as Record<string, unknown>[])
      .filter((p) => p && (p.type === "output_text" || p.type === "input_text" || "text" in p))
      .map((p) => (typeof p.text === "string" ? p.text : ""))
      .join("");
  }
  return "";
}

// 请求体：同时支持 Chat 的 messages 与 Responses 的 input
function parseRequestMessages(body: string | null): ParsedMessage[] {
  if (!body) return [];
  try {
    const parsed = JSON.parse(body);
    // Chat 格式
    if (Array.isArray(parsed?.messages)) {
      return (parsed.messages as Record<string, unknown>[]).map((m) => ({
        role: (m.role as string) ?? "user",
        content:
          typeof m.content === "string"
            ? m.content
            : m.content
              ? JSON.stringify(m.content)
              : "",
      }));
    }
    // Responses 格式：input 可以是字符串或条目数组
    const input = parsed?.input;
    if (typeof input === "string") {
      return [{ role: "user", content: input }];
    }
    if (Array.isArray(input)) {
      return (input as Record<string, unknown>[]).map((it) => {
        const role = (it.role as string) ?? "user";
        let content = "";
        if (typeof it.content === "string") content = it.content;
        else if (Array.isArray(it.content)) content = extractResponsesText(it.content);
        else if (it.content) content = JSON.stringify(it.content);
        return { role, content };
      });
    }
    return [];
  } catch {
    return [];
  }
}

// 响应体：同时支持 Chat 与 Responses，流式 SSE 与单条 JSON
function parseResponseChoices(body: string | null): ParsedChoice[] {
  if (!body) return [];

  // 1) 先尝试按 SSE 流式帧解析（Chat 与 Responses 流式都走这里）
  const frames = extractSsePayloads(body);
  if (frames.length > 0) {
    const choice = aggregateFrames(frames);
    return choice ? [choice] : [];
  }

  // 2) 单条 JSON
  try {
    const parsed = JSON.parse(body);
    // Chat 格式
    if (Array.isArray(parsed?.choices)) {
      return (parsed.choices as Record<string, unknown>[]).map((c) => {
        const message = (c.message ?? c.delta ?? {}) as Record<string, unknown>;
        return {
          role: (message.role as string) ?? "assistant",
          content: typeof message.content === "string" ? (message.content as string) : "",
          reasoning:
            typeof message.reasoning_content === "string"
              ? (message.reasoning_content as string)
              : "",
        };
      });
    }
    // Responses 非流式格式：output 数组
    if (Array.isArray(parsed?.output)) {
      return parseResponsesOutput(parsed.output as Record<string, unknown>[]);
    }
  } catch {
    return [];
  }
  return [];
}

// 把解析出的 Responses output 数组转成对话视图（支持 text / reasoning）
function parseResponsesOutput(output: Record<string, unknown>[]): ParsedChoice[] {
  let content = "";
  let reasoning = "";
  for (const item of output) {
    if (item.type === "message") {
      const txt = extractResponsesText(item.content);
      if (txt) content = txt;
    } else if (item.type === "reasoning") {
      const summary = (item.summary as Record<string, unknown>[] | undefined)?.[0];
      const txt = typeof summary?.text === "string" ? (summary.text as string) : "";
      if (txt) reasoning = txt;
    }
  }
  if (!content && !reasoning) return [];
  return [{ role: "assistant", content, reasoning }];
}

// 聚合一组合并的 SSE 帧（Chat chunk 或 Responses 事件）成单条选择
function aggregateFrames(frames: Record<string, unknown>[]): ParsedChoice | null {
  let content = "";
  let reasoning = "";
  for (const f of frames) {
    const type = f.type as string | undefined;
    if (!type) {
      // Chat chunk：choices[].delta
      const choices = f.choices as Record<string, unknown>[] | undefined;
      if (Array.isArray(choices)) {
        for (const c of choices) {
          const d = (c.delta ?? {}) as Record<string, unknown>;
          if (typeof d.content === "string") content += d.content;
          if (typeof d.reasoning_content === "string") reasoning += d.reasoning_content;
        }
      }
      continue;
    }
    // Responses 事件
    if (type === "response.output_text.delta") {
      if (typeof f.delta === "string") content += f.delta as string;
    } else if (type === "response.reasoning_summary_text.delta") {
      if (typeof f.delta === "string") reasoning += f.delta as string;
    } else if (type === "response.output_item.done") {
      const item = f.item as Record<string, unknown> | undefined;
      if (item?.type === "message") {
        const txt = extractResponsesText(item.content);
        if (txt) content = content || txt;
      } else if (item?.type === "reasoning") {
        const summary = (item.summary as Record<string, unknown>[] | undefined)?.[0];
        const txt = typeof summary?.text === "string" ? (summary.text as string) : "";
        if (txt) reasoning = reasoning || txt;
      }
    } else if (type === "response.completed") {
      const out = (f.response as Record<string, unknown> | undefined)?.output as
        | Record<string, unknown>[]
        | undefined;
      if (Array.isArray(out)) {
        for (const item of out) {
          if (item?.type === "message") {
            const txt = extractResponsesText(item.content);
            if (txt) content = content || txt;
          } else if (item?.type === "reasoning") {
            const summary = (item.summary as Record<string, unknown>[] | undefined)?.[0];
            const txt = typeof summary?.text === "string" ? (summary.text as string) : "";
            if (txt) reasoning = reasoning || txt;
          }
        }
      }
    }
  }
  if (!content && !reasoning) return null;
  return { role: "assistant", content, reasoning };
}

// ─── 对话视图（请求） ────────────────────────────────────────────────────────

function MessageList({ messages }: { messages: ParsedMessage[] }) {
  return (
    <div className="space-y-3">
      {messages.map((m, i) => (
        <MessageBubble key={i} index={i} role={m.role} content={m.content} />
      ))}
    </div>
  );
}

function MessageBubble({
  index,
  role,
  content,
  reasoning,
}: {
  index: number;
  role: string;
  content: string;
  reasoning?: string;
}) {
  const isUser = role === "user";
  const Icon = isUser ? UserIcon : Bot;
  const roleLabel = isUser ? "User" : role === "system" ? "System" : role === "assistant" ? "AI" : role;
  const bubbleCls = isUser
    ? "border-blue-200 bg-blue-50 dark:border-blue-900 dark:bg-blue-950/30"
    : role === "system"
      ? "border-amber-200 bg-amber-50 dark:border-amber-900 dark:bg-amber-950/30"
      : "border-emerald-200 bg-emerald-50 dark:border-emerald-900 dark:bg-emerald-950/30";

  return (
    <div className="flex gap-2">
      <div className="mt-0.5 flex h-6 w-6 shrink-0 items-center justify-center rounded-full bg-muted text-muted-foreground">
        <Icon className="h-3.5 w-3.5" />
      </div>
      <div className="flex-1 space-y-1.5">
        <div className="flex items-center gap-2 text-xs text-muted-foreground">
          <span className="font-medium">{roleLabel}</span>
          <span className="font-mono text-[10px]">#{index + 1}</span>
        </div>
        {reasoning !== undefined && <ReasoningBlock reasoning={reasoning} />}
        <div className={`rounded-md border p-3 text-sm whitespace-pre-wrap break-words ${bubbleCls}`}>
          {content || <span className="italic text-muted-foreground">（空内容）</span>}
        </div>
      </div>
    </div>
  );
}

// ─── 对话视图（响应） ────────────────────────────────────────────────────────

function ChoiceList({ choices }: { choices: ParsedChoice[] }) {
  return (
    <div className="space-y-3">
      {choices.map((c, i) => (
        <MessageBubble
          key={i}
          index={i}
          role={c.role}
          content={c.content}
          reasoning={c.reasoning}
        />
      ))}
    </div>
  );
}

function ReasoningBlock({ reasoning }: { reasoning: string }) {
  const [expanded, setExpanded] = useState(false);
  if (!reasoning) return null;
  const preview = reasoning.replace(/\n+/g, " ").trim();
  const truncated = preview.length > 200 ? `${preview.slice(0, 200)}…` : preview;
  const shouldFold = preview.length > 200;

  return (
    <div className="rounded-md border border-purple-200 bg-purple-50 dark:border-purple-900 dark:bg-purple-950/30">
      <div className="flex items-center justify-between border-b border-purple-200/60 px-3 py-1.5">
        <div className="flex items-center gap-1.5 text-xs font-medium text-purple-700 dark:text-purple-300">
          <Lightbulb className="h-3.5 w-3.5" />
          推理内容
        </div>
        <div className="flex items-center gap-1">
          <CopyButton value={reasoning} />
          {shouldFold && (
            <button
              type="button"
              onClick={() => setExpanded(!expanded)}
              className="flex items-center gap-0.5 rounded-full px-2 py-0.5 text-[10px] font-medium text-purple-700 hover:bg-purple-100 dark:hover:bg-purple-900/40"
            >
              {expanded ? (
                <>
                  <ChevronUp className="h-3 w-3" />
                  收起
                </>
              ) : (
                <>
                  <ChevronDown className="h-3 w-3" />
                  展开
                </>
              )}
            </button>
          )}
        </div>
      </div>
      <div className="px-3 py-2 text-xs whitespace-pre-wrap break-words text-purple-900 dark:text-purple-200">
        {shouldFold && !expanded ? truncated : reasoning}
      </div>
    </div>
  );
}

// ─── 通用：复制按钮 / JSON 块 ────────────────────────────────────────────────

function CopyButton({ value }: { value: string }) {
  const [copied, setCopied] = useState(false);
  return (
    <button
      type="button"
      onClick={async () => {
        try {
          await navigator.clipboard.writeText(value);
          setCopied(true);
          setTimeout(() => setCopied(false), 1200);
        } catch {
          /* clipboard unavailable */
        }
      }}
      className={`flex items-center gap-0.5 rounded-full px-2 py-0.5 text-[10px] font-medium transition-colors ${
        copied
          ? "bg-emerald-100 text-emerald-700"
          : "text-muted-foreground hover:bg-muted hover:text-foreground"
      }`}
    >
      {copied ? <Check className="h-3 w-3" /> : <Copy className="h-3 w-3" />}
      {copied ? "已复制" : "复制"}
    </button>
  );
}

function JsonBlock({ body }: { body: string }) {
  const pretty = useMemo(() => {
    try {
      return JSON.stringify(JSON.parse(body), null, 2);
    } catch {
      return body;
    }
  }, [body]);
  return (
    <div className="relative">
      <pre className="max-h-96 overflow-auto rounded-md bg-muted/40 p-3 font-mono text-xs whitespace-pre-wrap break-words">
        {pretty}
      </pre>
      <div className="absolute top-2 right-2">
        <CopyButton value={pretty} />
      </div>
    </div>
  );
}