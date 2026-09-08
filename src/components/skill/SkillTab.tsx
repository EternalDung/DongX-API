import { useCallback, useEffect, useState } from "react";
import {
  Server,
  Zap,
  Copy,
  Check,
  Download,
  ArrowRight,
  FileText,
} from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
import { CodeBlock } from "@/components/CodeBlock";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogFooter,
  DialogHeader,
  DialogTitle,
} from "@/components/ui/dialog";
import { Skeleton } from "@/components/ui/skeleton";
import { useToast } from "@/components/ui/toast";
import { mcpApi } from "@/lib/api";
import type { McpStatus } from "@/types";
import { createZip, downloadBlob } from "@/lib/zip";
import { ragSkills, type RagToolSpec, type JsonProp } from "./ragSkillSpec";

const DEFAULT_ENDPOINT = "http://127.0.0.1:9842/mcp";

/** 为工具生成一个可运行的示例入参（必填项填占位值，带默认值的可选项一并带上）。 */
function sampleValue(key: string, prop: JsonProp): unknown {
  if (key === "kb_id") return "kb_xxx";
  switch (prop.type) {
    case "integer":
    case "number":
      return typeof prop.default === "number" ? prop.default : 1;
    case "boolean":
      return false;
    case "array":
      return [];
    default:
      return "示例";
  }
}

function exampleArgs(spec: RagToolSpec): string {
  const props = spec.inputSchema.properties ?? {};
  const required = spec.inputSchema.required ?? [];
  const args: Record<string, unknown> = {};
  for (const [k, v] of Object.entries(props)) {
    if (required.includes(k)) {
      args[k] = sampleValue(k, v);
    } else if (v.default !== undefined) {
      args[k] = v.default;
    }
  }
  return JSON.stringify(args);
}

function demoCommand(spec: RagToolSpec): string {
  return `python3 scripts/mcp_call.py ${spec.name} '${exampleArgs(spec)}'`;
}

/** 生成导出用的 SKILL.md（frontmatter + 工具契约 + demo），与页面展示同源。 */
function buildSkillMd(tools: RagToolSpec[], endpoint: string): string {
  const ep = endpoint || DEFAULT_ENDPOINT;
  const lines: string[] = [];
  lines.push("---");
  lines.push("name: dongx-rag-skill");
  lines.push(
    "description: DongX 本地网关的 RAG 知识库技能。通过 MCP (Streamable HTTP) 暴露语义检索、RAG 问答、文档读取与统计，供客户端 Agent 直接调用。触发词：知识库检索、RAG 问答、搜文档、读文档、知识库统计。",
  );
  lines.push("license: MIT");
  lines.push("metadata:");
  lines.push("  author: dongx");
  lines.push('  version: "1.0.0"');
  lines.push("  category: knowledge-base");
  lines.push("  homepage: https://github.com/wei/dongx");
  lines.push("---");
  lines.push("");
  lines.push("# DongX RAG 技能包");
  lines.push("");
  lines.push(
    "通过 MCP (Streamable HTTP) 连接 DongX 本地网关，提供 RAG 语义检索、RAG 问答、文档读取与统计。所有调用经 `scripts/mcp_call.py`。",
  );
  lines.push("");
  lines.push("## 前置条件");
  lines.push("");
  lines.push(`- DongX 网关运行中，MCP 端点：${ep}`);
  lines.push("- 至少一个知识库已开启「MCP 暴露」（KB 开关处开启）");
  lines.push("");
  lines.push("## 使用方式");
  lines.push("");
  lines.push("```bash");
  lines.push("python3 scripts/mcp_call.py <tool_name> '<json_arguments>'");
  lines.push("```");
  lines.push("");
  lines.push("## 工具清单");
  lines.push("");

  for (const spec of tools) {
    const props = spec.inputSchema.properties ?? {};
    const required = spec.inputSchema.required ?? [];
    lines.push(`### ${spec.name}`);
    lines.push("");
    lines.push(spec.description);
    lines.push("");
    lines.push("**入参**");
    lines.push("");
    lines.push("| 参数 | 类型 | 必填 | 默认 | 说明 |");
    lines.push("|---|---|---|---|---|");
    for (const [k, v] of Object.entries(props)) {
      const def = v.default !== undefined ? String(v.default) : "—";
      lines.push(
        `| ${k} | ${v.type ?? ""} | ${required.includes(k) ? "是" : "否"} | ${def} | ${v.description ?? ""} |`,
      );
    }
    lines.push("");
    lines.push("**出参**");
    lines.push("");
    lines.push(spec.returns);
    lines.push("");
    lines.push("**示例**");
    lines.push("");
    lines.push("```bash");
    lines.push(demoCommand(spec));
    lines.push("```");
    lines.push("");
  }
  return lines.join("\n");
}

