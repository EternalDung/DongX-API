//! 问答：向量化问题 → 检索相关分块 → 构造上下文 → 复用网关分发机制发起
//! chat 完成。对应 `/v1/rag/ask` 与 Tauri 命令 `ask_kb`。

use sqlx::SqlitePool;

use serde::Serialize;

use crate::adapter::{get_adaptor, ChannelConfig, ProxyRequest};
use crate::core::dispatcher::DispatchContext;
use crate::core::failover::{Failover, Step};
use crate::error::AppError;
use crate::rag::embed::embed_texts;
use crate::rag::retrieve::{retrieve, RetrievedChunk};
use crate::rag::store::get_kb;

/// 单个引用来源（返回给前端）。
#[derive(Debug, Serialize)]
pub struct Source {
    pub kb_id: String,
    pub doc_title: String,
    pub content: String,
    pub score: f32,
}

/// 问答结果。
#[derive(Debug, Serialize)]
pub struct AskResult {
    pub answer: String,
    pub sources: Vec<Source>,
}

/// 在指定知识库范围内回答 `question`。
///
/// - `model`：用于生成回答的 chat 模型（经网关分发，与嵌入模型无关）
/// - 取首个 `kb_id` 的嵌入配置向量化问题；检索跨所有 `kb_ids`
pub async fn ask(
    pool: &SqlitePool,
    kb_ids: &[String],
    question: &str,
    model: &str,
) -> Result<AskResult, AppError> {
    if kb_ids.is_empty() {
        return Err(AppError::Validation("未指定知识库".into()));
    }
    if question.trim().is_empty() {
        return Err(AppError::Validation("问题不能为空".into()));
    }
    if model.trim().is_empty() {
        return Err(AppError::Validation("回答模型不能为空".into()));
    }

    // 用首个知识库的嵌入配置向量化问题。
    let kb = get_kb(pool, &kb_ids[0]).await?;
    let q_vec = embed_texts(
        pool,
        &kb.embedding_channel_id,
        &kb.embedding_model,
        vec![question.to_string()],
    )
    .await?
    .into_iter()
    .next()
    .ok_or_else(|| AppError::Internal("嵌入结果为空".into()))?;

    let top_k = 5;
    let hits: Vec<RetrievedChunk> = retrieve(pool, kb_ids, &q_vec, top_k).await?;

    // 构造知识库上下文（<knowledge_base> 块 + Token 降级：v1 直接拼接 Top-K）。
    let mut ctx = String::new();
    for h in &hits {
        ctx.push_str(&format!("[来源: {}]\n{}\n\n", h.doc_title, h.content));
    }
    let system_prompt = format!(
        "你是本地知识库问答助手。请仅基于下面 <knowledge_base> 中的内容回答用户问题；\
         若内容中没有相关信息，请明确说明「知识库中未找到相关信息」，不要编造。\n\n\
         <knowledge_base>\n{}\n</knowledge_base>",
        if ctx.is_empty() { "（无相关片段）" } else { &ctx }
    );

    let chat_body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system_prompt },
            { "role": "user", "content": question }
        ],
        "stream": false
    });

    // 复用网关分发 / 故障转移发起 chat 完成（与 embeddings 同构，不走 HTTP 鉴权）。
    let ctx_disp = DispatchContext {
        model: model.to_string(),
        api_key_id: format!("rag:{}", kb_ids.join(",")),
        is_stream: false,
        request_body: chat_body.clone(),
    };
    let mut fo = Failover::new(pool.clone(), ctx_disp, 1);
    let mut answer = String::new();
    loop {
        let step = fo.next().await?;
        let selected = match step {
            Step::Try(c) => c,
            Step::NoChannel(msg) => return Err(AppError::NotFound(msg)),
            Step::Exhausted => break,
        };
        let config = ChannelConfig {
            base_url: selected.base_url.clone(),
            api_key: selected.upstream_api_key.clone(),
            models: selected.models.clone(),
            model_mapping: selected.model_mapping.clone(),
            extra: selected.extra.clone(),
            timeout_secs: selected.timeout_secs,
            stream: false,
        };
        let proxy_req = ProxyRequest {
            model: model.to_string(),
            body: chat_body.clone(),
            stream: false,
        };
        let adaptor = get_adaptor(&selected.channel_type);
        match adaptor.forward(&proxy_req, &config).await {
            Ok((status, body, _usage)) => {
                if (200..300).contains(&status) {
                    answer = body
                        .get("choices")
                        .and_then(|c| c.get(0))
                        .and_then(|c| c.get("message"))
                        .and_then(|m| m.get("content"))
                        .and_then(|c| c.as_str())
                        .unwrap_or("")
                        .to_string();
                    break;
                }
                let msg = body
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("上游返回错误")
                    .to_string();
                let retryable = status >= 500 || status == 429 || status == 408 || status == 409;
                if !retryable {
                    return Err(AppError::Proxy(format!("上游返回 {}: {}", status, msg)));
                }
                if !fo.should_retry() {
                    return Err(AppError::Proxy(format!("上游返回 {}: {}", status, msg)));
                }
            }
            Err(e) => {
                if !fo.should_retry() {
                    return Err(AppError::Proxy(e.to_string()));
                }
            }
        }
    }

    if answer.is_empty() {
        return Err(AppError::Proxy("所有候选渠道均未能生成回答".into()));
    }

    let sources = hits
        .into_iter()
        .map(|h| Source {
            kb_id: h.kb_id,
            doc_title: h.doc_title,
            content: h.content,
            score: h.score,
        })
        .collect();

    Ok(AskResult { answer, sources })
}
