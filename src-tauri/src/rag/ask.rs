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
        .0
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

/// 通用 chat 补全：构造 system + user 消息，经锁定渠道（`channel_id`）或 dispatcher
/// 自动分发（Failover/熔断/加权）发起单次非流式 chat，返回文本内容。
///
/// 供 `ask_deep_research` 在每轮「生成追问」「生成发现」「最终综合」时复用，
/// 与 `ask` 主流程的转发逻辑保持一致。
async fn chat_completion(
    pool: &SqlitePool,
    model: &str,
    channel_id: Option<&str>,
    system: &str,
    user: &str,
) -> Result<String, AppError> {
    let chat_body = serde_json::json!({
        "model": model,
        "messages": [
            { "role": "system", "content": system },
            { "role": "user", "content": user }
        ],
        "stream": false
    });

    if let Some(cid) = channel_id {
        let row: ChannelRow = sqlx::query_as::<_, ChannelRow>(
            "SELECT * FROM channels WHERE id = ?",
        )
        .bind(cid)
        .fetch_optional(pool)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("渠道不存在: {}", cid)))?;
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
        call_chat_once(&row, model, &chat_body).await
    } else {
        let ctx_disp = DispatchContext {
            model: model.to_string(),
            api_key_id: "rag:deep-research".to_string(),
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
                    if !retryable || !fo.should_retry() {
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
        Ok(out)
    }
}

/// 去重累积的来源（按 kb_id + doc_title + content），避免多轮检索返回重复分块。
fn dedupe_sources(mut s: Vec<Source>) -> Vec<Source> {
    let mut seen: Vec<(String, String, String)> = Vec::new();
    s.retain(|x| {
        let key = (x.kb_id.clone(), x.doc_title.clone(), x.content.clone());
        if seen.iter().any(|k| *k == key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    s
}

/// Deep Research：多轮迭代检索 + 综合（对齐 waliapi 的 `deep_research`）。
///
/// 第 0 轮用原始问题；后续每轮让 LLM 基于已有发现生成追问查询，
/// 重新嵌入 → 检索 → 累积来源 → 生成该轮发现；达到 `max_rounds` 后做最终综合。
/// 中途若某轮检索为空（非首轮）或 LLM 调用失败则提前结束。
pub async fn ask_deep_research(
    pool: &SqlitePool,
    kb_ids: &[String],
    question: &str,
    model: &str,
    channel_id: Option<&str>,
    mode: RetrievalMode,
    top_k: usize,
    keyword_weight: f32,
    max_rounds: usize,
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
    let max_rounds = max_rounds.max(1);

    let mut all_findings: Vec<String> = Vec::new();
    let mut all_sources: Vec<Source> = Vec::new();

    for round in 0..max_rounds {
        // 1. 本轮查询：首轮用原问题，后续让 LLM 基于已有发现生成追问。
        let round_query = if round == 0 {
            question.to_string()
        } else {
            let findings = all_findings
                .iter()
                .enumerate()
                .map(|(i, f)| format!("第{}轮: {}", i + 1, f))
                .collect::<Vec<_>>()
                .join("\n");
            let prompt = format!(
                "基于原始问题和已有发现，生成一个简短的追问查询（只返回查询本身，不要解释）。\n\n\
                 原始问题: {query}\n\n已有发现:\n{findings}\n\n\
                 请生成下一步需要搜索的关键词或问题（直接返回查询文本，不要加引号或其他格式）：",
                query = question,
                findings = findings,
            );
            match chat_completion(
                pool,
                model,
                channel_id,
                "你是一个研究助手，根据已有发现生成下一步搜索查询。只返回查询本身。",
                &prompt,
            )
            .await
            {
                Ok(q) if !q.trim().is_empty() => q.trim().to_string(),
                _ => question.to_string(),
            }
        };

        // 2. 嵌入 + 检索
        let q_vec: Vec<f32> = if need_embed {
            embed_texts(
                pool,
                &kb.embedding_channel_id,
                &kb.embedding_model,
                vec![round_query.clone()],
            )
            .await?
            .0
            .into_iter()
            .next()
            .ok_or_else(|| AppError::Internal("嵌入结果为空".into()))?
        } else {
            Vec::new()
        };
        let hits = retrieve(
            pool,
            kb_ids,
            &round_query,
            &q_vec,
            top_k,
            mode,
            keyword_weight,
        )
        .await?;
        if hits.is_empty() && round > 0 {
            break; // 后续轮次无新内容，提前结束
        }
        for h in &hits {
            all_sources.push(Source {
                kb_id: h.kb_id.clone(),
                doc_title: h.doc_title.clone(),
                content: h.content.clone(),
                score: h.score,
            });
        }

        // 3. 本轮发现
        let context = hits
            .iter()
            .map(|h| format!("[来源: {}]\n{}\n\n", h.doc_title, h.content))
            .collect::<String>();
        let round_prompt = if round == 0 {
            format!(
                "你是一个深度研究助手。请分析以下 RAG 内容，并给出初步发现。\n\n\
                 原始问题: {query}\n\n<knowledge_base>\n{context}</knowledge_base>\n\n\
                 请完成：\n1. 理解问题的核心需求\n2. 从 RAG 中提取相关信息\n3. 给出初步发现\n4. 如果信息不足，指出还需要哪些方面",
                query = question,
                context = context,
            )
        } else {
            let findings = all_findings
                .iter()
                .enumerate()
                .map(|(i, f)| format!("第{}轮发现: {}", i + 1, f))
                .collect::<Vec<_>>()
                .join("\n");
            format!(
                "继续深度研究。\n\n原始问题: {query}\n\n已有发现:\n{findings}\n\n\
                 新检索到的内容:\n<knowledge_base>\n{context}</knowledge_base>\n\n\
                 请完成：\n1. 分析新内容与已有发现的关系\n2. 补充或修正之前的发现\n3. 指出是否需要继续研究",
                query = question,
                findings = findings,
                context = context,
            )
        };
        let round_answer = match chat_completion(
            pool,
            model,
            channel_id,
            "你是深度研究助手。基于 RAG 内容进行多轮迭代研究，逐步深入分析。",
            &round_prompt,
        )
        .await
        {
            Ok(a) => a,
            Err(e) => {
                tracing::warn!("Deep research 第 {} 轮失败: {}", round + 1, e);
                break;
            }
        };
        all_findings.push(round_answer);

        // round >= 2 且已是最后一轮则结束（信息已充分）。
        if round >= 2 && round == max_rounds - 1 {
            break;
        }
    }

    // 最终综合
    let findings_summary = all_findings
        .iter()
        .enumerate()
        .map(|(i, f)| format!("### 第{}轮发现\n{}", i + 1, f))
        .collect::<Vec<_>>()
        .join("\n\n");
    let final_prompt = format!(
        "你是深度研究助手。请基于以下多轮研究发现，给出对原始问题「{query}」的最终综合回答。\n\n{findings}",
        query = question,
        findings = findings_summary,
    );
    let answer = chat_completion(
        pool,
        model,
        channel_id,
        "你是深度研究助手，综合多轮研究发现给出最终回答。",
        &final_prompt,
    )
    .await?;

    Ok(AskResult {
        answer,
        sources: dedupe_sources(all_sources),
    })
}
