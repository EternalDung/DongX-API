//! 问答：向量化问题 → 检索相关分块 → 构造上下文 → 复用网关分发机制发起
//! chat 完成。对应 `/v1/rag/ask` 与 Tauri 命令 `ask_kb`。

use rand::Rng;
use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;

use crate::adapter::{get_adaptor, ChannelConfig, ProxyRequest};
use crate::core::dispatcher::DispatchContext;
use crate::core::failover::{Failover, Step};
use crate::crypto;
use crate::error::AppError;
use crate::models::ChannelRow;
use crate::rag::embed::embed_texts;
use crate::rag::retrieve::{retrieve, RetrievalMode, RetrievedChunk};
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

/// 按（id, weight）列表做加权随机挑选。weight clamp 到 ≥1。
fn weighted_pick(pairs: &[(String, i32)]) -> Option<String> {
    if pairs.is_empty() {
        return None;
    }
    let total: i32 = pairs.iter().map(|(_, w)| (*w).max(1)).sum();
    let mut rng = rand::thread_rng();
    let pick_idx = if total <= 0 {
        0
    } else {
        let mut r = rng.gen_range(0..total);
        let mut i = 0;
        for (_id, w) in pairs {
            let w = (*w).max(1);
            r -= w;
            if r < 0 {
                break;
            }
            i += 1;
        }
        i
    };
    pairs.get(pick_idx).map(|(id, _)| id.clone())
}

/// 解密渠道密钥并加权随机挑选一条上游 key。
fn decrypt_pick_upstream_key(cred_encrypted: &str) -> Result<String, AppError> {
    let plaintext = crypto::decrypt(cred_encrypted).map_err(AppError::Crypto)?;

    if let Ok(keys) = serde_json::from_str::<Vec<Value>>(&plaintext) {
        if !keys.is_empty() {
            let pairs: Vec<(String, i32)> = keys
                .iter()
                .filter_map(|k| {
                    let key = k.get("key")?.as_str()?.to_string();
                    if key.is_empty() {
                        return None;
                    }
                    let weight = k
                        .get("weight")
                        .and_then(|v| v.as_i64())
                        .unwrap_or(1) as i32;
                    Some((key, weight))
                })
                .collect();
            if !pairs.is_empty() {
                return weighted_pick(&pairs)
                    .ok_or_else(|| AppError::Crypto("无可用上游密钥".into()));
            }
        }
    }

    if plaintext.is_empty() {
        return Err(AppError::Crypto("上游密钥为空".into()));
    }
    Ok(plaintext)
}

/// 渠道是否服务于给定模型（列在 `models` 或被 `model_mapping` 映射到）。
fn channel_serves_model(row: &ChannelRow, model: &str) -> bool {
    if let Ok(models) = serde_json::from_str::<Vec<String>>(&row.models) {
        if models.iter().any(|m| m == model) {
            return true;
        }
    }
    if let Ok(mapping) = serde_json::from_str::<Value>(&row.model_mapping) {
        if mapping.get(model).is_some() {
            return true;
        }
    }
    false
}

