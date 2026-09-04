//! 知识库持久化：读取知识库行、写入文档与分块（含向量）。

use sqlx::{Row, SqlitePool};

use sha2::{Digest, Sha256};

use crate::error::{AppError, AppResult};
use crate::rag::chunk::estimate_tokens;
use crate::rag::models::{ChunkMeta, KnowledgeBaseRow};

/// 待写入的一个分块：内容 + 向量 + 语义元数据。
pub struct ChunkInput {
    pub content: String,
    pub embedding: Vec<f32>,
    pub meta: ChunkMeta,
}

/// 按 id 读取知识库；不存在返回 `NotFound`。
pub async fn get_kb(pool: &SqlitePool, id: &str) -> AppResult<KnowledgeBaseRow> {
    sqlx::query_as::<_, KnowledgeBaseRow>("SELECT * FROM knowledge_bases WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("知识库不存在: {}", id)))
}

/// 计算文本的内容哈希（SHA-256 十六进制），用于摄入前去重。
pub fn content_hash(text: &str) -> String {
    let mut h = Sha256::new();
    h.update(text.as_bytes());
    format!("{:x}", h.finalize())
}

/// 按内容哈希查找知识库内已就绪（status=1）的文档，用于摄入前去重。
/// 返回 `(doc_id, chunk_count)`；无重复返回 `None`。
/// 仅匹配「就绪」状态，避免与处理中（0）或失败（2）的残留行误撞。
pub async fn find_document_by_hash(
    pool: &SqlitePool,
    kb_id: &str,
    content_hash: &str,
) -> AppResult<Option<(String, i32)>> {
    let row = sqlx::query(
        "SELECT id, chunk_count FROM kb_documents \
         WHERE kb_id = ? AND content_hash = ? AND status = 1 LIMIT 1",
    )
    .bind(kb_id)
    .bind(content_hash)
    .fetch_optional(pool)
    .await?;
    Ok(row.map(|r| {
        let id: String = r.try_get("id").unwrap_or_default();
        let cc: i32 = r.try_get("chunk_count").unwrap_or(0);
        (id, cc)
    }))
}

/// 写入一份文档及其所有分块（向量以 JSON 落 `kb_chunks.embedding`，元数据落对应列）。
/// 返回新建的文档 id。
pub async fn insert_document(
    pool: &SqlitePool,
    kb_id: &str,
    title: &str,
    source_type: &str,
    source_ref: &str,
    embedding_model: &str,
    file_size: i64,
    token_count: i64,
    content_hash: &str,
    chunks: Vec<ChunkInput>,
) -> AppResult<String> {
    let doc_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let chunk_count = chunks.len() as i32;
    let char_count: i32 = chunks.iter().map(|c| c.content.chars().count() as i32).sum();

    sqlx::query(
        "INSERT INTO kb_documents (id, kb_id, title, source_type, source_ref,
            char_count, chunk_count, status, file_size, token_count, content_hash,
            created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?, ?, ?, ?)",
    )
    .bind(&doc_id)
    .bind(kb_id)
    .bind(title)
    .bind(source_type)
    .bind(source_ref)
    .bind(char_count)
    .bind(chunk_count)
    .bind(file_size)
    .bind(token_count)
    .bind(content_hash)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    for (i, ci) in chunks.into_iter().enumerate() {
        let chunk_id = uuid::Uuid::new_v4().to_string();
        let emb_json =
            serde_json::to_string(&ci.embedding).map_err(|e| AppError::Internal(e.to_string()))?;
        sqlx::query(
            "INSERT INTO kb_chunks (\
                id, kb_id, doc_id, seq, content, embedding, embedding_model, token_count, \
                heading, language, symbol_name, symbol_kind, signature, line_start, line_end, source_path, \
                created_at) \
             VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?)",
        )
        .bind(&chunk_id)
        .bind(kb_id)
        .bind(&doc_id)
        .bind(i as i32)
        .bind(&ci.content)
        .bind(emb_json)
        .bind(embedding_model)
        // token_count：每分块按 CJK 感知近似估算（与 chunk.rs::estimate_tokens 保持一致），
        // 老数据落库时为 0，新摄入自动按内容估算；显示在「查看分片」下钻的 token 列。
        .bind(estimate_tokens(&ci.content) as i32)
        .bind(&ci.meta.heading)
        .bind(&ci.meta.language)
        .bind(&ci.meta.symbol_name)
        .bind(&ci.meta.symbol_kind)
        .bind(&ci.meta.signature)
        .bind(ci.meta.line_start)
        .bind(ci.meta.line_end)
        .bind(&ci.meta.source_path)
        .bind(&now)
        .execute(pool)
        .await?;
        // 同步维护 FTS5 全文索引（独立表，应用层显式写入）
        sqlx::query(
            "INSERT INTO kb_chunks_fts (chunk_id, kb_id, content) VALUES (?, ?, ?)",
        )
        .bind(&chunk_id)
        .bind(kb_id)
        .bind(&ci.content)
        .execute(pool)
        .await?;
    }
    Ok(doc_id)
}

/// 删除某文档的全部分块，并同步清理 FTS5 索引。
/// 须在删除 `kb_documents` 行之前调用（FTS 清理依赖 kb_chunks 的 doc_id 反查）。
pub async fn purge_document_chunks(pool: &SqlitePool, doc_id: &str) -> AppResult<()> {
    sqlx::query(
        "DELETE FROM kb_chunks_fts WHERE chunk_id IN (SELECT id FROM kb_chunks WHERE doc_id = ?)",
    )
    .bind(doc_id)
    .execute(pool)
    .await?;
    sqlx::query("DELETE FROM kb_chunks WHERE doc_id = ?")
        .bind(doc_id)
        .execute(pool)
        .await?;
    Ok(())
}

/// 删除某知识库的全部分块，并同步清理 FTS5 索引。
pub async fn purge_kb_chunks(pool: &SqlitePool, kb_id: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM kb_chunks_fts WHERE kb_id = ?")
        .bind(kb_id)
        .execute(pool)
        .await?;
    sqlx::query("DELETE FROM kb_chunks WHERE kb_id = ?")
        .bind(kb_id)
        .execute(pool)
        .await?;
    Ok(())
}
