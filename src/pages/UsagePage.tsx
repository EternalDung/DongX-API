import { useEffect, useMemo, useState } from "react";
import {
  BookOpen,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
  Copy,
  Loader2,
  MessageSquare,
  Plug,
  Send,
  Sparkles,
  Terminal,
  Zap,
} from "lucide-react";
import {
  Card,
  CardContent,
  CardHeader,
  CardTitle,
} from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Badge } from "@/components/ui/badge";
import { Select } from "@/components/ui/select";
import { EmptyState } from "@/components/ui/empty-state";
import { useToast } from "@/components/ui/toast";
import { Skeleton } from "@/components/ui/skeleton";
import { channelApi, keyApi, settingsApi } from "@/lib/api";
import { cn } from "@/lib/utils";
import type { ApiKey, Channel, Settings } from "@/types";

// ============================================================
// 协议卡片数据（参考 waliapi 的 3 协议设计；MVP 仅 OpenAI Chat 可用）
// ============================================================
interface ProtocolDef {
  id: "chat" | "responses" | "anthropic";
  label: string;
  desc: string;
  endpoint: string;
  enabled: boolean;
  icon: typeof MessageSquare;
}

const PROTOCOLS: ProtocolDef[] = [
  {
    id: "chat",
    label: "OpenAI Chat",
    desc: "标准 Chat Completions 协议，广泛兼容",
    endpoint: "/chat/completions",
    enabled: true,
    icon: MessageSquare,
  },
  {
    id: "responses",
    label: "OpenAI Responses",
    desc: "Responses API，input/output 格式",
    endpoint: "/responses",
    enabled: false,
    icon: Sparkles,
  },
  {
    id: "anthropic",
    label: "Anthropic Messages",
    desc: "Claude Messages 协议，支持 Claude Code",
    endpoint: "/messages",
    enabled: false,
    icon: Zap,
  },
];

// ============================================================
// 顶部「客户端」pill tabs（仅 API 接口可用，其他后续支持）
// ============================================================
const CLIENT_TABS = [
  { id: "api", label: "API 接口", enabled: true },
  { id: "codex", label: "Codex", enabled: false },
  { id: "claude-code", label: "Claude Code", enabled: false },
  { id: "opencode", label: "OpenCode", enabled: false },
  { id: "openclaw", label: "OpenClaw", enabled: false },
  { id: "hermes", label: "Hermes", enabled: false },
] as const;

// ============================================================
// 代码示例（4 个平台 × 仅 OpenAI Chat 可用）
// ============================================================
type CodeLang = "curl" | "javascript" | "typescript" | "python";

const CODE_LANGS: { id: CodeLang; label: string }[] = [
  { id: "curl", label: "cURL" },
  { id: "javascript", label: "JavaScript" },
  { id: "typescript", label: "TypeScript" },
  { id: "python", label: "Python" },
];

/** 下拉框里以「名称」为主、密钥掩码显示，避免长明文把名称挤没了 */
function maskKeyForDisplay(full: string): string {
  const m = full.match(/^(sk-dongapi-)(.{4}).*(.{4})$/);
  if (m) return `${m[1]}${m[2]}••••${m[3]}`;
  return full.length > 16 ? `${full.slice(0, 10)}••••${full.slice(-4)}` : full;
}

