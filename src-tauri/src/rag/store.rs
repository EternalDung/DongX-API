//! 知识库持久化：读取知识库行、写入文档与分块（含向量）。

use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};
use crate::rag::models::KnowledgeBaseRow;

/// 按 id 读取知识库；不存在返回 `NotFound`。
pub async fn get_kb(pool: &SqlitePool, id: &str) -> AppResult<KnowledgeBaseRow> {
    sqlx::query_as::<_, KnowledgeBaseRow>("SELECT * FROM knowledge_bases WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("知识库不存在: {}", id)))
}

/// 写入一份文档及其所有分块（向量以 JSON 落 `kb_chunks.embedding`）。
/// 返回新建的文档 id。
pub async fn insert_document(
    pool: &SqlitePool,
    kb_id: &str,
    title: &str,
    source_type: &str,
    source_ref: &str,
    embedding_model: &str,
    chunks: Vec<(String, Vec<f32>)>,
) -> AppResult<String> {
    let doc_id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();
    let chunk_count = chunks.len() as i32;
    let char_count: i32 = chunks.iter().map(|(c, _)| c.chars().count() as i32).sum();

    sqlx::query(
        "INSERT INTO kb_documents (id, kb_id, title, source_type, source_ref,
            char_count, chunk_count, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, ?, ?, 1, ?, ?)",
    )
    .bind(&doc_id)
    .bind(kb_id)
    .bind(title)
    .bind(source_type)
    .bind(source_ref)
    .bind(char_count)
    .bind(chunk_count)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await?;

    for (i, (content, emb)) in chunks.into_iter().enumerate() {
        let chunk_id = uuid::Uuid::new_v4().to_string();
        let emb_json =
            serde_json::to_string(&emb).map_err(|e| AppError::Internal(e.to_string()))?;
        sqlx::query(
            "INSERT INTO kb_chunks (id, kb_id, doc_id, seq, content, embedding, embedding_model, token_count, created_at)
             VALUES (?, ?, ?, ?, ?, ?, ?, 0, ?)",
        )
        .bind(&chunk_id)
        .bind(kb_id)
        .bind(&doc_id)
        .bind(i as i32)
        .bind(&content)
        .bind(emb_json)
        .bind(embedding_model)
        .bind(&now)
        .execute(pool)
        .await?;
    }
    Ok(doc_id)
}
