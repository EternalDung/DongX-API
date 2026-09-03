//! 检索：支持三种模式 —— 向量（余弦）、关键词（BM25）、混合（归一化加权融合）。
//!
//! - 向量：对查询嵌入向量在指定知识库分块中求 Top-K 余弦相似度。
//! - 关键词：BM25 召回，零外部依赖；中文按字符 bigram、英文/数字按词切分。
//! - 混合：向量得分与 BM25 得分分别 min-max 归一化到 [0,1]，按
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
    kb_id: String,
    doc_id: String,
    doc_title: String,
    content: String,
    emb: Option<Vec<f32>>,
}

/// 在 `kb_ids` 范围内检索 `query_text`/`query_vec` 最相关的 Top-K 分块。
///
/// - `query_text`：原始查询文本，BM25 关键词召回使用。
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

    let mut chunks: Vec<ChunkData> = Vec::new();
    for row in rows {
        let emb_str: Option<String> = row.try_get("embedding").unwrap_or(None);
        let emb: Option<Vec<f32>> = emb_str
            .and_then(|s| serde_json::from_str::<Vec<f32>>(s.as_str()).ok())
            .filter(|v| !v.is_empty());
        chunks.push(ChunkData {
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

    // 关键词得分（BM25）
    let kw_scores: Vec<f32> = if need_kw {
        let qt = tokenize(query_text);
        bm25(&chunks, &qt)
    } else {
        vec![0.0; chunks.len()]
    };

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

/// 判断是否 CJK 基本汉字（覆盖常用中文，扩展区略）。
fn is_cjk(ch: char) -> bool {
    matches!(ch as u32, 0x4E00..=0x9FFF | 0x3400..=0x4DBF)
}

/// 分词：CJK 字符按字符 bigram（同时保留单字），连续 ASCII 字母数字作为一个小写词。
fn tokenize(text: &str) -> Vec<String> {
    let mut tokens: Vec<String> = Vec::new();
    let mut buf = String::new();
    let mut prev_cjk: Option<char> = None;
    let flush_buf = |buf: &mut String, tokens: &mut Vec<String>| {
        if !buf.is_empty() {
            tokens.push(buf.clone());
            buf.clear();
        }
    };
    for ch in text.chars() {
        if ch.is_ascii_alphanumeric() {
            buf.push(ch.to_ascii_lowercase());
            prev_cjk = None;
        } else if is_cjk(ch) {
            flush_buf(&mut buf, &mut tokens);
            if let Some(p) = prev_cjk {
                tokens.push(format!("{}{}", p, ch));
            }
            tokens.push(ch.to_string());
            prev_cjk = Some(ch);
        } else {
            flush_buf(&mut buf, &mut tokens);
            prev_cjk = None;
        }
    }
    flush_buf(&mut buf, &mut tokens);
    tokens.retain(|t| !t.is_empty());
    tokens
}

/// 对 `chunks` 求 `query_terms` 的 BM25 得分（每块一个 f32）。
/// 语料 = 本批 `chunks`；IDF 用标准平滑公式；k1=1.5, b=0.75。
fn bm25(chunks: &[ChunkData], query_terms: &[String]) -> Vec<f32> {
    if query_terms.is_empty() || chunks.is_empty() {
        return vec![0.0; chunks.len()];
    }
    let mut df: HashMap<String, usize> = HashMap::new();
    let mut doc_tf: Vec<HashMap<String, usize>> = Vec::with_capacity(chunks.len());
    let mut total_len = 0usize;
    for c in chunks {
        let toks = tokenize(&c.content);
        let mut tf: HashMap<String, usize> = HashMap::new();
        for t in &toks {
            *tf.entry(t.clone()).or_insert(0) += 1;
        }
        for t in tf.keys() {
            *df.entry(t.clone()).or_insert(0) += 1;
        }
        total_len += toks.len();
        doc_tf.push(tf);
    }
    let n = chunks.len() as f32;
    let avgdl = total_len as f32 / n.max(1.0);
    let k1 = 1.5f32;
    let b = 0.75f32;
    let mut scores = Vec::with_capacity(chunks.len());
    for tf in &doc_tf {
        let dl = tf.values().sum::<usize>() as f32;
        let mut s = 0.0f32;
        for qt in query_terms {
            if let Some(&n_t) = df.get(qt) {
                let idf = ((n - n_t as f32 + 0.5) / (n_t as f32 + 0.5) + 1.0).ln();
                let f = *tf.get(qt).unwrap_or(&0) as f32;
                s += idf * (f * (k1 + 1.0)) / (f + k1 * (1.0 - b + b * dl / avgdl.max(1.0)));
            }
        }
        scores.push(s);
    }
    scores
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
        // 三块：Rust 相关两块、Python 一块；embedding 置 NULL（BM25 不依赖）
        let contents = [
            "Rust 所有权机制详解与移动语义",
            "Python 装饰器使用指南与闭包",
            "Rust 生命周期标注与借用检查",
        ];
        for (i, content) in contents.iter().enumerate() {
            sqlx::query(
                "INSERT INTO kb_chunks (id,kb_id,doc_id,content,embedding,created_at) VALUES (?,?,?,?,NULL,'2026-01-01T00:00:00Z')",
            )
            .bind(format!("c{}", i))
            .bind("kb1")
            .bind("d1")
            .bind(*content)
            .execute(&pool)
            .await
            .unwrap();
        }
        pool
    }

    #[test]
    fn tokenize_splits_cjk_bigram_and_ascii() {
        let toks = tokenize("Rust所有权");
        assert!(toks.contains(&"rust".to_string()), "英文小写化: {:?}", toks);
        assert!(toks.contains(&"所有".to_string()), "CJK bigram: {:?}", toks);
        assert!(toks.contains(&"有权".to_string()), "CJK bigram: {:?}", toks);
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
    async fn vector_mode_returns_empty_without_embeddings() {
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
        // 查询词完全不在语料 → BM25 全 0 → 混合排序退化为向量（此处向量也全 0，顺序稳定即可）
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
