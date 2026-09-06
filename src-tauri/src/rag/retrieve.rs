//! 检索：支持三种模式 —— 向量（余弦）、关键词（FTS5 trigram）、混合（归一化加权融合）。
//!
//! - 向量：对查询嵌入向量在指定知识库分块中求 Top-K 余弦相似度。
//! - 关键词：FTS5 原生全文检索（trigram tokenizer）召回，得分即 FTS5 的 BM25 排名
//!   （`-rank`，越大越相关）。中文子串匹配友好，无需手工分词。
//! - 混合：向量余弦得分（0–1）与 FTS5 得分（min-max 归一化到 0–1）按
//!   `final = (1 - keyword_weight) * vec + keyword_weight * bm25` 融合。

use std::collections::HashMap;

use sqlx::Row;
use sqlx::SqlitePool;

use crate::error::AppError;

/// 检索模式。
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RetrievalMode {
    Vector,
    Keyword,
    Hybrid,
}

impl RetrievalMode {
    /// 从前端/命令传入的字符串解析；缺省或未知值回退 Vector。
    pub fn from_str_opt(s: Option<&str>) -> Self {
        match s {
            Some("keyword") => RetrievalMode::Keyword,
            Some("hybrid") => RetrievalMode::Hybrid,
            _ => RetrievalMode::Vector,
        }
    }
}

/// 一条命中的分块（不含序列化需求）。
#[derive(Debug)]
pub struct RetrievedChunk {
    pub kb_id: String,
    pub doc_id: String,
    pub doc_title: String,
    pub content: String,
    pub score: f32,
}

/// 检索中间态：原始分块数据 + 解析后的向量。
struct ChunkData {
    id: String,
    kb_id: String,
    doc_id: String,
    doc_title: String,
    content: String,
    emb: Option<Vec<f32>>,
}

/// 在 `kb_ids` 范围内检索 `query_text`/`query_vec` 最相关的 Top-K 分块。
///
/// - `query_text`：原始查询文本，FTS5 关键词召回使用。
/// - `query_vec`：查询嵌入向量，向量/混合模式使用；纯关键词模式下可传空。
/// - `mode`：检索模式（见 [`RetrievalMode`]）。
/// - `keyword_weight`：混合模式下关键词得分权重（0..1），向量权重为 1 - keyword_weight。
pub async fn retrieve(
    pool: &SqlitePool,
    kb_ids: &[String],
    query_text: &str,
    query_vec: &[f32],
    top_k: usize,
    mode: RetrievalMode,
    keyword_weight: f32,
) -> Result<Vec<RetrievedChunk>, AppError> {
    if kb_ids.is_empty() {
        return Ok(Vec::new());
    }
    let placeholders = vec!["?"; kb_ids.len()].join(",");
    let sql = format!(
        "SELECT c.id AS chunk_id, c.kb_id, c.doc_id, c.content, c.embedding, d.title AS doc_title \
         FROM kb_chunks c LEFT JOIN kb_documents d ON d.id = c.doc_id \
         WHERE c.kb_id IN ({})",
        placeholders
    );
    let mut q = sqlx::query(&sql);
    for id in kb_ids {
        q = q.bind(id);
    }
    let rows = q.fetch_all(pool).await?;

    let mut chunks: Vec<ChunkData> = Vec::new();
    for row in rows {
        let emb_str: Option<String> = row.try_get("embedding").unwrap_or(None);
        let emb: Option<Vec<f32>> = emb_str
            .and_then(|s| serde_json::from_str::<Vec<f32>>(s.as_str()).ok())
            .filter(|v| !v.is_empty());
        chunks.push(ChunkData {
            id: row.try_get("chunk_id").unwrap_or_default(),
            kb_id: row.try_get("kb_id").unwrap_or_default(),
            doc_id: row.try_get("doc_id").unwrap_or_default(),
            doc_title: row
                .try_get::<Option<String>, _>("doc_title")
                .unwrap_or(None)
                .unwrap_or_default(),
            content: row.try_get("content").unwrap_or_default(),
            emb,
        });
    }
    if chunks.is_empty() {
        return Ok(Vec::new());
    }

    let need_vec = matches!(mode, RetrievalMode::Vector | RetrievalMode::Hybrid);
    let need_kw = matches!(mode, RetrievalMode::Keyword | RetrievalMode::Hybrid);

    // 向量得分
    let vec_scores: Vec<f32> = if need_vec {
        chunks
            .iter()
            .map(|c| match &c.emb {
                Some(e) if !query_vec.is_empty() => cosine(query_vec, e),
                _ => 0.0,
            })
            .collect()
    } else {
        vec![0.0; chunks.len()]
    };

    // 关键词得分（FTS5 trigram）
    let fts_scores: HashMap<String, f32> = if need_kw {
        match normalize_fts_query(query_text) {
            Some(q) => fts_keyword_scores(pool, kb_ids, &q).await,
            None => HashMap::new(),
        }
    } else {
        HashMap::new()
    };
    let kw_scores: Vec<f32> = chunks
        .iter()
        .map(|c| fts_scores.get(&c.id).copied().unwrap_or(0.0))
        .collect();

    // 融合
    let kw = keyword_weight.clamp(0.0, 1.0);
    let vec_norm = normalize(&vec_scores);
    let kw_norm = normalize(&kw_scores);
    let mut combined: Vec<(f32, usize)> = (0..chunks.len())
        .map(|i| {
            let score = match mode {
                RetrievalMode::Vector => vec_scores[i],
                RetrievalMode::Keyword => kw_scores[i],
                RetrievalMode::Hybrid => (1.0 - kw) * vec_norm[i] + kw * kw_norm[i],
            };
            (score, i)
        })
        .collect();
    combined.sort_by(|a, b| b.0.partial_cmp(&a.0).unwrap_or(std::cmp::Ordering::Equal));
    let k = top_k.max(1);
    Ok(combined
        .into_iter()
        .take(k)
        .map(|(score, i)| RetrievedChunk {
            kb_id: chunks[i].kb_id.clone(),
            doc_id: chunks[i].doc_id.clone(),
            doc_title: chunks[i].doc_title.clone(),
            content: chunks[i].content.clone(),
            score,
        })
        .collect())
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

/// 把一组得分 min-max 归一化到 [0,1]；全相等时返回全 0（避免除零）。
fn normalize(scores: &[f32]) -> Vec<f32> {
    let max = scores.iter().cloned().fold(f32::MIN, f32::max);
    let min = scores.iter().cloned().fold(f32::MAX, f32::min);
    let range = max - min;
    if range <= 1e-9 {
        return vec![0.0; scores.len()];
    }
    scores.iter().map(|s| (s - min) / range).collect()
}

/// 把查询文本规整为 FTS5 可安全检索的形式：
/// - 仅保留 ASCII 字母数字、CJK 基本汉字与空白；
/// - ASCII 统一小写（trigram 大小写折叠兜底）；连续空白合并为单空格；
/// - 去除首尾空白。
///
/// 返回 `None` 表示规整后有效字符不足 3 个（trigram 最短需 3 字符，
/// 否则无法构成 trigram，直接回退空关键词得分，由混合模式的向量部分兜底）。
fn normalize_fts_query(q: &str) -> Option<String> {
    let mut out = String::new();
    let mut prev_space = false;
    for c in q.chars() {
        if c.is_ascii_alphanumeric() {
            out.push(c.to_ascii_lowercase());
            prev_space = false;
        } else if is_cjk(c) {
            out.push(c);
            prev_space = false;
        } else if c.is_whitespace() && !prev_space && !out.is_empty() {
            out.push(' ');
            prev_space = true;
        }
        // 其它标点 / 符号丢弃
    }
    let nospace_len = out.chars().filter(|c| !c.is_whitespace()).count();
    if nospace_len < 3 {
        None
    } else {
        Some(out.trim().to_string())
    }
}

/// 判断是否 CJK 基本汉字（覆盖常用中文，扩展区略）。
fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}

