//! 文件类型识别：按扩展名 / 内容判定分块策略所需的类型。

/// 内容类型，决定分块策略（见 `chunk::split` 的分发逻辑）。
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum FileKind {
    /// Markdown：按标题切分
    Markdown,
    /// 代码：按 AST 符号切分；参数为扩展名（如 "rs" / "py"），
    /// 由 `code_parser` 判断是否支持符号级切分，不支持则回退普通切分。
    Code(String),
    /// 结构化文本（json / yaml / toml / xml / html / csv 等）；参数为格式
    /// 扩展名，写入 chunk 元数据的 `language` 供前端 CodeBlock 高亮
    /// （分块仍按普通文本切分，不做符号级切分）。
    Structured(String),
    /// 纯文本 / 其它
    Plain,
}

fn ext_of(filename: &str) -> String {
    filename.rsplit('.').next().unwrap_or("").to_lowercase()
}

/// 按文件名判定类型。
pub fn detect_kind_by_name(filename: &str) -> FileKind {
    let ext = ext_of(filename);
    match ext.as_str() {
        "md" | "markdown" => FileKind::Markdown,
        "json" | "yaml" | "yml" | "toml" | "xml" | "html" | "htm" | "csv" | "svg" => {
            FileKind::Structured(ext.clone())
        }
        "rs" | "py" | "ts" | "tsx" | "js" | "jsx" | "go" | "java" | "c" | "cpp" | "h" | "hpp"
        | "cc" | "cs" | "php" | "swift" | "kt" | "kts" | "rb" | "scala" | "dart" | "sh"
        | "bash" | "zsh" | "sql" | "r" | "lua" | "vim" | "proto" | "gradle" => {
            FileKind::Code(ext)
        }
        _ => FileKind::Plain,
    }
}

/// 按内容判定（用于无文件名的粘贴文本）。
/// 含 Markdown 标题（以 `# ` 开头的行）则按 Markdown 处理，否则纯文本。
pub fn detect_kind_by_content(content: &str) -> FileKind {
    if content
        .lines()
        .any(|l| l.trim_start().starts_with("# "))
    {
        FileKind::Markdown
    } else {
        FileKind::Plain
    }
}
