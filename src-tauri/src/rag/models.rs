//! RAG 内部行结构（FromRow），用于引擎层读取 `knowledge_bases` 等表。
//! 注意与 `commands/rag.rs` 的 `KnowledgeBase`（含统计、Serialize、面向前端）
//! 区分：本结构只映射表原始列，不含派生统计。

use sqlx::FromRow;

/// `knowledge_bases` 表行（不含 doc/chunk 统计）。
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
}
