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
    pub created_at: String,
    pub updated_at: String,
    /// 摄入时排除的目录（逗号分隔，NULL=不排除）。
    pub exclude_dirs: Option<String>,
    /// 摄入时排除的文件（逗号分隔，NULL=不排除）。
    pub exclude_files: Option<String>,
    /// 摄入时仅包含的文件类型（逗号分隔，NULL=全部）。
    pub include_file_types: Option<String>,
    /// 单次向量化批大小（NULL=取引擎默认）。
    pub embedding_batch_size: Option<i64>,
}
