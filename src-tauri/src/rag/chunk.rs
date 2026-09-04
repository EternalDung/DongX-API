//! 类型感知分块：按文件类型路由到不同策略。
//!
//! - Markdown：按标题切分，整段不拆散；超大段再按尺寸细分。
//! - 代码：优先按 AST 符号（函数 / 类 / 方法）切分；不支持的语言回退普通切分。
//! - 结构化 / 纯文本：按尺寸 + 重叠切分。
//!
//! 每个块携带 [`ChunkMeta`]：标题 / 语言 / 符号 / 行范围 / 来源路径，
//! 供检索后溯源（"这段来自 `auth.rs:42-88` 的 `UserService.login`"）。

use std::collections::HashSet;

use crate::rag::code_parser::{self, Symbol};
use crate::rag::models::{Chunk, ChunkMeta};
use crate::rag::parser::FileKind;

/// 分块尺寸配置（字符为单位，沿用 DongX 已有的 1500 / 200 习惯）。
#[derive(Debug, Clone)]
pub struct SplitConfig {
    pub max_chars: usize,
    pub overlap_chars: usize,
}

impl Default for SplitConfig {
    fn default() -> Self {
        Self {
            max_chars: 1500,
            overlap_chars: 200,
        }
    }
}

impl SplitConfig {
    /// 由知识库配置构造：任一为 0 时回落引擎默认（1500 / 200）。
    pub fn from_kb(chunk_size: i64, chunk_overlap: i64) -> Self {
        Self {
            max_chars: if chunk_size > 0 { chunk_size as usize } else { 1500 },
            overlap_chars: if chunk_overlap > 0 {
                chunk_overlap as usize
            } else {
                200
            },
        }
    }
}

/// 主分发器：按类型把文本切成带元数据的块。
///
/// `config` 来自知识库的分块配置（大小 / 重叠），由调用方用
/// [`SplitConfig::from_kb`] 构造；传 `&SplitConfig::default()` 即引擎默认。
pub fn split(content: &str, kind: FileKind, source_path: Option<&str>, config: &SplitConfig) -> Vec<Chunk> {
    let base_meta = ChunkMeta {
        source_path: source_path.map(|s| s.to_string()),
        ..Default::default()
    };
    match kind {
        FileKind::Markdown => split_markdown(content, config, &base_meta),
        FileKind::Code(ext) => {
            let meta = ChunkMeta {
                language: Some(ext.clone()),
                ..base_meta
            };
            if code_parser::is_supported_language(&ext) {
                let symbols = code_parser::extract_symbols(source_path.unwrap_or(""), content);
                split_code_by_symbols(content, &symbols, config, &meta)
            } else {
                split_text(content, config, &meta)
            }
        }
        FileKind::Structured | FileKind::Plain => {
            split_text(content, config, &base_meta)
        }
    }
}

/// 按尺寸 + 重叠切分（行对齐，避免切断句子）。
fn split_text(content: &str, config: &SplitConfig, meta: &ChunkMeta) -> Vec<Chunk> {
    let lines: Vec<&str> = content.lines().collect();
    let total = lines.len();
    let mut chunks = Vec::new();
    let mut current = String::new();
    let mut current_chars = 0usize;
    let mut chunk_start = 0usize;

    for (idx, line) in lines.iter().enumerate() {
        let line_text = format!("{}\n", line);
        let lc = line_text.chars().count();
        if current_chars + lc > config.max_chars && !current.is_empty() {
            let c = current.trim().to_string();
            if !c.is_empty() {
                chunks.push(Chunk {
                    content: c,
                    meta: ChunkMeta {
                        line_start: chunk_start as i64,
                        line_end: idx as i64,
                        ..meta.clone()
                    },
                });
            }
            // 重叠：保留尾部 overlap_chars
            if config.overlap_chars > 0 && current.chars().count() > config.overlap_chars {
                let skip = current.chars().count() - config.overlap_chars;
                current = current.chars().skip(skip).collect();
                current_chars = current.chars().count();
            } else {
                current.clear();
                current_chars = 0;
            }
            chunk_start = idx;
        }
        current.push_str(&line_text);
        current_chars += lc;
    }
    let c = current.trim().to_string();
    if !c.is_empty() {
        chunks.push(Chunk {
            content: c,
            meta: ChunkMeta {
                line_start: chunk_start as i64,
                line_end: total as i64,
                ..meta.clone()
            },
        });
    }
    chunks
}

/// 按 Markdown 标题切分（每个标题起一个块，整段不拆散）；单个超大段再细分。
fn split_markdown(content: &str, config: &SplitConfig, meta: &ChunkMeta) -> Vec<Chunk> {
    let lines: Vec<&str> = content.lines().collect();
    let mut raw: Vec<(String, Option<String>, usize)> = Vec::new(); // (section, heading, start_line)
    let mut section = String::new();
    let mut current_heading: Option<String> = None;
    let mut section_start = 0usize;

    for (idx, line) in lines.iter().enumerate() {
        if line.starts_with('#') {
            // 遇到新标题：先把上一个 section（属于上一个 heading）落成一个块
            let sec = section.trim().to_string();
            if !sec.is_empty() {
                raw.push((sec, current_heading.clone(), section_start));
            }
            section.clear();
            section_start = idx;
            let trimmed = line.trim_start_matches('#').trim().to_string();
            current_heading = Some(trimmed);
        }
        section.push_str(line);
        section.push('\n');
    }
    // 末尾 flush
    let sec = section.trim().to_string();
    if !sec.is_empty() {
        raw.push((sec, current_heading.clone(), section_start));
    }

    // 超大段再按普通尺寸细分（保留 heading 元数据）
    let mut chunks = Vec::new();
    for (c, heading, start) in raw {
        if c.chars().count() > config.max_chars * 2 {
            let sub_meta = ChunkMeta {
                heading: heading.clone(),
                ..meta.clone()
            };
            chunks.extend(split_text(&c, config, &sub_meta));
        } else {
            chunks.push(Chunk {
                content: c,
                meta: ChunkMeta {
                    heading,
                    line_start: start as i64,
                    line_end: lines.len() as i64,
                    ..meta.clone()
                },
            });
        }
    }
    chunks
}