function buildCodeSamples(
  baseUrl: string,
  model: string,
  apiKey: string,
): Record<CodeLang, string> {
  const url = `${baseUrl}/chat/completions`;
  // 自动填入当前下拉选中的密钥；未选择时回退占位符（代码仍可直接复制，
  // 仅需在 API_KEY 处补上真实密钥即可运行）
  const keyLiteral = apiKey.trim() || "sk-dongapi-你的密钥";
  const sampleBody = {
    model: model || "MODEL_NAME",
    messages: [{ role: "user", content: "Say hello in one sentence" }],
    stream: false,
  };
  return {
    curl: `# 网关监听 127.0.0.1，仅本机可达
API_KEY="${keyLiteral}"
curl -X POST "${url}" \\
  -H "Authorization: Bearer \${API_KEY}" \\
  -H "Content-Type: application/json" \\
  -d '${JSON.stringify(sampleBody)}'`,
    javascript: `// 浏览器 fetch — 网关监听 127.0.0.1，仅本机可达
const API_KEY = "${keyLiteral}";

const res = await fetch("${url}", {
  method: "POST",
  headers: {
    "Authorization": \`Bearer \${API_KEY}\`,
    "Content-Type": "application/json",
  },
  body: JSON.stringify(${JSON.stringify(sampleBody, null, 2)}),
});
const data = await res.json();
console.log(data.choices[0].message.content);`,
    typescript: `import type { ChatCompletion } from "./types";

const API_KEY = "${keyLiteral}";

const res = await fetch<ChatCompletion>("${url}", {
  method: "POST",
  headers: {
    "Authorization": \`Bearer \${API_KEY}\`,
    "Content-Type": "application/json",
  },
  body: JSON.stringify(${JSON.stringify(sampleBody, null, 2)}),
});`,
    python: `import requests

API_KEY = "${keyLiteral}"

resp = requests.post(
    "${url}",
    headers={
        "Authorization": f"Bearer {API_KEY}",
        "Content-Type": "application/json",
    },
    json=${JSON.stringify(sampleBody, null, 4).replace(/\n/g, "\n    ")},
)
print(resp.json()["choices"][0]["message"]["content"])`,
  };
}

// ============================================================
// 连接测试状态机
// ============================================================
type TestState = "idle" | "running" | "success" | "error";

interface TestResult {
  state: Exclude<TestState, "idle" | "running">;
  status?: number;
  statusText?: string;
  latencyMs: number;
  body: string;
  content?: string; // 提取出的回复文本（成功时）
}

const TEST_PROMPT = "用一句话打个招呼";

function buildRequestBody(protocol: ProtocolDef["id"], model: string): unknown {
  if (protocol === "anthropic") {
    return {
      model,
      max_tokens: 256,
      messages: [{ role: "user", content: TEST_PROMPT }],
    };
  }
  // OpenAI Chat + OpenAI Responses 共用 messages 结构（MVP 简化）
  return {
    model,
    messages: [{ role: "user", content: TEST_PROMPT }],
  };
}

function extractContent(protocol: ProtocolDef["id"], data: unknown): string | undefined {
  if (!data || typeof data !== "object") return undefined;
  const obj = data as Record<string, any>;
  if (protocol === "anthropic") {
    return Array.isArray(obj["content"]) ? obj["content"]?.[0]?.text : undefined;
  }
  if (protocol === "responses") {
    return obj["output"]?.[0]?.content?.[0]?.text;
  }
  return obj["choices"]?.[0]?.message?.content;
}

