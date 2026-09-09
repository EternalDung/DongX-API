//! 索引状态与重建：管理面命令与 MCP 工具共用同一份实现。
//!
//! 管理面 `commands/rag.rs` 只保留命令壳（从 `State` 取 pool 后转调本模块），
//! MCP 工具本身持有 pool 也转调本模块 —— 避免 SQL 与重嵌逻辑复制两份而漂移。

use serde::Serialize;
use sqlx::Row;
use sqlx::SqlitePool;

use crate::rag::embed::embed_texts;
use crate::rag::store::get_kb;

/// 索引状态（对齐前端 `IndexStatus`）。
///
/// - `embedded_count`：已向量化（embedding 非空且非 `[]`）的分块数；
/// - `stale_count`：分块记录的嵌入模型与知识库当前 `embedding_model` 不一致的分块数
///   （改了嵌入模型后旧分块即 stale，需要重建索引）；
/// - `is_complete`：全部分块都已向量化；
/// - `is_stale`：存在 stale 分块。
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export_to = "rag.ts")]
pub struct IndexStatus {
    #[ts(type = "number")]
    pub doc_count: i64,
    #[ts(type = "number")]
    pub chunk_count: i64,
    #[ts(type = "number")]
    pub embedded_count: i64,
    #[ts(type = "number")]
    pub stale_count: i64,
    /// 全部分块的 token 总数（来自上游嵌入响应的 prompt_tokens 汇总）。
    #[ts(type = "number")]
    pub total_tokens: i64,
    /// 知识库当前绑定的嵌入模型（判定 stale 的基准）。
    pub embedding_model: String,
    pub is_complete: bool,
    pub is_stale: bool,
}

/// 计算索引状态（查询命令与重建索引共用，避免重复 SQL）。
pub async fn compute_index_status(pool: &SqlitePool, kb_id: &str) -> Result<IndexStatus, String> {
    let kb = get_kb(pool, kb_id).await.map_err(|e| e.to_string())?;
    let stats = sqlx::query(
        "SELECT \
            (SELECT COUNT(*) FROM kb_documents WHERE kb_id = ?) AS doc_count, \
            (SELECT COUNT(*) FROM kb_chunks WHERE kb_id = ?) AS chunk_count, \
            (SELECT COUNT(*) FROM kb_chunks \
                WHERE kb_id = ? AND embedding IS NOT NULL \
                  AND embedding <> '' AND embedding <> '[]') AS embedded_count, \
            (SELECT COUNT(*) FROM kb_chunks \
                WHERE kb_id = ? AND (embedding_model IS NULL OR embedding_model <> ?)) AS stale_count, \
            (SELECT COALESCE(SUM(token_count), 0) FROM kb_documents WHERE kb_id = ?) AS total_tokens",
    )
    .bind(kb_id)
    .bind(kb_id)
    .bind(kb_id)
    .bind(kb_id)
    .bind(&kb.embedding_model)
    .bind(kb_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    let doc_count: i64 = stats.try_get("doc_count").unwrap_or(0);
    let chunk_count: i64 = stats.try_get("chunk_count").unwrap_or(0);
    let embedded_count: i64 = stats.try_get("embedded_count").unwrap_or(0);
    let stale_count: i64 = stats.try_get("stale_count").unwrap_or(0);
    let total_tokens: i64 = stats.try_get("total_tokens").unwrap_or(0);

    Ok(IndexStatus {
        doc_count,
        chunk_count,
        embedded_count,
        stale_count,
        total_tokens,
        embedding_model: kb.embedding_model,
        is_complete: chunk_count > 0 && embedded_count == chunk_count,
        is_stale: stale_count > 0,
    })
}

/// 重建索引：按知识库「当前」嵌入模型，重新向量化全部分块并写回
/// （embedding + embedding_model），用于切换嵌入模型后的存量刷新。
///
/// 按 `embedding_batch_size`（缺省 16）分批调用嵌入接口，避免一次性把全文
/// 堆进内存。同步执行（调用方阻塞等待）：管理面前端以 Spinner 等待，
/// MCP 侧由调用方等待返回；本地单用户量下可接受。
pub async fn reindex_kb(pool: &SqlitePool, kb_id: &str) -> Result<IndexStatus, String> {
    let kb = get_kb(pool, kb_id).await.map_err(|e| e.to_string())?;

    // 读全部分块（无论是否已向量化，都按当前模型重嵌）
    let rows: Vec<(String, String)> =
        sqlx::query("SELECT id, content FROM kb_chunks WHERE kb_id = ? ORDER BY seq ASC")
            .bind(kb_id)
            .fetch_all(pool)
            .await
            .map_err(|e| e.to_string())?
            .into_iter()
            .map(|r: sqlx::sqlite::SqliteRow| {
                let id: String = r.try_get("id").unwrap_or_default();
                let content: String = r.try_get("content").unwrap_or_default();
                (id, content)
            })
            .collect();

    if rows.is_empty() {
        return compute_index_status(pool, kb_id).await;
    }

    let batch = kb.embedding_batch_size.filter(|&b| b > 0).unwrap_or(16) as usize;

    for chunk in rows.chunks(batch) {
        let contents: Vec<String> = chunk.iter().map(|(_, c)| c.clone()).collect();
        let (vecs, _tokens) = embed_texts(
            pool,
            &kb.embedding_channel_id,
            &kb.embedding_model,
            contents,
        )
        .await
        .map_err(|e| e.to_string())?;
        if vecs.len() != chunk.len() {
            return Err("重建索引时嵌入返回的向量数量与分块数量不一致".to_string());
        }
        for ((id, _), emb) in chunk.iter().zip(vecs) {
            let emb_json = serde_json::to_string(&emb).map_err(|e| e.to_string())?;
            sqlx::query("UPDATE kb_chunks SET embedding = ?, embedding_model = ? WHERE id = ?")
                .bind(emb_json)
                .bind(&kb.embedding_model)
                .bind(id)
                .execute(pool)
                .await
                .map_err(|e| e.to_string())?;
        }
    }

    compute_index_status(pool, kb_id).await
}