/** DongX Streamable HTTP 版 MCP 客户端（直连 POST JSON-RPC，区别于 waliapi 的 SSE 版）。 */
const MCP_CALL_PY = `#!/usr/bin/env python3
"""DongX RAG MCP client - Streamable HTTP (JSON-RPC 2.0 over POST).

通过 DongX 网关暴露的 MCP 端点调用 RAG 工具。

用法:
    python3 mcp_call.py <tool_name> '<json_arguments>'
    python3 mcp_call.py list_knowledge_bases '{}'
    python3 mcp_call.py search_knowledge_base '{"kb_id":"kb_xxx","query":"如何配置渠道","top_k":5}'
    python3 mcp_call.py ask_knowledge_base '{"kb_id":"kb_xxx","question":"支持哪些模型？"}'

环境变量:
    DONGX_MCP_URL - 覆盖 MCP 端点（默认 http://127.0.0.1:9842/mcp）
"""

import json
import os
import sys
import urllib.request
import urllib.error

CONFIG_PATH = os.path.expanduser("~/.dongx/skills/rag/config.json")
DEFAULT_MCP_URL = "http://127.0.0.1:9842/mcp"
TIMEOUT = 30


def get_mcp_url():
    url = os.environ.get("DONGX_MCP_URL")
    if url:
        return url
    if os.path.exists(CONFIG_PATH):
        try:
            with open(CONFIG_PATH, "r", encoding="utf-8") as f:
                cfg = json.load(f)
                if cfg.get("mcp_url"):
                    return cfg["mcp_url"]
        except (json.JSONDecodeError, OSError):
            pass
    return DEFAULT_MCP_URL


def call_tool(tool_name, arguments):
    url = get_mcp_url()
    payload = {
        "jsonrpc": "2.0",
        "id": 1,
        "method": "tools/call",
        "params": {"name": tool_name, "arguments": arguments},
    }
    req = urllib.request.Request(url, data=json.dumps(payload).encode("utf-8"), method="POST")
    req.add_header("Content-Type", "application/json")
    req.add_header("Accept", "application/json")
    try:
        with urllib.request.urlopen(req, timeout=TIMEOUT) as resp:
            body = json.loads(resp.read().decode("utf-8"))
    except urllib.error.HTTPError as e:
        body = json.loads(e.read().decode("utf-8", errors="replace"))

    if "error" in body:
        err = body["error"]
        print("Error {0}: {1}".format(err.get("code"), err.get("message")), file=sys.stderr)
        sys.exit(1)

    result = body.get("result", {})
    content = result.get("content", [])
    if result.get("isError"):
        print("Warning: ", end="")
    if isinstance(content, list):
        for item in content:
            if isinstance(item, dict) and item.get("type") == "text":
                print(item["text"])
            else:
                print(json.dumps(item, ensure_ascii=False))
    elif isinstance(content, str):
        print(content)
    else:
        print(json.dumps(result, ensure_ascii=False, indent=2))


def main():
    if len(sys.argv) < 2:
        print(__doc__)
        sys.exit(1)
    tool_name = sys.argv[1]
    args_str = sys.argv[2] if len(sys.argv) > 2 else "{}"
    try:
        arguments = json.loads(args_str)
    except json.JSONDecodeError as e:
        print("Invalid JSON arguments: {0}".format(e), file=sys.stderr)
        sys.exit(1)
    try:
        call_tool(tool_name, arguments)
    except Exception as e:
        print("MCP call failed: {0}".format(e), file=sys.stderr)
        sys.exit(1)


if __name__ == "__main__":
    main()
`;