/// 单次直接对指定渠道发起 chat 调用：解密 key、选择上游 key、转发并解析 answer。
async fn call_chat_once(
    row: &ChannelRow,
    model: &str,
    chat_body: &Value,
) -> Result<String, AppError> {
    let upstream_api_key = decrypt_pick_upstream_key(&row.cred_encrypted)?;
    let models: Vec<String> = serde_json::from_str(&row.models).unwrap_or_default();
    let model_mapping: Value =
        serde_json::from_str(&row.model_mapping).unwrap_or_else(|_| serde_json::json!({}));
    let config: Value =
        serde_json::from_str(&row.config).unwrap_or_else(|_| serde_json::json!({}));
    let timeout_secs = config
        .get("timeout_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(60);

    let channel_config = ChannelConfig {
        base_url: row.base_url.clone(),
        api_key: upstream_api_key,
        models,
        model_mapping,
        extra: config,
        timeout_secs,
        stream: false,
    };
    let proxy_req = ProxyRequest {
        model: model.to_string(),
        body: chat_body.clone(),
        stream: false,
    };
    let adaptor = get_adaptor(&row.channel_type);
    let (status, body, _usage) = adaptor.forward(&proxy_req, &channel_config).await?;
    if !(200..300).contains(&status) {
        let msg = body
            .get("error")
            .and_then(|e| e.get("message"))
            .and_then(|m| m.as_str())
            .unwrap_or("上游返回错误")
            .to_string();
        return Err(AppError::Proxy(format!("上游返回 {}: {}", status, msg)));
    }
    let answer = body
        .get("choices")
        .and_then(|c| c.get(0))
        .and_then(|c| c.get("message"))
        .and_then(|m| m.get("content"))
        .and_then(|c| c.as_str())
        .unwrap_or("")
        .to_string();
    Ok(answer)
}

/// 在指定知识库范围内回答 `question`。
///
/// - `model`：用于生成回答的 chat 模型（经网关分发，与嵌入模型无关）
/// - `channel_id`：可选，锁定单一渠道直接发（不走 Failover/熔断/加权）。
///   None 时保留原有 dispatcher 分发逻辑。
/// - `mode` / `top_k` / `keyword_weight`：检索模式、召回数、混合权重，透传给 `retrieve`。
/// - 取首个 `kb_id` 的嵌入配置向量化问题；纯关键词模式下跳过嵌入以省配额。
/// - 取首个 `kb_id` 的嵌入配置向量化问题；检索跨所有 `kb_ids`
pub async fn ask(
    pool: &SqlitePool,
    kb_ids: &[String],
    question: &str,
    model: &str,
    channel_id: Option<&str>,
    mode: RetrievalMode,
    top_k: usize,
    keyword_weight: f32,
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

    let kb = get_kb(pool, &kb_ids[0]).await?;
    // 纯关键词模式无需向量：跳过嵌入，省一次上游调用 + 配额。
    let need_embed = !matches!(mode, RetrievalMode::Keyword);
    let q_vec: Vec<f32> = if need_embed {
        embed_texts(
            pool,
            &kb.embedding_channel_id,
            &kb.embedding_model,
            vec![question.to_string()],
        )
        .await?
        .into_iter()
        .next()
        .ok_or_else(|| AppError::Internal("嵌入结果为空".into()))?
    } else {
        Vec::new()
    };

    let hits: Vec<RetrievedChunk> = retrieve(
        pool,
        kb_ids,
        question,
        &q_vec,
        top_k,
        mode,
        keyword_weight,
    )
    .await?;

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

    let answer = if let Some(cid) = channel_id {
        let row: ChannelRow = match sqlx::query_as::<_, ChannelRow>(
            "SELECT * FROM channels WHERE id = ?",
        )
        .bind(cid)
        .fetch_optional(pool)
        .await?
        {
            Some(r) => r,
            None => return Err(AppError::NotFound(format!("渠道不存在: {}", cid))),
        };
        if row.status != 1 {
            return Err(AppError::Validation(format!(
                "渠道「{}」已禁用，请先启用后再作为问答渠道",
                row.name
            )));
        }
        if !channel_serves_model(&row, model) {
            return Err(AppError::Validation(format!(
                "渠道「{}」不支持模型「{}」",
                row.name, model
            )));
        }
        call_chat_once(&row, model, &chat_body).await?
    } else {
        let ctx_disp = DispatchContext {
            model: model.to_string(),
            api_key_id: format!("rag:{}", kb_ids.join(",")),
            is_stream: false,
            request_body: chat_body.clone(),
        };
        let mut fo = Failover::new(pool.clone(), ctx_disp, 1);
        let mut out = String::new();
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
                        out = body
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
        if out.is_empty() {
            return Err(AppError::Proxy("所有候选渠道均未能生成回答".into()));
        }
        out
    };

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