/// 用 FTS5 trigram 对 `kb_ids` 范围做关键词检索，返回 `chunk_id → 得分(-rank)`。
///
/// 查询失败时（异常输入等）返回空 map，让调用方回退到向量检索，不阻断整体问答。
async fn fts_keyword_scores(
    pool: &SqlitePool,
    kb_ids: &[String],
    query: &str,
) -> HashMap<String, f32> {
    let mut map = HashMap::new();
    let placeholders = vec!["?"; kb_ids.len()].join(",");
    let sql = format!(
        "SELECT chunk_id, -rank AS score FROM kb_chunks_fts \
         WHERE kb_id IN ({}) AND kb_chunks_fts MATCH ? ORDER BY rank",
        placeholders
    );
    let mut q = sqlx::query_as::<_, (String, f64)>(&sql);
    for id in kb_ids {
        q = q.bind(id);
    }
    q = q.bind(query);
    match q.fetch_all(pool).await {
        Ok(rows) => {
            for (cid, score) in rows {
                map.insert(cid, score as f32);
            }
        }
        Err(e) => {
            tracing::warn!("FTS5 关键词检索失败（回退为空）: {}", e);
        }
    }
    map
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .unwrap();
        sqlx::migrate!()
            .run(&pool)
            .await
            .expect("migrations 应用失败");
        sqlx::query(
            "INSERT INTO knowledge_bases (id,name,embedding_model,status,created_at,updated_at) VALUES (?,?,?,1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
        )
        .bind("kb1")
        .bind("KB")
        .bind("text-embedding-3-small")
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query(
            "INSERT INTO kb_documents (id,kb_id,title,status,created_at,updated_at) VALUES (?,?,?,1,'2026-01-01T00:00:00Z','2026-01-01T00:00:00Z')",
        )
        .bind("d1")
        .bind("kb1")
        .bind("Doc")
        .execute(&pool)
        .await
        .unwrap();
        // 三块：Rust 相关两块、Python 一块；embedding 置 NULL（向量模式另测）
        let contents = [
            "Rust 所有权机制详解与移动语义",
            "Python 装饰器使用指南与闭包",
            "Rust 生命周期标注与借用检查",
        ];
        for (i, content) in contents.iter().enumerate() {
            let cid = format!("c{}", i);
            sqlx::query(
                "INSERT INTO kb_chunks (id,kb_id,doc_id,content,embedding,created_at) VALUES (?,?,?,?,NULL,'2026-01-01T00:00:00Z')",
            )
            .bind(&cid)
            .bind("kb1")
            .bind("d1")
            .bind(*content)
            .execute(&pool)
            .await
            .unwrap();
            // 同步写入 FTS5 索引（与 store::insert_document 的维护逻辑一致）
            sqlx::query("INSERT INTO kb_chunks_fts (chunk_id, kb_id, content) VALUES (?, ?, ?)")
                .bind(&cid)
                .bind("kb1")
                .bind(*content)
                .execute(&pool)
                .await
                .unwrap();
        }
        pool
    }

    #[test]
    fn normalize_fts_query_drops_short_input() {
        // < 3 有效字符 → None（trigram 无法构成）
        assert!(normalize_fts_query("知识").is_none());
        assert!(normalize_fts_query("ab").is_none());
        // 正常保留并小写 ASCII
        assert_eq!(
            normalize_fts_query("Rust 所有权"),
            Some("rust 所有权".to_string())
        );
    }

    #[tokio::test]
    async fn keyword_mode_ranks_rust_chunk_first() {
        let pool = test_pool().await;
        let hits = retrieve(
            &pool,
            &["kb1".to_string()],
            "Rust 所有权",
            &[],
            3,
            RetrievalMode::Keyword,
            0.3,
        )
        .await
        .unwrap();
        assert_eq!(hits.len(), 3);
        assert!(
            hits[0].content.contains("Rust 所有权"),
            "关键词模式应把含查询词的块排第一，实际: {:?}",
            hits.iter().map(|h| &h.content).collect::<Vec<_>>()
        );
    }

    #[tokio::test]
    async fn keyword_mode_skips_unrelated_python_chunk() {
        let pool = test_pool().await;
        let hits = retrieve(
            &pool,
            &["kb1".to_string()],
            "Rust 生命周期",
            &[],
            1,
            RetrievalMode::Keyword,
            0.3,
        )
        .await
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(
            hits[0].content.contains("Rust"),
            "应命中 Rust 相关块而非 Python，实际: {}",
            hits[0].content
        );
    }

    #[tokio::test]
    async fn trigram_matches_cjk_substring() {
        let pool = test_pool().await;
        // "知识库" 不在任何块里，但 "本知识库内容" 子串应被 trigram 命中（若插入这样的块）
        sqlx::query("INSERT INTO kb_chunks (id,kb_id,doc_id,content,embedding,created_at) VALUES (?,?,?,?,NULL,'2026-01-01T00:00:00Z')")
            .bind("cX").bind("kb1").bind("d1").bind("本知识库内容索引示例").execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO kb_chunks_fts (chunk_id,kb_id,content) VALUES (?,?,?)")
            .bind("cX")
            .bind("kb1")
            .bind("本知识库内容索引示例")
            .execute(&pool)
            .await
            .unwrap();
        let hits = retrieve(
            &pool,
            &["kb1".to_string()],
            "知识库",
            &[],
            1,
            RetrievalMode::Keyword,
            0.3,
        )
        .await
        .unwrap();
        assert_eq!(hits.len(), 1);
        assert!(hits[0].content.contains("本知识库内容"));
    }

    #[tokio::test]
    async fn short_query_under_3_chars_falls_back_empty() {
        // ≤2 字查询关键词检索回退空（不报错），排序稳定即可
        let pool = test_pool().await;
        let hits = retrieve(
            &pool,
            &["kb1".to_string()],
            "知识",
            &[],
            3,
            RetrievalMode::Keyword,
            0.3,
        )
        .await
        .unwrap();
        assert_eq!(hits.len(), 3);
    }

    #[tokio::test]
    async fn vector_mode_returns_all_without_embeddings() {
        // 分块 embedding 为 NULL，向量模式拿不到任何向量得分 → 仍返回块但 score=0
        let pool = test_pool().await;
        let hits = retrieve(
            &pool,
            &["kb1".to_string()],
            "anything",
            &[0.1, 0.2, 0.3],
            3,
            RetrievalMode::Vector,
            0.3,
        )
        .await
        .unwrap();
        assert_eq!(hits.len(), 3);
        assert!(hits.iter().all(|h| h.score == 0.0));
    }

    #[tokio::test]
    async fn hybrid_falls_back_when_no_keyword_overlap() {
        // 查询词完全不在语料 → FTS5 全 0 → 混合排序退化为向量（此处向量也全 0，顺序稳定即可）
        let pool = test_pool().await;
        let hits = retrieve(
            &pool,
            &["kb1".to_string()],
            "量子计算区块链元宇宙",
            &[],
            3,
            RetrievalMode::Hybrid,
            0.5,
        )
        .await
        .unwrap();
        assert_eq!(hits.len(), 3);
    }
}