export function SkillTab() {
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [detailTool, setDetailTool] = useState<RagToolSpec | null>(null);
  const [previewMd, setPreviewMd] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const [exporting, setExporting] = useState(false);
  const toast = useToast();

  useEffect(() => {
    let alive = true;
    mcpApi
      .status()
      .then((s) => {
        if (alive) setStatus(s);
      })
      .catch(() => {
        if (alive) setStatus(null);
      })
      .finally(() => {
        if (alive) setLoading(false);
      });
    return () => {
      alive = false;
    };
  }, []);

  // 工具契约来自静态技能包（ragSkillSpec），仅端点随运行态变化。
  const ragTools = ragSkills;
  const endpoint = status?.endpoint ?? DEFAULT_ENDPOINT;

  const handleCopyEndpoint = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(endpoint);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  }, [endpoint]);

  const handleExport = useCallback(() => {
    if (!ragTools.length) return;
    setExporting(true);
    try {
      const md = buildSkillMd(ragTools, endpoint);
      const config = JSON.stringify({ mcp_url: endpoint }, null, 2);
      const blob = createZip([
        { name: "SKILL.md", data: md },
        { name: "scripts/mcp_call.py", data: MCP_CALL_PY },
        { name: "config.json", data: config },
      ]);
      downloadBlob(blob, "dongx-rag-skill.zip");
      toast.success("技能包已导出：dongx-rag-skill.zip 已保存到下载目录（Downloads）");
    } finally {
      setExporting(false);
    }
  }, [ragTools, endpoint, toast]);

  const handlePreview = useCallback(() => {
    if (!ragTools.length) return;
    setPreviewMd(buildSkillMd(ragTools, endpoint));
  }, [ragTools, endpoint]);

  if (loading) {
    return (
      <div className="space-y-4">
        <Skeleton className="h-16 w-full rounded-lg" />
        <Skeleton className="h-40 w-full rounded-lg" />
      </div>
    );
  }

  return (
    <div className="space-y-6">
      {/* 端点信息条 */}
      <div className="flex items-center gap-3 rounded-lg border border-border-tertiary bg-background-secondary px-4 py-3">
        <span
          className={`rounded-full px-2 py-0.5 text-xs ${
            status?.running
              ? "bg-success/15 text-success"
              : "bg-muted text-muted-foreground"
          }`}
        >
          {status?.running ? "运行中" : "未运行"}
        </span>
        <code className="break-all font-mono text-xs text-foreground-secondary">{endpoint}</code>
        <button
          onClick={handleCopyEndpoint}
          className="ml-1 shrink-0 rounded p-1 text-muted-foreground hover:text-foreground"
          title="复制端点"
        >
          {copied ? <Check className="h-4 w-4" /> : <Copy className="h-4 w-4" />}
        </button>
        <span className="ml-auto shrink-0 text-xs text-muted-foreground">
          {ragTools.length} 个工具
        </span>
      </div>

      {!status?.running && (
        <div className="rounded-lg border border-border-tertiary bg-background-secondary px-4 py-3 text-xs text-muted-foreground">
          网关未运行，MCP 端点不可达。启动网关后再导出技能包（SKILL.md 会使用默认端点 {DEFAULT_ENDPOINT}）。
        </div>
      )}

      <p className="text-sm text-foreground-secondary">
        技能包 (Skill Package) — 把 MCP 工具连同契约（入参 / 出参 / demo）打包，供客户端 Agent 直接加载调用。
      </p>

      {/* 技能包卡片 */}
      <div className="grid grid-cols-1 gap-4 md:grid-cols-2">
        <Card>
          <CardContent className="space-y-3 p-5">
            <div className="flex items-center gap-3">
              <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-info/15 text-info">
                <Server className="h-4 w-4" />
              </div>
              <div>
                <p className="font-medium">RAG 知识库技能包</p>
                <p className="text-xs text-muted-foreground">检索增强 · {ragTools.length} 工具</p>
              </div>
            </div>
            <p className="text-xs leading-relaxed text-foreground-secondary">
              语义检索、RAG 问答、文档读取与统计。每个工具都带完整入参 / 出参契约。
            </p>
            <div className="flex flex-wrap gap-2">
              <Button
                variant="outline"
                size="sm"
                onClick={() => handlePreview()}
                disabled={!ragTools.length}
              >
                <FileText className="h-3.5 w-3.5" />
                预览 SKILL.md
              </Button>
              <Button
                size="sm"
                onClick={() => handleExport()}
                disabled={!ragTools.length || exporting}
              >
                <Download className="h-3.5 w-3.5" />
                {exporting ? "导出中…" : "导出技能包"}
              </Button>
            </div>
          </CardContent>
        </Card>

        <Card className="opacity-70">
          <CardContent className="space-y-3 p-5">
            <div className="flex items-center gap-3">
              <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-muted text-muted-foreground">
                <Zap className="h-4 w-4" />
              </div>
              <div>
                <p className="font-medium">Wiki 技能包</p>
                <p className="text-xs text-muted-foreground">规划中 · 未暴露</p>
              </div>
            </div>
            <p className="text-xs leading-relaxed text-foreground-secondary">
              Wiki MCP 尚未实现，本版仅占位展示，导出按钮禁用。
            </p>
            <div className="flex flex-wrap gap-2">
              <Button variant="outline" size="sm" disabled>
                预览 SKILL.md
              </Button>
              <Button size="sm" disabled>
                导出技能包
              </Button>
            </div>
          </CardContent>
        </Card>
      </div>

      {/* RAG 工具清单 */}
      <div>
        <p className="mb-2 text-sm font-medium">RAG 工具清单 ({ragTools.length}) — 点击展开契约</p>
        <div className="space-y-1.5">
          {ragTools.map((spec) => (
            <button
              key={spec.name}
              onClick={() => setDetailTool(spec)}
              className="flex w-full items-center gap-3 rounded-lg border border-border-tertiary px-3 py-2.5 text-left hover:border-border-secondary hover:bg-background-secondary"
            >
              <code className="shrink-0 font-mono text-xs text-foreground">{spec.name}</code>
              <span className="flex-1 text-xs text-muted-foreground">{spec.description}</span>
              <ArrowRight className="h-3.5 w-3.5 shrink-0 text-muted-foreground" />
            </button>
          ))}
        </div>
      </div>

      {/* 工具详情弹窗 */}
      <Dialog open={!!detailTool} onOpenChange={(o) => !o && setDetailTool(null)}>
        <DialogContent className="max-h-[85vh] overflow-y-auto !w-[min(95vw,680px)] !max-w-[min(95vw,680px)]">
          {detailTool && (
            <>
              <DialogHeader>
                <DialogTitle className="break-all font-mono text-base">{detailTool.name}</DialogTitle>
                <DialogDescription>{detailTool.description}</DialogDescription>
              </DialogHeader>

              <div className="space-y-4">
                <div>
                  <p className="mb-2 text-sm font-medium">入参 (input)</p>
                  <div className="max-w-full overflow-x-auto rounded-lg border border-border-tertiary">
                    <table className="w-full text-xs">
                      <thead>
                        <tr className="text-muted-foreground">
                          <th className="px-2 py-1.5 text-left">参数</th>
                          <th className="px-2 py-1.5 text-left">类型</th>
                          <th className="px-2 py-1.5 text-left">必填</th>
                          <th className="px-2 py-1.5 text-left">默认</th>
                          <th className="px-2 py-1.5 text-left">说明</th>
                        </tr>
                      </thead>
                      <tbody>
                        {Object.entries(detailTool.inputSchema.properties ?? {}).map(([k, v]) => {
                          const required = detailTool.inputSchema.required ?? [];
                          return (
                            <tr key={k} className="border-t border-border-tertiary">
                              <td className="px-2 py-1.5 font-mono text-foreground">{k}</td>
                              <td className="px-2 py-1.5 text-foreground-secondary">{v.type ?? ""}</td>
                              <td
                                className={`px-2 py-1.5 ${
                                  required.includes(k) ? "text-success" : "text-muted-foreground"
                                }`}
                              >
                                {required.includes(k) ? "是" : "否"}
                              </td>
                              <td className="px-2 py-1.5 text-foreground-secondary">
                                {v.default !== undefined ? String(v.default) : "—"}
                              </td>
                              <td className="px-2 py-1.5 text-foreground-secondary">
                                {v.description ?? ""}
                              </td>
                            </tr>
                          );
                        })}
                      </tbody>
                    </table>
                  </div>
                </div>

                <div>
                  <p className="mb-2 text-sm font-medium">出参 (output)</p>
                  <div className="whitespace-pre-wrap break-words rounded-lg border border-border-tertiary bg-background-secondary p-3 text-xs leading-relaxed text-foreground-secondary">
                    {detailTool.returns}
                  </div>
                </div>

                <div>
                  <p className="mb-2 text-sm font-medium">Demo 脚本 (mcp_call.py)</p>
                  <CodeBlock code={demoCommand(detailTool)} lang="bash" />
                </div>
              </div>

              <DialogFooter>
                <Button variant="outline" onClick={() => setDetailTool(null)}>
                  关闭
                </Button>
              </DialogFooter>
            </>
          )}
        </DialogContent>
      </Dialog>

      {/* SKILL.md 预览弹窗 */}
      <Dialog open={!!previewMd} onOpenChange={(o) => !o && setPreviewMd(null)}>
        <DialogContent className="max-h-[85vh] overflow-y-auto !w-[min(95vw,1000px)] !max-w-[min(95vw,1000px)]">
          <DialogHeader>
            <DialogTitle>SKILL.md 预览</DialogTitle>
            <DialogDescription>导出包内 SKILL.md 的完整内容（与页面契约同源）。</DialogDescription>
          </DialogHeader>
          {previewMd && (
            <div className="max-h-[60vh] overflow-y-auto whitespace-pre-wrap break-words rounded-lg border border-border-tertiary bg-muted/40 p-4 font-mono text-xs leading-relaxed text-foreground-secondary">
              {previewMd}
            </div>
          )}
          <DialogFooter>
            <Button onClick={handleExport} disabled={exporting}>
              <Download className="h-3.5 w-3.5" />
              下载 .zip
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
