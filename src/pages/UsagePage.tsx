import { useEffect, useMemo, useState } from "react";
import {
  BookOpen,
  CheckCircle2,
  ChevronDown,
  ChevronUp,
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
import { Select, SelectContent, SelectItem, SelectTrigger, SelectValue } from "@/components/ui/select";
import { EmptyState } from "@/components/ui/empty-state";
import { useToast } from "@/components/ui/toast";
import { CopyButton } from "@/components/ui/copy-button";
import { Skeleton } from "@/components/ui/skeleton";
import { channelApi, keyApi, settingsApi, clientConfigApi } from "@/lib/api";
import { cn } from "@/lib/utils";
import { ClientConfigView } from "@/components/ClientConfigView";
import { CodeBlock } from "@/components/CodeBlock";
import type { ApiKey, Channel, ClientInfo, Settings } from "@/types";

// ============================================================
// 协议卡片数据（OpenAI Chat / OpenAI Responses / Anthropic Messages）
// 三个协议均已开放：后端数据面分别注册 /v1/chat/completions、
// /v1/responses、/v1/messages，且 Responses 与 Messages 都通过「请求转
// Chat → 共享管道 → 响应转回」的桥接实现，与 Chat 共用同一套鉴权/
// 分发/熔断/日志逻辑。下游可按自身 SDK 选择任一协议接入。
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
    enabled: true,
    icon: Sparkles,
  },
  {
    id: "anthropic",
    label: "Anthropic Messages",
    desc: "Claude Messages 协议，支持 Claude Code",
    endpoint: "/messages",
    enabled: true,
    icon: Zap,
  },
];

// ============================================================
// 代码示例（4 个平台 × 按所选协议生成）
// ============================================================
type CodeLang = "curl" | "javascript" | "typescript" | "python";

const CODE_LANGS: { id: CodeLang; label: string }[] = [
  { id: "curl", label: "cURL" },
  { id: "javascript", label: "JavaScript" },
  { id: "typescript", label: "TypeScript" },
  { id: "python", label: "Python" },
];

