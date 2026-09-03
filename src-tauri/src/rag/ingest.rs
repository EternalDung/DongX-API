//! 摄入：分块 → 向量化 → 持久化。供 Tauri 命令 `ingest_kb_text` 调用。

use sqlx::SqlitePool;

use serde::Serialize;

use crate::error::AppError;
use crate::rag::chunk::chunk_text;
use crate::rag::embed::embed_texts;
use crate::rag::store::{get_kb, insert_document};

/// 摄入结果。
#[derive(Debug, Serialize)]
pub struct IngestResult {
    pub document_id: String,
    pub chunk_count: usize,
}

/// 把一段文本摄入指定知识库：分块后用该知识库绑定的嵌入渠道向量化并落库。
pub async fn ingest_text(
    pool: &SqlitePool,
    kb_id: &str,
    title: &str,
    text: &str,
) -> Result<IngestResult, AppError> {
    if text.trim().is_empty() {
        return Err(AppError::Validation("待摄入文本为空".into()));
    }
    let kb = get_kb(pool, kb_id).await?;
    let chunks = chunk_text(text, 1500, 200);
    if chunks.is_empty() {
        return Err(AppError::Validation("分块后无可用内容".into()));
    }
    let vecs = embed_texts(pool, &kb.embedding_channel_id, &kb.embedding_model, chunks.clone()).await?;
    if vecs.len() != chunks.len() {
        return Err(AppError::Proxy(
            "嵌入返回的向量数量与分块数量不一致".into(),
        ));
    }
    let paired: Vec<(String, Vec<f32>)> = chunks.into_iter().zip(vecs).collect();
    let chunk_count = paired.len();
    let document_id = insert_document(pool, kb_id, title, "text", "", paired).await?;
    Ok(IngestResult {
        document_id,
        chunk_count,
    })
}
