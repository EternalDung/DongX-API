import { useMemo } from "react";
import Prism from "prismjs";
import { Check, Copy } from "lucide-react";
import { useCopyToClipboard } from "@/components/ui/copy-button";
import "prismjs/components/prism-json";
import "prismjs/components/prism-toml";
// 代码示例用的多语言（注意：typescript 依赖 javascript，须在之后导入）
import "prismjs/components/prism-bash";
import "prismjs/components/prism-javascript";
import "prismjs/components/prism-typescript";
import "prismjs/components/prism-python";
import "prismjs/components/prism-java";
import "prismjs/components/prism-yaml";
import "prismjs/components/prism-markdown";
import "prismjs/themes/prism-tomorrow.css";

interface Props {
  code: string;
  lang: string;
  /** 在右上角悬浮显示复制按钮（如分片预览）。默认关闭。 */
  copyable?: boolean;
}

/// 扩展名 / 别名 → Prism 注册的语法名。
///
/// 后端按文件扩展名存 `language`（如 `py` / `ts` / `rs`），而 Prism 以完整名
/// 注册（`python` / `typescript` / `rust`）。这里做一层归一，避免 `py` 之类
/// 查不到而回退到 `clike`（几乎无高亮）。
const LANG_ALIAS: Record<string, string> = {
  py: "python",
  js: "javascript",
  jsx: "javascript",
  ts: "typescript",
  tsx: "typescript",
  sh: "bash",
  zsh: "bash",
  shell: "bash",
  yml: "yaml",
  md: "markdown",
  markdown: "markdown",
  rb: "ruby", // 未内置 → 回落 clike
  kt: "kotlin",
  rs: "rust",
  go: "go",
  java: "java",
  c: "c",
  cpp: "cpp",
  cs: "csharp",
  php: "php",
  swift: "swift",
  scala: "scala",
  dart: "dart",
  sql: "sql",
  html: "markup",
  htm: "markup",
  xml: "markup",
  svg: "markup",
  toml: "toml",
  json: "json",
  csv: "csv",
  log: "log",
};

/**
 * 语法高亮代码块（JSON / TOML / Bash / JS / TS / Python / YAML / Markdown 等）。
 * 沿用 prism-tomorrow 暗色主题，与配置文件预览区黑底风格一致。
 *
 * `copyable` 为 true 时在右上角悬浮复制按钮，复制反馈与全局 CopyButton
 * 一致（toast + 1.2s 高亮态），用于分片预览等只读场景。
 */
export function CodeBlock({ code, lang, copyable = false }: Props) {
  const { copied, copy } = useCopyToClipboard({ notify: true });

  const html = useMemo(() => {
    const key = LANG_ALIAS[lang.toLowerCase()] ?? lang;
    const grammar = Prism.languages[key] ?? Prism.languages.clike;
    return Prism.highlight(code, grammar, key);
  }, [code, lang]);

  return (
    <div className="relative">
      {copyable && (
        <button
          type="button"
          onClick={() => copy(code)}
          title={copied ? "已复制" : "复制"}
          className="absolute right-2.5 top-2.5 z-10 rounded-md border border-zinc-700 bg-zinc-900/90 p-1.5 text-zinc-400 transition-colors hover:border-zinc-500 hover:text-zinc-100"
        >
          {copied ? (
            <Check className="h-3.5 w-3.5 text-emerald-400" />
          ) : (
            <Copy className="h-3.5 w-3.5" />
          )}
        </button>
      )}
      <pre className="max-h-96 overflow-auto rounded-xl border bg-zinc-950 p-4 font-mono text-[12px] leading-relaxed">
        <code
          className={`language-${lang} text-zinc-100`}
          dangerouslySetInnerHTML={{ __html: html }}
        />
      </pre>
    </div>
  );
}
