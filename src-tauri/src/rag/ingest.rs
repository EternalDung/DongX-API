//! 摄入：分块 → 向量化 → 持久化。供 Tauri 命令 `ingest_kb_text` 调用。

use sqlx::SqlitePool;

use serde::Serialize;

use crate::error::AppError;
use crate::rag::chunk::{split, SplitConfig};
use crate::rag::embed::embed_texts;
use crate::rag::parser::detect_kind_by_content;
use crate::rag::store::{content_hash, find_document_by_hash, get_kb, insert_document, ChunkInput};

/// 摄入结果。
#[derive(Debug, Serialize)]
pub struct IngestResult {
    pub document_id: String,
    pub chunk_count: usize,
    /// 该文本与已有文档内容重复，未重复摄入（命中去重）。
    pub duplicate: bool,
}

/// 把一段文本摄入指定知识库：分块后用该知识库绑定的嵌入渠道向量化并落库。
///
/// `file_size` 为原始文件字节数（前端上传时一并传入），用于文档列表展示。
/// 若同一知识库内已存在 `content_hash` 相同且状态为「就绪」的文档，则视为
/// 重复上传，直接返回已有文档信息（不再向量化，省去重复开销）。
pub async fn ingest_text(
    pool: &SqlitePool,
    kb_id: &str,
    title: &str,
    text: &str,
    file_size: i64,
) -> Result<IngestResult, AppError> {
    if text.trim().is_empty() {
        return Err(AppError::Validation("待摄入文本为空".into()));
    }
    let kb = get_kb(pool, kb_id).await?;

    // 去重：内容哈希相同且已就绪的文档视为同一份，直接复用。
    let hash = content_hash(text);
    if let Some((document_id, chunk_count)) =
        find_document_by_hash(pool, kb_id, &hash).await?
    {
        return Ok(IngestResult {
            document_id,
            chunk_count: chunk_count.max(0) as usize,
            duplicate: true,
        });
    }

    let kind = detect_kind_by_content(text);
    let config = SplitConfig::from_kb(kb.chunk_size, kb.chunk_overlap);
    let chunks = split(text, kind, None, &config);
    if chunks.is_empty() {
        return Err(AppError::Validation("分块后无可用内容".into()));
    }
    let contents: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let (vecs, token_count) = embed_texts(pool, &kb.embedding_channel_id, &kb.embedding_model, contents).await?;
    if vecs.len() != chunks.len() {
        return Err(AppError::Proxy(
            "嵌入返回的向量数量与分块数量不一致".into(),
        ));
    }
    let inputs: Vec<ChunkInput> = chunks
        .into_iter()
        .zip(vecs)
        .map(|(c, e)| ChunkInput {
            content: c.content,
            embedding: e,
            meta: c.meta,
        })
        .collect();
    let chunk_count = inputs.len();
    let document_id = insert_document(
        pool,
        kb_id,
        title,
        "text",
        "",
        &kb.embedding_model,
        file_size,
        token_count,
        &hash,
        inputs,
    )
    .await?;
    Ok(IngestResult {
        document_id,
        chunk_count,
        duplicate: false,
    })
}