/// 按 AST 符号切分代码；符号为空或语言不支持时回退普通切分。
fn split_code_by_symbols(
    content: &str,
    symbols: &[Symbol],
    config: &SplitConfig,
    meta: &ChunkMeta,
) -> Vec<Chunk> {
    if symbols.is_empty() {
        return split_text(content, config, meta);
    }

    let lines: Vec<&str> = content.lines().collect();
    let mut chunks = Vec::new();
    let mut covered: HashSet<usize> = HashSet::new();

    for sym in symbols {
        let start = sym.start_line.min(lines.len().saturating_sub(1));
        let end = sym.end_line.min(lines.len().saturating_sub(1));
        if end < start {
            continue;
        }
        let chunk_content: String = lines[start..=end].join("\n");
        let chars = chunk_content.chars().count();

        let sym_meta = ChunkMeta {
            heading: Some(format!("{}: {}", sym.kind.as_str(), sym.name)),
            language: meta.language.clone(),
            symbol_name: Some(sym.name.clone()),
            symbol_kind: Some(sym.kind.as_str().to_string()),
            signature: sym.signature.clone(),
            line_start: start as i64,
            line_end: end as i64,
            source_path: meta.source_path.clone(),
        };

        // 超大符号内部再切分
        if chars > config.max_chars * 3 {
            chunks.extend(split_text(&chunk_content, config, &sym_meta));
        } else {
            chunks.push(Chunk {
                content: chunk_content,
                meta: sym_meta,
            });
        }
        for i in start..=end {
            covered.insert(i);
        }
    }

    // 未被符号覆盖的行（import / 全局语句 / 注释）→ 收集为孤儿块
    let mut orphan = String::new();
    let mut orphan_start: Option<usize> = None;
    for (idx, line) in lines.iter().enumerate() {
        if !covered.contains(&idx) {
            if orphan_start.is_none() {
                orphan_start = Some(idx);
            }
            orphan.push_str(line);
            orphan.push('\n');
        } else if !orphan.is_empty() {
            let c = orphan.trim().to_string();
            if !c.is_empty() {
                let ostart = orphan_start.unwrap();
                chunks.push(Chunk {
                    content: c,
                    meta: ChunkMeta {
                        line_start: ostart as i64,
                        line_end: idx as i64,
                        ..meta.clone()
                    },
                });
            }
            orphan.clear();
            orphan_start = None;
        }
    }
    if !orphan.is_empty() {
        let c = orphan.trim().to_string();
        if !c.is_empty() {
            let ostart = orphan_start.unwrap_or(0);
            chunks.push(Chunk {
                content: c,
                meta: ChunkMeta {
                    line_start: ostart as i64,
                    line_end: lines.len() as i64,
                    ..meta.clone()
                },
            });
        }
    }

    chunks.sort_by_key(|c| c.meta.line_start);
    chunks
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn split_plain_respects_size_and_overlap() {
        let text: String = (0..200).map(|i| format!("line {} content here\n", i)).collect();
        let chunks = split(&text, FileKind::Plain, None, &SplitConfig::default());
        assert!(!chunks.is_empty());
        for c in &chunks {
            assert!(c.content.chars().count() <= SplitConfig::default().max_chars + 50);
        }
        assert_eq!(chunks[0].meta.line_start, 0);
    }

    #[test]
    fn split_markdown_keeps_heading() {
        let md = "# Title\nintro text\n\n## Section A\nbody A\n\n## Section B\nbody B\n";
        let chunks = split(md, FileKind::Markdown, None, &SplitConfig::default());
        assert!(chunks.iter().any(|c| c.meta.heading.as_deref() == Some("Title")));
        assert!(chunks.iter().any(|c| c.meta.heading.as_deref() == Some("Section A")));
    }

    #[test]
    fn split_code_symbol_aware_for_rust() {
        let code = "pub struct User {\n    pub name: String,\n}\n\nimpl User {\n    pub fn new(name: String) -> Self {\n        Self { name }\n    }\n}\n\nfn main() {\n    let u = User::new(\"x\".into());\n}\n";
        let chunks = split(code, FileKind::Code("rs".to_string()), Some("src/user.rs"), &SplitConfig::default());
        assert!(chunks
            .iter()
            .any(|c| c.meta.symbol_name.as_deref() == Some("User")
                && c.meta.symbol_kind.as_deref() == Some("struct")));
        assert!(chunks
            .iter()
            .any(|c| c.meta.symbol_name.as_deref() == Some("new")
                && c.meta.symbol_kind.as_deref() == Some("method")));
        assert!(chunks.iter().all(|c| c.meta.source_path.as_deref() == Some("src/user.rs")));
        assert!(chunks.iter().all(|c| c.meta.language.as_deref() == Some("rs")));
    }

    #[test]
    fn split_code_unsupported_falls_back_to_text() {
        // "kt" 当前未接入 tree-sitter 语法，应回退普通切分（不报错、无符号元数据）
        let code = "fun main() { println(\"hi\") }";
        let chunks = split(code, FileKind::Code("kt".to_string()), Some("Main.kt"), &SplitConfig::default());
        assert!(!chunks.is_empty());
        assert!(chunks.iter().all(|c| c.meta.symbol_name.is_none()));
    }
}
