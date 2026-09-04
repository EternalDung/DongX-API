import { useMemo } from "react";
import Prism from "prismjs";
import "prismjs/components/prism-json";
import "prismjs/components/prism-toml";
// 代码示例用的多语言（注意：typescript 依赖 javascript，须在之后导入）
import "prismjs/components/prism-bash";
import "prismjs/components/prism-javascript";
import "prismjs/components/prism-typescript";
import "prismjs/components/prism-python";
import "prismjs/components/prism-yaml";
import "prismjs/components/prism-markdown";
import "prismjs/themes/prism-tomorrow.css";

interface Props {
  code: string;
  lang: string;
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
 */
export function CodeBlock({ code, lang }: Props) {
  const html = useMemo(() => {
    const key = LANG_ALIAS[lang.toLowerCase()] ?? lang;
    const grammar = Prism.languages[key] ?? Prism.languages.clike;
    return Prism.highlight(code, grammar, key);
  }, [code, lang]);

  return (
    <pre className="max-h-96 overflow-auto rounded-xl border bg-zinc-950 p-4 font-mono text-[12px] leading-relaxed">
      <code
        className={`language-${lang} text-zinc-100`}
        dangerouslySetInnerHTML={{ __html: html }}
      />
    </pre>
  );
}