// ============================================================
// 主页面
// ============================================================
export function UsagePage() {
  const toast = useToast();

  // 数据
  const [channels, setChannels] = useState<Channel[]>([]);
  const [keys, setKeys] = useState<ApiKey[]>([]);
  const [settings, setSettings] = useState<Settings | null>(null);
  const [dataLoading, setDataLoading] = useState(true);

  // 选择
  const [activeProtocol, setActiveProtocol] = useState<ProtocolDef["id"]>("chat");
  const [activeClient, setActiveClient] = useState<string>("api");
  const [selectedKeyId, setSelectedKeyId] = useState("");
  const [model, setModel] = useState("");

  // 测试结果
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  const [testState, setTestState] = useState<TestState>("idle");

  // 代码示例
  const [codeLang, setCodeLang] = useState<CodeLang>("curl");
  const [codeExpanded, setCodeExpanded] = useState(true);

  // 派生
  const protocol = PROTOCOLS.find((p) => p.id === activeProtocol)!;
  const baseUrl = settings
    ? `http://${settings.server_host}:${settings.server_port}/v1`
    : "http://127.0.0.1:9842/v1";
  const fullEndpoint = `${baseUrl}${protocol.endpoint}`;

  // 选中的密钥明文（由下拉框驱动；本地明文存储，直接可用于请求）
  const apiKey = useMemo(
    () => keys.find((k) => k.id === selectedKeyId)?.key ?? "",
    [keys, selectedKeyId],
  );

  // 模型列表（合并所有启用渠道的 models，按字母排序去重）
  const modelOptions = useMemo(() => {
    const set = new Set<string>();
    channels
      .filter((c) => c.status === 1)
      .forEach((c) => c.models.forEach((m) => set.add(m)));
    return Array.from(set).sort();
  }, [channels]);

  // 可用（启用）的网关密钥
  const availableKeys = useMemo(
    () => keys.filter((k) => k.status === 1),
    [keys],
  );

  // 初始加载
  useEffect(() => {
    let cancelled = false;
    (async () => {
      try {
        const [c, k, s] = await Promise.all([
          channelApi.list().catch(() => []),
          keyApi.list().catch(() => []),
          settingsApi.get().catch(() => null),
        ]);
        if (cancelled) return;
        setChannels(c);
        setKeys(k);
        setSettings(s);
      } finally {
        if (!cancelled) setDataLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // 默认选第一个 model
  useEffect(() => {
    if (!model && modelOptions.length > 0) setModel(modelOptions[0]);
  }, [modelOptions, model]);

  // 默认选中第一个可用的密钥
  useEffect(() => {
    if (!selectedKeyId && availableKeys.length > 0) {
      setSelectedKeyId(availableKeys[0].id);
    }
  }, [availableKeys, selectedKeyId]);

  const canTest =
    testState !== "running" &&
    protocol.enabled &&
    apiKey.trim().length > 0 &&
    model.trim().length > 0 &&
    settings != null;

  const copyText = async (text: string, what: string) => {
    try {
      await navigator.clipboard.writeText(text);
      toast.success(`已复制${what}`);
    } catch {
      toast.error("复制失败");
    }
  };

  const handleTest = async () => {
    if (!canTest) return;
    setTestState("running");
    setTestResult(null);
    const start = performance.now();
    try {
      const isAnthropic = protocol.id === "anthropic";
      const headers: Record<string, string> = {
        "Content-Type": "application/json",
      };
      if (isAnthropic) {
        headers["x-api-key"] = apiKey.trim();
        headers["anthropic-version"] = "2023-06-01";
      } else {
        headers["Authorization"] = `Bearer ${apiKey.trim()}`;
      }
      const resp = await fetch(fullEndpoint, {
        method: "POST",
        headers,
        body: JSON.stringify(buildRequestBody(protocol.id, model)),
      });
      const text = await resp.text();
      let data: unknown = null;
      try {
        data = JSON.parse(text);
      } catch {
        /* non-JSON */
      }
      const elapsed = Math.round(performance.now() - start);
      const ok = resp.ok;
      const content = ok ? extractContent(protocol.id, data) : undefined;
      setTestResult({
        state: ok ? "success" : "error",
        status: resp.status,
        statusText: resp.statusText,
        latencyMs: elapsed,
        body: data ? JSON.stringify(data, null, 2) : text,
        content,
      });
      setTestState(ok ? "success" : "error");
      if (ok) toast.success(`测试成功 · ${elapsed}ms`);
      else toast.error(`测试失败 · HTTP ${resp.status}`);
    } catch (e: any) {
      const elapsed = Math.round(performance.now() - start);
      const msg = e?.message || String(e);
      setTestResult({
        state: "error",
        latencyMs: elapsed,
        body: `Request failed: ${msg}\n\n可能原因：\n1. 网关未启动（检查 9842 端口）\n2. 上游渠道故障\n3. 防火墙/代理拦截`,
      });
      setTestState("error");
      toast.error("网络请求失败");
    }
  };

  // 代码示例派生（依赖 baseUrl + model + 当前选中的密钥）
  const codeSamples = useMemo(
    () => buildCodeSamples(baseUrl, model, apiKey),
    [baseUrl, model, apiKey],
  );

  return (
    <div className="space-y-6">
      {/* ── 顶部标题 ──────────────────────────────────────── */}
      <div>
        <div className="flex items-center gap-2.5">
          <div className="flex h-9 w-9 items-center justify-center rounded-xl bg-primary/10 text-primary ring-1 ring-inset ring-primary/15">
            <BookOpen className="h-5 w-5" />
          </div>
          <div>
            <h1 className="text-2xl font-semibold tracking-tight">LLM 使用</h1>
            <p className="mt-0.5 text-sm text-muted-foreground">
              本地网关直连测试 · OpenAI 兼容协议
            </p>
          </div>
        </div>
      </div>

      {/* ── 客户端 tabs（pill row） ────────────────────────── */}
      <div className="flex flex-wrap gap-2">
        {CLIENT_TABS.map((t) => {
          const active = t.id === activeClient;
          return (
            <button
              key={t.id}
              onClick={() => t.enabled && setActiveClient(t.id)}
              disabled={!t.enabled}
              className={cn(
                "rounded-full border px-4 py-1.5 text-sm font-medium transition-colors",
                active
                  ? "border-primary bg-primary text-primary-foreground shadow-sm"
                  : t.enabled
                    ? "border-border bg-background text-foreground hover:bg-accent"
                    : "border-border bg-muted/40 text-muted-foreground/60 cursor-not-allowed",
              )}
            >
              {t.label}
              {!t.enabled && (
                <span className="ml-1.5 text-[10px] opacity-70">soon</span>
              )}
            </button>
          );
        })}
      </div>

      {/* ── 协议卡片 ──────────────────────────────────────── */}
      <div className="grid gap-4 md:grid-cols-3">
        {PROTOCOLS.map((p) => {
          const Icon = p.icon;
          const selected = p.id === activeProtocol;
          return (
            <button
              key={p.id}
              onClick={() => p.enabled && setActiveProtocol(p.id)}
              disabled={!p.enabled}
              className={cn(
                "group relative flex flex-col items-start gap-2 rounded-xl border bg-card p-5 text-left shadow-sm transition-all",
                selected && p.enabled
                  ? "border-primary ring-2 ring-primary/30"
                  : "hover:border-primary/40 hover:shadow",
                !p.enabled && "cursor-not-allowed opacity-60",
              )}
            >
              {selected && p.enabled && (
                <CheckCircle2 className="absolute top-3 right-3 h-5 w-5 text-primary" />
              )}
              {!p.enabled && (
                <Badge
                  variant="secondary"
                  className="absolute top-3 right-3 text-[10px]"
                >
                  敬请期待
                </Badge>
              )}
              <div
                className={cn(
                  "flex h-10 w-10 items-center justify-center rounded-xl",
                  selected && p.enabled
                    ? "bg-primary text-primary-foreground"
                    : "bg-muted text-muted-foreground",
                )}
              >
                <Icon className="h-5 w-5" />
              </div>
              <div className="text-base font-semibold">{p.label}</div>
              <div className="text-xs text-muted-foreground">{p.desc}</div>
              <code className="mt-1 rounded-md bg-muted/60 px-2 py-1 font-mono text-[11px] text-muted-foreground">
                {p.endpoint}
              </code>
            </button>
          );
        })}
      </div>

      {/* ── 接入信息 + 连接测试（双栏） ───────────────────── */}
      <div className="grid gap-4 lg:grid-cols-[1.05fr_0.95fr]">
        {/* 左：接入信息 */}
        <Card>
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <CardTitle className="text-base">接入信息</CardTitle>
              <Badge variant="success" className="text-[11px]">
                {protocol.label}
              </Badge>
            </div>
          </CardHeader>
          <CardContent className="space-y-4">
            <FieldRow
              label="BASE URL"
              icon={<Plug className="h-3.5 w-3.5" />}
            >
              {dataLoading ? (
                <Skeleton className="h-9 w-full" />
              ) : (
                <div className="flex items-center gap-1">
                  <Input
                    readOnly
                    value={baseUrl}
                    className="flex-1 font-mono text-xs"
                  />
                  <CopyButton
                    copied={false}
                    onCopy={() => copyText(baseUrl, "BASE URL")}
                  />
                </div>
              )}
            </FieldRow>

            <FieldRow label="协议端点" icon={<Terminal className="h-3.5 w-3.5" />}>
              <div className="flex items-center gap-1">
                <Input
                  readOnly
                  value={fullEndpoint}
                  className="flex-1 font-mono text-xs"
                />
                <CopyButton
                  copied={false}
                  onCopy={() => copyText(fullEndpoint, "协议端点")}
                />
              </div>
            </FieldRow>

            <FieldRow label="API KEY" icon={<span className="font-mono text-[10px]">SK</span>}>
              <div className="space-y-1.5">
                <Select
                  value={selectedKeyId}
                  onChange={(e) => setSelectedKeyId(e.target.value)}
                  disabled={availableKeys.length === 0}
                  className="font-mono text-xs"
                >
                  {availableKeys.length === 0 ? (
                    <option value="">（暂无可用密钥，请先到「密钥管理」创建）</option>
                  ) : (
                    availableKeys.map((k) => (
                      <option key={k.id} value={k.id}>
                        {k.name} · {maskKeyForDisplay(k.key)}
                      </option>
                    ))
                  )}
                </Select>
                <p className="text-[11px] leading-relaxed text-muted-foreground">
                  选择已创建的网关密钥（本地明文存储，可直接选用）。若列表为空，请先到
                  「密钥管理」创建。
                </p>
              </div>
            </FieldRow>

            <FieldRow label="MODEL" icon={<span className="font-mono text-[10px]">M</span>}>
              {modelOptions.length === 0 ? (
                <div className="rounded-md border border-dashed px-3 py-2 text-xs text-muted-foreground">
                  暂无可用模型，请先在「渠道管理」启用至少一个渠道
                </div>
              ) : (
                <Select
                  value={model}
                  onChange={(e) => setModel(e.target.value)}
                  className="font-mono text-xs"
                >
                  {modelOptions.map((m) => (
                    <option key={m} value={m}>
                      {m}
                    </option>
                  ))}
                </Select>
              )}
            </FieldRow>
          </CardContent>
        </Card>

        {/* 右：连接测试 */}
        <Card>
          <CardHeader className="pb-3">
            <div className="flex items-center justify-between">
              <CardTitle className="text-base">连接测试</CardTitle>
              <Button
                onClick={handleTest}
                disabled={!canTest}
                size="sm"
                className="gap-1.5"
              >
                {testState === "running" ? (
                  <Loader2 className="h-3.5 w-3.5 animate-spin" />
                ) : (
                  <Send className="h-3.5 w-3.5" />
                )}
                发送测试请求
              </Button>
            </div>
          </CardHeader>
          <CardContent>
            {/* 预览行 */}
            <div className="mb-3 flex items-center gap-2 rounded-lg border bg-muted/30 px-3 py-2 text-xs">
              <span className="text-muted-foreground">将使用</span>
              <Badge variant="secondary" className="text-[11px]">
                {protocol.label}
              </Badge>
              <span className="text-muted-foreground">→</span>
              <code className="flex-1 truncate font-mono text-[11px] text-foreground/80">
                {protocol.endpoint}
              </code>
            </div>

            {/* 结果区 */}
            {testState === "idle" && !testResult && (
              <EmptyState
                icon={Zap}
                title="点击上方按钮发起测试请求"
                description={`会向网关发送一个最小请求（"${TEST_PROMPT}"），用于校验端到端链路：网关 → 鉴权 → 渠道路由 → 上游 → 回传。`}
              />
            )}

            {testState === "running" && (
              <div className="flex flex-col items-center justify-center gap-3 rounded-lg border border-primary/30 bg-primary/5 py-12 text-sm text-primary">
                <Loader2 className="h-6 w-6 animate-spin" />
                <span>正在发送测试请求...</span>
                <span className="text-[11px] text-muted-foreground">
                  预计 1-3 秒（取决于上游响应）
                </span>
              </div>
            )}

            {testResult && <ResultPanel result={testResult} />}
          </CardContent>
        </Card>
      </div>

      {/* ── 代码示例 ──────────────────────────────────────── */}
      <Card>
        <CardHeader className="pb-3">
          <button
            onClick={() => setCodeExpanded((v) => !v)}
            className="flex w-full items-center justify-between"
          >
            <CardTitle className="text-base">代码示例</CardTitle>
            {codeExpanded ? (
              <ChevronUp className="h-4 w-4 text-muted-foreground" />
            ) : (
              <ChevronDown className="h-4 w-4 text-muted-foreground" />
            )}
          </button>
        </CardHeader>
        {codeExpanded && (
          <CardContent className="space-y-3">
            <div className="flex flex-wrap gap-1.5">
              {CODE_LANGS.map((l) => (
                <button
                  key={l.id}
                  onClick={() => setCodeLang(l.id)}
                  className={cn(
                    "rounded-md border px-3 py-1 text-xs font-medium transition-colors",
                    codeLang === l.id
                      ? "border-primary bg-primary text-primary-foreground"
                      : "border-border bg-background text-foreground hover:bg-accent",
                  )}
                >
                  {l.label}
                </button>
              ))}
            </div>
            <div className="relative">
              <pre className="max-h-96 overflow-auto rounded-xl border bg-zinc-950 p-4 font-mono text-[12px] leading-relaxed text-zinc-100">
                {codeSamples[codeLang]}
              </pre>
              <Button
                variant="outline"
                size="sm"
                onClick={() => copyText(codeSamples[codeLang], "代码")}
                className="absolute top-3 right-3 h-7 gap-1.5 border-zinc-700 bg-zinc-900/80 text-xs text-zinc-200 hover:bg-zinc-800 hover:text-zinc-100"
              >
                <Copy className="h-3 w-3" />
                复制
              </Button>
            </div>
          </CardContent>
        )}
      </Card>
    </div>
  );
}

// ============================================================
// 子组件
// ============================================================
function FieldRow({
  label,
  icon,
  children,
}: {
  label: string;
  icon: React.ReactNode;
  children: React.ReactNode;
}) {
  return (
    <div className="space-y-1.5">
      <div className="flex items-center gap-1.5 text-xs font-medium text-muted-foreground">
        <span className="flex h-4 w-4 items-center justify-center text-muted-foreground/70">
          {icon}
        </span>
        {label}
      </div>
      {children}
    </div>
  );
}

function CopyButton({
  copied,
  onCopy,
}: {
  copied: boolean;
  onCopy: () => void;
}) {
  return (
    <Button
      variant="outline"
      size="icon"
      onClick={onCopy}
      className="h-9 w-9 shrink-0"
      title="复制"
    >
      {copied ? (
        <CheckCircle2 className="h-3.5 w-3.5 text-success" />
      ) : (
        <Copy className="h-3.5 w-3.5" />
      )}
    </Button>
  );
}

function ResultPanel({ result }: { result: TestResult }) {
  const isSuccess = result.state === "success";
  return (
    <div
      className={cn(
        "space-y-3 rounded-lg border p-4",
        isSuccess
          ? "border-success/30 bg-success/5"
          : "border-destructive/30 bg-destructive/5",
      )}
    >
      <div className="flex items-center justify-between">
        <Badge variant={isSuccess ? "success" : "destructive"}>
          {isSuccess
            ? `成功 ${result.status ?? 200}`
            : `失败${result.status ? ` HTTP ${result.status}` : ""}`}
        </Badge>
        <span className="font-mono text-xs text-muted-foreground tabular-nums">
          {result.latencyMs}ms
        </span>
      </div>

      {isSuccess && result.content && (
        <div className="rounded-md border border-success/30 bg-background/60 p-3 text-sm">
          <div className="mb-1 text-[11px] font-medium text-muted-foreground">
            回复内容
          </div>
          <div className="text-foreground">{result.content}</div>
        </div>
      )}

      <details className="text-xs">
        <summary className="cursor-pointer text-muted-foreground hover:text-foreground">
          {isSuccess ? "查看原始响应" : "查看错误详情"}
        </summary>
        <pre className="mt-2 max-h-72 overflow-auto rounded-md bg-muted/50 p-3 font-mono text-[11px] leading-relaxed">
          {result.body}
        </pre>
      </details>
    </div>
  );
}