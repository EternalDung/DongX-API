import { useCallback, useEffect, useState } from "react";
import {
  Server,
  Copy,
  Check,
  Download,
  ChevronDown,
  FileText,
} from "lucide-react";
import { Card, CardContent } from "@/components/ui/card";
import { Button } from "@/components/ui/button";
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
import { CopyButton } from "@/components/ui/copy-button";
import { mcpApi } from "@/lib/api";
import { cn } from "@/lib/utils";
import type { McpStatus } from "@/types";
import { createZip, downloadBlob } from "@/lib/zip";
import { ragSkills } from "./ragSkillSpec";
import { wikiSkills } from "./wikiSkillSpec";
import type { SkillToolSpec, JsonProp } from "./skillTypes";

// ---------------------------------------------------------------------------
// 技能包定义：RAG / Wiki 共用同一套「卡片 + 工具清单 + 导出」逻辑。
// 新增技能包只需往下面数组里加一项，无需改渲染代码。
// ---------------------------------------------------------------------------

const REPO_URL = "https://github.com/EternalDung/DongX-API";

/** 一个可导出的技能包：决定 SKILL.md frontmatter、卡片文案与工具清单。 */
interface SkillPackage {
  id: "rag" | "wiki";
  /** frontmatter `name` */
  pkgName: string;
  /** frontmatter `description` */
  desc: string;
  /** frontmatter metadata.category */
  category: string;
  /** SKILL.md 一级标题 */
  mdTitle: string;
  /** SKILL.md 导语 */
  mdIntro: string;
  /** SKILL.md 前置条件（端点行由代码统一加，这里只写额外条件） */
  prereq: string[];
  zipName: string;
  cardTitle: string;
  cardSubtitle: string;
  cardSummary: string;
  tools: SkillToolSpec[];
}

const SKILL_PACKAGES: SkillPackage[] = [
  {
    id: "rag",
    pkgName: "dongx-rag-skill",
    desc: "DongX 本地网关的 RAG 知识库技能。通过 MCP (Streamable HTTP) 暴露语义检索、RAG 问答、文档读取与统计，供客户端 Agent 直接调用。触发词：知识库检索、RAG 问答、搜文档、读文档、知识库统计。",
    category: "knowledge-base",
    mdTitle: "# DongX RAG 技能包",
    mdIntro:
      "通过 MCP (Streamable HTTP) 连接 DongX 本地网关，提供 RAG 语义检索、RAG 问答、文档读取与统计。所有调用经 `scripts/mcp_call.py`。",
    prereq: ["至少一个知识库已开启「MCP 暴露」（KB 开关处开启）"],
    zipName: "dongx-rag-skill.zip",
    cardTitle: "RAG 知识库技能包",
    cardSubtitle: "检索增强",
    cardSummary:
      "语义检索、RAG 问答、文档读取与统计。每个工具都带完整入参 / 出参契约。",
    tools: ragSkills,
  },
  {
    id: "wiki",
    pkgName: "dongx-wiki-skill",
    desc: "DongX 本地网关的 Wiki 知识库技能。通过 MCP (Streamable HTTP) 暴露 Wiki 项目浏览、页面读取、关键词检索与问答，供客户端 Agent 直接调用。触发词：Wiki 检索、Wiki 问答、读 Wiki 页面、Wiki 项目列表。",
    category: "wiki",
    mdTitle: "# DongX Wiki 技能包",
    mdIntro:
      "通过 MCP (Streamable HTTP) 连接 DongX 本地网关，提供 Wiki 项目浏览、页面读取、关键词检索与问答。所有调用经 `scripts/mcp_call.py`。",
    prereq: ["至少一个 Wiki 项目已开启「MCP 暴露」（项目开关处开启）"],
    zipName: "dongx-wiki-skill.zip",
    cardTitle: "Wiki 知识库技能包",
    cardSubtitle: "结构化知识",
    cardSummary:
      "Wiki 项目/页面浏览、关键词检索、Wiki 问答与源资料查看。每个工具都带完整入参 / 出参契约。",
    tools: wikiSkills,
  },
];