/** 把代码示例的下拉语言映射到 prism 语法标识 */
function codeLangToPrism(lang: CodeLang): string {
  switch (lang) {
    case "curl":
      return "bash";
    case "javascript":
      return "javascript";
    case "typescript":
      return "typescript";
    case "python":
      return "python";
    default:
      return "clike";
  }
}

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
  stream: boolean,
  protocolId: ProtocolDef["id"],
): Record<CodeLang, string> {
  const isResponses = protocolId === "responses";
  const isAnthropic = protocolId === "anthropic";
  const path = isAnthropic
    ? "/messages"
    : isResponses
      ? "/responses"
      : "/chat/completions";
  const url = `${baseUrl}${path}`;
  // 直接把当前下拉选中的密钥内联进示例代码（本地网关，密钥即明文），
  // 这样复制后即可直接运行，无需再手动替换占位符。
  const keyLiteral = apiKey.trim() || "sk-dongapi-你的密钥";
  const modelLiteral = model || "MODEL_NAME";

  // 请求体：三种协议字段不同（anthropic 还需 max_tokens）。
  const sampleBody: Record<string, unknown> = isAnthropic
    ? {
        model: modelLiteral,
        max_tokens: 1024,
        messages: [{ role: "user", content: "Say hello in one sentence" }],
        stream: !!stream,
      }
    : isResponses
      ? {
          model: modelLiteral,
          input: [{ role: "user", content: "Say hello in one sentence" }],
          stream: !!stream,
        }
      : {
          model: modelLiteral,
          messages: [{ role: "user", content: "Say hello in one sentence" }],
          stream: !!stream,
        };
  const bodyLiteral = JSON.stringify(sampleBody, null, 2);
  const bodyLiteralPy = JSON.stringify(sampleBody, null, 4).replace(/\n/g, "\n    ");
  const streamFlag = stream ? "True" : "False";

  // 鉴权头：anthropic 用 x-api-key + anthropic-version；其余用 Bearer。
  const curlAuth = isAnthropic
    ? `  -H "x-api-key: ${keyLiteral}" \\\n  -H "anthropic-version: 2023-06-01" \\`
    : `  -H "Authorization: Bearer ${keyLiteral}" \\`;
  const jsAuth = isAnthropic
    ? `    "x-api-key": "${keyLiteral}",\n    "anthropic-version": "2023-06-01",`
    : `    "Authorization": "Bearer ${keyLiteral}",`;
  const pyAuth = isAnthropic
    ? `        "x-api-key": "${keyLiteral}",\n        "anthropic-version": "2023-06-01",`
    : `        "Authorization": "Bearer ${keyLiteral}",`;

  // 流式 SSE 解析：统一兼容 Chat / Responses / Anthropic 三种事件形状。
  const jsStreamParse = `const reader = res.body.getReader();
const decoder = new TextDecoder();
let text = "";
while (true) {
  const { done, value } = await reader.read();
  if (done) break;
  const chunk = decoder.decode(value, { stream: true });
  for (const frame of chunk.split("\\n\\n")) {
    const dataLine = frame.split("\\n").find((l) => l.startsWith("data:"));
    if (!dataLine) continue;
    const payload = dataLine.slice(5).trim();
    if (!payload || payload === "[DONE]") continue;
    try {
      const j = JSON.parse(payload);
      const d =
        j?.choices?.[0]?.delta?.content ??
        (j?.type === "response.output_text.delta" ? j.delta : "") ??
        (j?.type === "content_block_delta" ? (j.delta?.text || j.delta?.thinking || "") : "");
      text += d || "";
    } catch {}
  }
}
console.log(text);`;

  const pyStreamParse = `text = ""
for line in resp.iter_lines():
    if not line or not line.startswith(b"data:"):
        continue
    payload = line[5:].strip()
    if payload == b"[DONE]":
        continue
    try:
        j = json.loads(payload)
        if "choices" in j:
            d = j["choices"][0]["delta"].get("content") or ""
        elif j.get("type") == "response.output_text.delta":
            d = j.get("delta") or ""
        elif j.get("type") == "content_block_delta":
            d = (j.get("delta") or {}).get("text") or (j.get("delta") or {}).get("thinking") or ""
        else:
            d = ""
        text += d
    except Exception:
        pass
print(text)`;

  // 非流式：anthropic 回复体是 { content:[{type:"text",text}] }，直接打印整体。
  const jsNonStream = isAnthropic
    ? `const data = await res.json();
console.log(data?.content?.[0]?.text ?? data);`
    : isResponses
      ? `const data = await res.json();
console.log(data.output[0].content[0].text);`
      : `const data = await res.json();
console.log(data.choices[0].message.content);`;

  const pyNonStream = isAnthropic
    ? `data = resp.json()
print(data.get("content", [{}])[0].get("text", data))`
    : isResponses
      ? `data = resp.json()
print(data["output"][0]["content"][0]["text"])`
      : `data = resp.json()
print(data["choices"][0]["message"]["content"])`;

  const jsBody = stream
    ? `const res = await fetch("${url}", {
  method: "POST",
  headers: {
    ${jsAuth}
    "Content-Type": "application/json",
  },
  body: JSON.stringify(${bodyLiteral}),
});
// 流式：逐帧读取 SSE（网关按所选协议输出 Chat / Responses / Anthropic 事件）
${jsStreamParse}`
    : `const res = await fetch("${url}", {
  method: "POST",
  headers: {
    ${jsAuth}
    "Content-Type": "application/json",
  },
  body: JSON.stringify(${bodyLiteral}),
});
${jsNonStream}`;

  const pythonBody = stream ? pyStreamParse : pyNonStream;

  return {
    curl: `# 网关监听 127.0.0.1，仅本机可达${stream ? "（stream 模式下 curl 会逐帧打印 SSE）" : ""}
curl -X POST "${url}" \\
${curlAuth}
  -H "Content-Type: application/json" \\
  -d '${JSON.stringify(sampleBody)}'`,
    javascript: `// 浏览器 fetch — 网关监听 127.0.0.1，仅本机可达
${jsBody}`,
    typescript: `// 网关监听 127.0.0.1，仅本机可达
${jsBody}`,
    python: `import json
import requests

resp = requests.post(
    "${url}",
    headers={
${pyAuth}
        "Content-Type": "application/json",
    },
    json=${bodyLiteralPy},
    stream=${streamFlag},
)
${pythonBody}`,
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

function buildRequestBody(
  protocol: ProtocolDef["id"],
  model: string,
  stream: boolean,
): unknown {
  const msg = { role: "user", content: TEST_PROMPT };
  let base: Record<string, unknown>;
  if (protocol === "anthropic") {
    base = { model, max_tokens: 256, messages: [msg] };
  } else if (protocol === "responses") {
    base = { model, input: [msg] };
  } else {
    base = { model, messages: [msg] };
  }
  return { ...base, stream: !!stream };
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
  const [clientConfigs, setClientConfigs] = useState<ClientInfo[]>([]);
  const [dataLoading, setDataLoading] = useState(true);

  // 选择
  const [activeProtocol, setActiveProtocol] = useState<ProtocolDef["id"]>("chat");
  const [activeClient, setActiveClient] = useState<string>("api");
  const [selectedKeyId, setSelectedKeyId] = useState("");
  const [model, setModel] = useState("");

  // 测试结果
  const [testResult, setTestResult] = useState<TestResult | null>(null);
  const [testState, setTestState] = useState<TestState>("idle");

  // 流式模式开关（后端数据面已支持 OpenAI 兼容 SSE 转换）
  const [streamMode, setStreamMode] = useState(false);

  // 代码示例
  const [codeLang, setCodeLang] = useState<CodeLang>("curl");
  const [codeExpanded, setCodeExpanded] = useState(true);

  // 派生
  const protocol = PROTOCOLS.find((p) => p.id === activeProtocol)!;
  // Responses 模式后端尚不支持流式：强制关闭实际流式，避免发 stream 到 /v1/responses 触发 400
  const effectiveStream = streamMode;
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
        const [c, k, s, clients] = await Promise.all([
          channelApi.list().catch(() => []),
          keyApi.list().catch(() => []),
          settingsApi.get().catch(() => null),
          clientConfigApi.list().catch(() => []),
        ]);
        if (cancelled) return;
        setChannels(c);
        setKeys(k);
        setSettings(s);
        setClientConfigs(clients);
      } finally {
        if (!cancelled) setDataLoading(false);
      }
    })();
    return () => {
      cancelled = true;
    };
  }, []);

  // 重新拉取客户端安装/接入状态（供子页「刷新」后回写）
  const refreshClients = async () => {
    try {
      const clients = await clientConfigApi.list().catch(() => []);
      setClientConfigs(clients);
    } catch {
      /* 忽略刷新失败 */
    }
  };

  // 顶部 tabs：API 接口固定首位 + 后端下发的各客户端
  const allTabs = useMemo(
    () => [
      { id: "api", label: "API 接口" },
      ...clientConfigs.map((c) => ({ id: c.name, label: c.label })),
    ],
    [clientConfigs],
  );

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
        body: JSON.stringify(buildRequestBody(protocol.id, model, effectiveStream)),
      });

      if (!resp.ok) {
        const text = await resp.text();
        const elapsed = Math.round(performance.now() - start);
        setTestResult({
          state: "error",
          status: resp.status,
          statusText: resp.statusText,
          latencyMs: elapsed,
          body: text,
        });
        setTestState("error");
        toast.error(`测试失败 · HTTP ${resp.status}`);
        return;
      }

      // 流式模式：逐帧读取 SSE（网关按所选协议输出 Chat / Responses /
      // Anthropic 三种事件形状，此处统一解析）。
      if (effectiveStream && resp.body) {
        const reader = resp.body.getReader();
        const decoder = new TextDecoder();
        let buffer = "";
        let content = "";
        let raw = "";
        while (true) {
          const { done, value } = await reader.read();
          if (done) break;
          const chunk = decoder.decode(value, { stream: true });
          raw += chunk;
          buffer += chunk;
          let idx: number;
          while ((idx = buffer.indexOf("\n\n")) !== -1) {
            const frame = buffer.slice(0, idx);
            buffer = buffer.slice(idx + 2);
            const dataLine = frame.split("\n").find((l) => l.startsWith("data:"));
            if (!dataLine) continue;
            const payload = dataLine.slice(5).trim();
            if (payload === "[DONE]") continue;
            try {
              const json = JSON.parse(payload);
              // 兼容三种 SSE 形状：
              //  - Chat: choices[0].delta.content
              //  - Responses: event=response.output_text.delta 的 delta 字段
              //  - Anthropic: event=content_block_delta 的 delta.text / thinking
              const d =
                json?.choices?.[0]?.delta?.content ??
                (json?.type === "response.output_text.delta" ? json.delta : "") ??
                (json?.type === "content_block_delta"
                  ? json.delta?.text || json.delta?.thinking || ""
                  : "");
              if (typeof d === "string") content += d;
            } catch {
              /* 跳过非 JSON 帧 */
            }
          }
        }
        const elapsed = Math.round(performance.now() - start);
        setTestResult({
          state: "success",
          status: resp.status,
          statusText: resp.statusText,
          latencyMs: elapsed,
          body: raw,
          content,
        });
        setTestState("success");
        toast.success(`流式测试成功 · ${elapsed}ms`);
        return;
      }

      // 非流式模式
      const text = await resp.text();
      let data: unknown = null;
      try {
        data = JSON.parse(text);
      } catch {
        /* non-JSON */
      }
      const elapsed = Math.round(performance.now() - start);
      const content = extractContent(protocol.id, data);
      setTestResult({
        state: "success",
        status: resp.status,
        statusText: resp.statusText,
        latencyMs: elapsed,
        body: data ? JSON.stringify(data, null, 2) : text,
        content,
      });
      setTestState("success");
      toast.success(`测试成功 · ${elapsed}ms`);
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

  // 代码示例派生（依赖 baseUrl + model + 当前选中的密钥 + 流式开关 + 协议）
  const codeSamples = useMemo(
    () => buildCodeSamples(baseUrl, model, apiKey, streamMode, activeProtocol),
    [baseUrl, model, apiKey, streamMode, activeProtocol],
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
        {allTabs.map((t) => {
          const active = t.id === activeClient;
          const installed =
            t.id !== "api" &&
            clientConfigs.find((c) => c.name === t.id)?.available;
          return (
            <button
              key={t.id}
              onClick={() => setActiveClient(t.id)}
              className={cn(
                "flex items-center gap-1.5 rounded-full border px-4 py-1.5 text-sm font-medium transition-colors",
                active
                  ? "border-primary bg-primary text-primary-foreground shadow-sm"
                  : "border-border bg-background text-foreground hover:bg-accent",
              )}
            >
              {t.label}
              {installed && (
                <span className="h-1.5 w-1.5 rounded-full bg-emerald-500" />
              )}
            </button>
          );
        })}
      </div>

      {/* ── API 接口页（仅 api tab 显示） ─────────────────── */}
      {activeClient === 'api' && (
        <>
          {/* ── 协议卡片 ──────────────────────────────────────── */}
          <div className="grid gap-4 md:grid-cols-3">
        {PROTOCOLS.map((p) => {
          const Icon = p.icon;
          const selected = p.id === activeProtocol;
          return (
            <button
              key={p.id}
              onClick={() => {
                if (!p.enabled) return;
                setActiveProtocol(p.id);
                if (p.id === "responses") setStreamMode(false);
              }}
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
      <div className="grid items-start gap-4 lg:grid-cols-[1.05fr_0.95fr]">
        {/* 左：接入信息 */}
        <Card className="min-w-0">
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
                  <CopyButton value={baseUrl} label="BASE URL" />
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
                <CopyButton value={fullEndpoint} label="协议端点" />
              </div>
            </FieldRow>

            <FieldRow label="API KEY" icon={<span className="font-mono text-[10px]">SK</span>}>
              <div className="space-y-1.5">
                <Select
                  value={selectedKeyId || undefined}
                  onValueChange={setSelectedKeyId}
                  disabled={availableKeys.length === 0}
                >
                  <SelectTrigger className="font-mono text-xs w-full">
                    <SelectValue
                      placeholder={
                        availableKeys.length === 0
                          ? "（暂无可用密钥，请先到「密钥管理」创建）"
                          : "选择密钥"
                      }
                    />
                  </SelectTrigger>
                  <SelectContent>
                    {availableKeys.map((k) => (
                      <SelectItem key={k.id} value={k.id}>
                        {k.name} · {maskKeyForDisplay(k.key)}
                      </SelectItem>
                    ))}
                  </SelectContent>
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
                  value={model || undefined}
                  onValueChange={setModel}
                >
                  <SelectTrigger className="font-mono text-xs w-full">
                    <SelectValue placeholder="选择模型" />
                  </SelectTrigger>
                  <SelectContent className="font-mono text-xs w-full">
                    {modelOptions.map((m) => (
                      <SelectItem key={m} value={m}>
                        {m}
                      </SelectItem>
                    ))}
                  </SelectContent>
                </Select>
              )}
            </FieldRow>
          </CardContent>
        </Card>

        {/* 右：连接测试 */}
        <Card className="min-w-0">
          <CardHeader className="pb-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <CardTitle className="text-base">连接测试</CardTitle>
              <div className="flex items-center gap-2">
                <button
                  type="button"
                  onClick={() => setStreamMode((v) => !v)}
                  className={cn(
                    "flex items-center gap-2 rounded-full border px-3 py-1 text-xs font-medium transition-colors",
                    streamMode
                      ? "border-primary bg-primary/10 text-primary"
                      : "border-border bg-background text-muted-foreground hover:bg-accent",
                  )}
                  title="开启后按 SSE 流式逐帧接收，验证网关流式转换"
                >
                  <span
                    className={cn(
                      "h-1.5 w-1.5 rounded-full",
                      streamMode ? "bg-primary" : "bg-muted-foreground/40",
                    )}
                  />
                  流式 Stream
                </button>
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
              {effectiveStream && (
                <Badge
                  variant="secondary"
                  className="shrink-0 bg-primary/10 text-primary"
                >
                  Stream
                </Badge>
              )}
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
              <CodeBlock code={codeSamples[codeLang]} lang={codeLangToPrism(codeLang)} />
              <CopyButton
                value={codeSamples[codeLang]}
                label="代码"
                variant="button"
                className="absolute top-3 right-3 h-7 gap-1.5 border-zinc-700 bg-zinc-900/80 text-xs text-zinc-200 hover:bg-zinc-800 hover:text-zinc-100"
              />
            </div>
          </CardContent>
        )}
      </Card>
        </>
      )}

      {/* ── 客户端接入配置页（非 api tab） ─────────────────── */}
      {activeClient !== 'api' &&
        (() => {
          const c = clientConfigs.find((x) => x.name === activeClient);
          return c ? (
            <ClientConfigView
              key={c.name}
              client={c}
              gatewayUrl={baseUrl}
              keys={keys}
              modelOptions={modelOptions}
              onRefresh={refreshClients}
            />
          ) : null;
        })()}
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