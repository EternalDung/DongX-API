//! RAG 内部行结构（FromRow），用于引擎层读取 `knowledge_bases` 等表。
//! 注意与 `commands/rag.rs` 的 `KnowledgeBase`（含统计、Serialize、面向前端）
//! 区分：本结构只映射表原始列，不含派生统计。

use sqlx::FromRow;

/// `knowledge_bases` 表行（不含 doc/chunk 统计）。
///
/// 额外携带摄入过滤字段（来自 010 迁移），供来源导入时作为全局默认值：
/// `exclude_dirs` / `exclude_files` / `include_file_types` / `embedding_batch_size`。
#[derive(Debug, Clone, FromRow)]
pub struct KnowledgeBaseRow {
    pub id: String,
    pub name: String,
    pub description: String,
    pub embedding_model: String,
    pub embedding_channel_id: String,
    pub status: i64,
    #[allow(dead_code)]
    pub created_at: String,
    #[allow(dead_code)]
    pub updated_at: String,
    /// 摄入时排除的目录（逗号分隔，NULL=不排除）。
    pub exclude_dirs: Option<String>,
    /// 摄入时排除的文件（逗号分隔，NULL=不排除）。
    pub exclude_files: Option<String>,
    /// 摄入时仅包含的文件类型（逗号分隔，NULL=全部）。
    pub include_file_types: Option<String>,
    /// 单次向量化批大小（NULL=取引擎默认）。
    pub embedding_batch_size: Option<i64>,
    /// 分块大小（字符数，0=引擎默认 1500）。
    pub chunk_size: i64,
    /// 分块重叠字符数（0=引擎默认 200）。
    pub chunk_overlap: i64,
}

/// 块的语义元数据（类型感知分块的产物）。
///
/// 这些字段不参与 FTS5 索引（仅 `content` 被索引），纯存储用于溯源与引用展示。
/// 普通文本块大多字段为 `None` / 0；代码块会带 `language` / `symbol_*` / 行范围；
/// Markdown 块带 `heading`。
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct ChunkMeta {
    pub heading: Option<String>,
    pub language: Option<String>,
    pub symbol_name: Option<String>,
    pub symbol_kind: Option<String>,
    pub signature: Option<String>,
    pub line_start: i64,
    pub line_end: i64,
    pub source_path: Option<String>,
}

/// 一个分块：内容 + 语义元数据。
#[derive(Debug, Clone)]
pub struct Chunk {
    pub content: String,
    pub meta: ChunkMeta,
}