const DEFAULT_ENDPOINT = "http://127.0.0.1:9842/mcp";

/** 为工具生成一个可运行的示例入参（必填项填占位值，带默认值的可选项一并带上）。 */
function sampleValue(key: string, prop: JsonProp): unknown {
  if (key === "kb_id") return "kb_xxx";
  if (key === "project_id") return "wiki_xxx";
  if (key === "slug") return "index";
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

function exampleArgs(spec: SkillToolSpec): string {
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

function demoCommand(spec: SkillToolSpec): string {
  return `python3 scripts/mcp_call.py ${spec.name} '${exampleArgs(spec)}'`;
}

/** 生成导出用的 SKILL.md（frontmatter + 工具契约 + demo），与页面展示同源。 */
function buildSkillMd(pkg: SkillPackage, endpoint: string): string {
  const ep = endpoint || DEFAULT_ENDPOINT;
  const lines: string[] = [];
  lines.push("---");
  lines.push(`name: ${pkg.pkgName}`);
  lines.push(`description: ${pkg.desc}`);
  lines.push("license: MIT");
  lines.push("metadata:");
  lines.push("  author: dongx");
  lines.push('  version: "1.0.0"');
  lines.push(`  category: ${pkg.category}`);
  lines.push(`  homepage: ${REPO_URL}`);
  lines.push("---");
  lines.push("");
  lines.push(pkg.mdTitle);
  lines.push("");
  lines.push(pkg.mdIntro);
  lines.push("");
  lines.push("## 前置条件");
  lines.push("");
  lines.push(`- DongX 网关运行中，MCP 端点：${ep}`);
  for (const p of pkg.prereq) {
    lines.push(`- ${p}`);
  }
  lines.push("");
  lines.push("## 使用方式");
  lines.push("");
  lines.push("```bash");
  lines.push("python3 scripts/mcp_call.py <tool_name> '<json_arguments>'");
  lines.push("```");
  lines.push("");
  lines.push("## 工具清单");
  lines.push("");

  for (const spec of pkg.tools) {
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

// ---------------------------------------------------------------------------
// 工具清单：手风琴（单开，点击就地展开契约，不使用弹窗）
// ---------------------------------------------------------------------------

function ToolAccordion({
  pkg,
  expandedKey,
  onToggle,
}: {
  pkg: SkillPackage;
  expandedKey: string | null;
  onToggle: (key: string) => void;
}) {
  return (
    <div>
      <div className="mb-2 flex items-baseline gap-2">
        <p className="text-sm font-medium">{pkg.cardTitle}</p>
        <span className="text-xs text-muted-foreground">
          {pkg.tools.length} 个工具
        </span>
      </div>

      <div className="overflow-hidden rounded-lg border border-border-tertiary bg-card">
        {pkg.tools.map((spec, i) => {
          const key = `${pkg.id}::${spec.name}`;
          const open = expandedKey === key;
          const props = spec.inputSchema.properties ?? {};
          const required = spec.inputSchema.required ?? [];
          const entries = Object.entries(props);
          const cmd = demoCommand(spec);

          return (
            <div
              key={spec.name}
              className={cn(
                i < pkg.tools.length - 1 && "border-b border-border-tertiary",
              )}
            >
              <button
                type="button"
                onClick={() => onToggle(key)}
                className={cn(
                  "flex w-full items-center gap-3 px-3.5 py-2.5 text-left transition-colors",
                  open ? "bg-background-secondary" : "hover:bg-background-secondary",
                )}
              >
                <span
                  className={cn(
                    "flex h-6 w-6 shrink-0 items-center justify-center rounded-md font-mono text-[11px]",
                    open
                      ? "bg-info/15 text-info"
                      : "bg-muted text-muted-foreground",
                  )}
                >
                  {String(i + 1).padStart(2, "0")}
                </span>
                <code className="shrink-0 font-mono text-xs text-foreground">
                  {spec.name}
                </code>
                <span className="min-w-0 flex-1 truncate text-xs text-muted-foreground">
                  {spec.description}
                </span>
                <ChevronDown
                  className={cn(
                    "h-3.5 w-3.5 shrink-0 text-muted-foreground transition-transform",
                    open && "rotate-180",
                  )}
                />
              </button>

              {open && (
                <div className="space-y-3 border-t border-border-tertiary px-3.5 py-3">
                  <div>
                    <p className="mb-1.5 text-sm font-medium">入参</p>
                    {entries.length === 0 ? (
                      <div className="rounded-lg border border-border-tertiary bg-background-secondary px-3 py-2 text-xs text-muted-foreground">
                        无入参，直接传 {"{}"} 即可
                      </div>
                    ) : (
                      <div className="max-w-full overflow-x-auto rounded-lg border border-border-tertiary">
                        <table className="w-full text-xs">
                          <thead>
                            <tr className="text-muted-foreground">
                              <th className="px-2.5 py-1.5 text-left font-normal">
                                参数
                              </th>
                              <th className="px-2 py-1.5 text-left font-normal">
                                类型
                              </th>
                              <th className="px-2 py-1.5 text-left font-normal">
                                必填
                              </th>
                              <th className="px-2 py-1.5 text-left font-normal">
                                默认
                              </th>
                              <th className="px-2 py-1.5 text-left font-normal">
                                说明
                              </th>
                            </tr>
                          </thead>
                          <tbody>
                            {entries.map(([k, v]) => (
                              <tr
                                key={k}
                                className="border-t border-border-tertiary"
                              >
                                <td className="px-2.5 py-1.5 font-mono text-foreground">
                                  {k}
                                </td>
                                <td className="px-2 py-1.5 font-mono text-[11px] text-foreground-secondary">
                                  {v.type ?? ""}
                                </td>
                                <td
                                  className={cn(
                                    "px-2 py-1.5",
                                    required.includes(k)
                                      ? "text-success"
                                      : "text-muted-foreground",
                                  )}
                                >
                                  {required.includes(k) ? "是" : "否"}
                                </td>
                                <td className="px-2 py-1.5 font-mono text-foreground-secondary">
                                  {v.default !== undefined
                                    ? String(v.default)
                                    : "—"}
                                </td>
                                <td className="px-2 py-1.5 text-foreground-secondary">
                                  {v.description ?? ""}
                                </td>
                              </tr>
                            ))}
                          </tbody>
                        </table>
                      </div>
                    )}
                  </div>

                  <div>
                    <p className="mb-1.5 text-sm font-medium">出参</p>
                    <div className="whitespace-pre-wrap break-words rounded-lg border border-border-tertiary bg-background-secondary p-3 text-xs leading-relaxed text-foreground-secondary">
                      {spec.returns}
                    </div>
                  </div>

                  <div>
                    <p className="mb-1.5 text-sm font-medium">示例</p>
                    <div className="flex items-center gap-2 rounded-lg border border-border-tertiary bg-background-secondary px-3 py-2">
                      <code className="min-w-0 flex-1 overflow-x-auto whitespace-nowrap font-mono text-[11px] text-foreground">
                        {cmd}
                      </code>
                      <CopyButton value={cmd} variant="pill" label="命令" />
                    </div>
                  </div>
                </div>
              )}
            </div>
          );
        })}
      </div>
    </div>
  );
}

export function SkillTab() {
  const [status, setStatus] = useState<McpStatus | null>(null);
  const [loading, setLoading] = useState(true);
  const [expandedKey, setExpandedKey] = useState<string | null>(null);
  const [previewPkg, setPreviewPkg] = useState<SkillPackage | null>(null);
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

  // 工具契约来自静态技能包（ragSkillSpec / wikiSkillSpec），仅端点随运行态变化。
  const endpoint = status?.endpoint ?? DEFAULT_ENDPOINT;
  const totalTools = SKILL_PACKAGES.reduce((n, p) => n + p.tools.length, 0);

  const handleCopyEndpoint = useCallback(async () => {
    try {
      await navigator.clipboard.writeText(endpoint);
      setCopied(true);
      setTimeout(() => setCopied(false), 1500);
    } catch {
      /* ignore */
    }
  }, [endpoint]);

  const handleExport = useCallback(
    (pkg: SkillPackage) => {
      if (!pkg.tools.length) return;
      setExporting(true);
      try {
        const md = buildSkillMd(pkg, endpoint);
        const config = JSON.stringify({ mcp_url: endpoint }, null, 2);
        const blob = createZip([
          { name: "SKILL.md", data: md },
          { name: "scripts/mcp_call.py", data: MCP_CALL_PY },
          { name: "config.json", data: config },
        ]);
        downloadBlob(blob, pkg.zipName);
        toast.success(`技能包已导出：${pkg.zipName} 已保存到下载目录（Downloads）`);
      } finally {
        setExporting(false);
      }
    },
    [endpoint, toast],
  );

  const handlePreview = useCallback((pkg: SkillPackage) => {
    if (!pkg.tools.length) return;
    setPreviewPkg(pkg);
  }, []);

  // 单开：点同一项收起，点其它项则替换。
  const handleToggle = useCallback((key: string) => {
    setExpandedKey((cur) => (cur === key ? null : key));
  }, []);

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
          {totalTools} 个工具
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
        {SKILL_PACKAGES.map((pkg) => (
          <Card key={pkg.id}>
            <CardContent className="space-y-3 p-5">
              <div className="flex items-center gap-3">
                <div className="flex h-9 w-9 items-center justify-center rounded-lg bg-info/15 text-info">
                  <Server className="h-4 w-4" />
                </div>
                <div>
                  <p className="font-medium">{pkg.cardTitle}</p>
                  <p className="text-xs text-muted-foreground">
                    {pkg.cardSubtitle} · {pkg.tools.length} 工具
                  </p>
                </div>
              </div>
              <p className="text-xs leading-relaxed text-foreground-secondary">
                {pkg.cardSummary}
              </p>
              <div className="flex flex-wrap gap-2">
                <Button
                  variant="outline"
                  size="sm"
                  onClick={() => handlePreview(pkg)}
                  disabled={!pkg.tools.length}
                >
                  <FileText className="h-3.5 w-3.5" />
                  预览 SKILL.md
                </Button>
                <Button
                  size="sm"
                  onClick={() => handleExport(pkg)}
                  disabled={!pkg.tools.length || exporting}
                >
                  <Download className="h-3.5 w-3.5" />
                  {exporting ? "导出中…" : "导出技能包"}
                </Button>
              </div>
            </CardContent>
          </Card>
        ))}
      </div>

      {/* 各技能包的工具清单（手风琴，单开） */}
      <div className="space-y-5">
        {SKILL_PACKAGES.map((pkg) => (
          <ToolAccordion
            key={pkg.id}
            pkg={pkg}
            expandedKey={expandedKey}
            onToggle={handleToggle}
          />
        ))}
      </div>

      {/* SKILL.md 预览弹窗：展示原始 Markdown（与导出包字节一致） */}
      <Dialog open={!!previewPkg} onOpenChange={(o) => !o && setPreviewPkg(null)}>
        <DialogContent className="max-h-[85vh] overflow-y-auto !w-[min(95vw,1000px)] !max-w-[min(95vw,1000px)]">
          <DialogHeader>
            <DialogTitle>SKILL.md 预览 — {previewPkg?.cardTitle ?? ""}</DialogTitle>
            <DialogDescription>导出包内 SKILL.md 的完整内容（与页面契约同源）。</DialogDescription>
          </DialogHeader>
          {previewPkg && (
            <div className="max-h-[60vh] overflow-y-auto whitespace-pre-wrap break-words rounded-lg border border-border-tertiary bg-muted/40 p-4 font-mono text-xs leading-relaxed text-foreground-secondary">
              {buildSkillMd(previewPkg, endpoint)}
            </div>
          )}
          <DialogFooter>
            <Button
              onClick={() => previewPkg && handleExport(previewPkg)}
              disabled={exporting || !previewPkg}
            >
              <Download className="h-3.5 w-3.5" />
              下载 .zip
            </Button>
          </DialogFooter>
        </DialogContent>
      </Dialog>
    </div>
  );
}
