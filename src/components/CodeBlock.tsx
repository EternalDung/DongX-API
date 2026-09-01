import { useMemo } from "react";
import Prism from "prismjs";
import "prismjs/components/prism-json";
import "prismjs/components/prism-toml";
// 代码示例用的多语言（注意：typescript 依赖 javascript，须在之后导入）
import "prismjs/components/prism-bash";
import "prismjs/components/prism-javascript";
import "prismjs/components/prism-typescript";
import "prismjs/components/prism-python";
import "prismjs/themes/prism-tomorrow.css";

interface Props {
  code: string;
  /** 语言标识：json / toml / bash / javascript / typescript / python 等 */
  lang: string;
}

/**
 * 语法高亮代码块（JSON / TOML / Bash / JS / TS / Python 等）。
 * 沿用 prism-tomorrow 暗色主题，与配置文件预览区黑底风格一致。
 */
export function CodeBlock({ code, lang }: Props) {
  const html = useMemo(() => {
    const grammar = Prism.languages[lang] ?? Prism.languages.clike;
    return Prism.highlight(code, grammar, lang);
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
