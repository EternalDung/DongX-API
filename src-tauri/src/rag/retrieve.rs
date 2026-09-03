//! 检索：暴力余弦相似度（v1）。在指定知识库的分块中，对查询向量求 Top-K。

use sqlx::Row;
use sqlx::SqlitePool;

use crate::error::AppError;

/// 一条命中的分块（不含序列化需求）。
#[derive(Debug)]
pub struct RetrievedChunk {
    pub kb_id: String,
    pub doc_id: String,
    pub doc_title: String,
    pub content: String,
    pub score: f32,
}

/// 在 `kb_ids` 范围内检索与 `query` 最相似的 Top-K 分块（按余弦相似度降序）。
/// 向量缺失（NULL / 解析失败）的分块跳过。
pub async fn retrieve(
    pool: &SqlitePool,
    kb_ids: &[String],
    query: &[f32],
    top_k: usize,
) -> Result<Vec<RetrievedChunk>, AppError> {
    if kb_ids.is_empty() || query.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; kb_ids.len()].join(",");
    let sql = format!(
        "SELECT c.kb_id, c.doc_id, c.content, c.embedding, d.title AS doc_title \
         FROM kb_chunks c LEFT JOIN kb_documents d ON d.id = c.doc_id \
         WHERE c.kb_id IN ({})",
        placeholders
    );
    let mut q = sqlx::query(&sql);
    for id in kb_ids {
        q = q.bind(id);
    }
    let rows = q.fetch_all(pool).await?;

    let mut scored: Vec<(f32, RetrievedChunk)> = Vec::new();
    for row in rows {
        let emb_str: Option<String> = row
            .try_get("embedding")
            .unwrap_or(None);
        let emb: Vec<f32> = match emb_str.and_then(|s| serde_json::from_str::<Vec<f32>>(s.as_str()).ok()) {
            Some(v) if !v.is_empty() => v,
            _ => continue,
        };
        let score = cosine(query, &emb);
        let chunk = RetrievedChunk {
            kb_id: row.try_get("kb_id").unwrap_or_default(),
            doc_id: row.try_get("doc_id").unwrap_or_default(),
            doc_title: row
                .try_get::<Option<String>, _>("doc_title")
                .unwrap_or(None)
                .unwrap_or_default(),
            content: row.try_get("content").unwrap_or_default(),
            score,
        };
        scored.push((score, chunk));
    }

    scored.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let k = top_k.max(1);
    Ok(scored.into_iter().take(k).map(|(_, c)| c).collect())
}

/// 余弦相似度；长度不一致或任一为零向量返回 0。
fn cosine(a: &[f32], b: &[f32]) -> f32 {
    if a.len() != b.len() || a.is_empty() {
        return 0.0;
    }
    let mut dot = 0.0f32;
    let mut na = 0.0f32;
    let mut nb = 0.0f32;
    for i in 0..a.len() {
        dot += a[i] * b[i];
        na += a[i] * a[i];
        nb += b[i] * b[i];
    }
    let denom = (na * nb).sqrt();
    if denom == 0.0 {
        0.0
    } else {
        dot / denom
    }
}
