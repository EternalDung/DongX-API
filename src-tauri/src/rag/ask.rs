//! 问答：向量化问题 → 检索相关分块 → 构造上下文 → 复用网关分发机制发起
//! chat 完成。对应 `/v1/rag/ask` 与 Tauri 命令 `ask_kb`。

use serde::Serialize;
use serde_json::Value;
use sqlx::SqlitePool;
use uuid::Uuid;

use crate::adapter::{get_adaptor, ChannelConfig, ProxyRequest};
use crate::core::dispatcher::DispatchContext;
use crate::core::failover::{Failover, Step};
use crate::core::upstream_key::pick_upstream_key;
use crate::db::repository::request_logs;
use crate::error::AppError;
use crate::models::ChannelRow;
use crate::rag::embed::embed_texts;
use crate::rag::retrieve::{retrieve, RetrievalMode, RetrievedChunk};
use crate::rag::store::get_kb;

/// 单个引用来源（返回给前端）。
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export_to = "rag.ts")]
#[ts(rename = "RagSource")]
pub struct Source {
    pub kb_id: String,
    pub doc_title: String,
    pub content: String,
    pub score: f32,
}

/// 问答结果。
#[derive(Debug, Serialize, ts_rs::TS)]
#[ts(export_to = "rag.ts")]
pub struct AskResult {
    pub answer: String,
    pub sources: Vec<Source>,
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

/// RAG 内部 LLM 调用的落库助手：与网关 `spawn_log` 写入同一张 `request_logs` 表，
/// 但 RAG 不经网关鉴权、无稳定网关 key，故 `api_key_id` 恒为 NULL，
/// `api_key_name` 统一标注为 `RAG: {知识库名}` 以区分来源。
///
/// 不挂安全审计（无 SecurityOutcome 依赖），仅记录用量与结果；用 `tokio::spawn`
/// 异步落库，不阻塞问答/深研主链路。问答与深研的每条 LLM 子调用都各记一行。
fn log_rag_attempt(
    pool: SqlitePool,
    kb_name: &str,
    mode: &str,
    model: &str,
    channel_name: &str,
    status: i32,
    prompt_tokens: i64,
    completion_tokens: i64,
    total_tokens: i64,
    duration_ms: i64,
    error_message: Option<String>,
    request_body: Option<String>,
    response_body: Option<String>,
    trace_id: &str,
    provider_request_id: Option<String>,
) {
    let api_key_name = format!("RAG: {}", kb_name);
    let channel = channel_name.to_string();
    let model = model.to_string();
    let mode = mode.to_string();
    let trace_id = trace_id.to_string();
    tokio::spawn(async move {
        if let Err(e) = request_logs::insert(
            &pool,
            Some(api_key_name.as_str()),
            None, // api_key_id：RAG 不经网关鉴权，无稳定 key，恒为 NULL
            Some(channel.as_str()),
            &model,
            None,
            &mode,
            status,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            duration_ms,
            error_message.as_deref(),
            false, // is_stream
            false, // is_retry
            request_body.as_deref(),
            response_body.as_deref(),
            "none",
            0,
            None,
            "allow",
            false,
            None,
            Some(trace_id.as_str()),
            provider_request_id.as_deref(),
            0,
        )
        .await
        {
            tracing::warn!("RAG 请求日志写入失败: {}", e);
        }
    });
}

/// 单次直接对指定渠道发起 chat 调用：解密 key、选择上游 key、转发并解析 answer。
/// 每次调用都会经 `log_rag_attempt` 落库一条请求日志（标识为 `RAG: {知识库名}`）。
async fn call_chat_once(
    pool: &SqlitePool,
    kb_name: &str,
    row: &ChannelRow,
    model: &str,
    chat_body: &Value,
    trace_id: &str,
) -> Result<String, AppError> {
    let upstream_api_key = pick_upstream_key(&row.cred_encrypted)?;
    let models: Vec<String> = serde_json::from_str(&row.models).unwrap_or_default();
    let model_mapping: Value =
        serde_json::from_str(&row.model_mapping).unwrap_or_else(|_| serde_json::json!({}));
    let config: Value = serde_json::from_str(&row.config).unwrap_or_else(|_| serde_json::json!({}));
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
    let af_start = std::time::Instant::now();
    let fwd = adaptor.forward(&proxy_req, &channel_config).await;
    let dur = af_start.elapsed().as_millis() as i64;
    match fwd {
        Ok((status, body, _usage, provider_request_id)) => {
            if !(200..300).contains(&status) {
                let msg = body
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("上游返回错误")
                    .to_string();
                log_rag_attempt(
                    pool.clone(),
                    kb_name,
                    "rag",
                    model,
                    &row.name,
                    status as i32,
                    0,
                    0,
                    0,
                    dur,
                    Some(msg.clone()),
                    Some(chat_body.to_string()),
                    Some(body.to_string()),
                    trace_id,
                    provider_request_id,
                );
                return Err(AppError::Proxy(format!("上游返回 {}: {}", status, msg)));
            }
            let pt = body
                .get("usage")
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let ct = body
                .get("usage")
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0);
            let tt = body
                .get("usage")
                .and_then(|u| u.get("total_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(pt + ct);
            log_rag_attempt(
                pool.clone(),
                kb_name,
                "rag",
                model,
                &row.name,
                status as i32,
                pt as i64,
                ct as i64,
                tt as i64,
                dur,
                None,
                Some(chat_body.to_string()),
                Some(body.to_string()),
                trace_id,
                provider_request_id,
            );
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
        Err(e) => {
            log_rag_attempt(
                pool.clone(),
                kb_name,
                "rag",
                model,
                &row.name,
                502,
                0,
                0,
                0,
                dur,
                Some(e.to_string()),
                Some(chat_body.to_string()),
                None,
                trace_id,
                None,
            );
            Err(e.into())
        }
    }
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

    let trace_id = Uuid::new_v4().to_string();
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

    let hits: Vec<RetrievedChunk> =
        retrieve(pool, kb_ids, question, &q_vec, top_k, mode, keyword_weight).await?;

    let mut ctx = String::new();
    for h in &hits {
        ctx.push_str(&format!("[来源: {}]\n{}\n\n", h.doc_title, h.content));
    }
    let system_prompt = format!(
        "你是本地知识库问答助手。请仅基于下面 <knowledge_base> 中的内容回答用户问题；\
         若内容中没有相关信息，请明确说明「知识库中未找到相关信息」，不要编造。\n\n\
         <knowledge_base>\n{}\n</knowledge_base>",
        if ctx.is_empty() {
            "（无相关片段）"
        } else {
            &ctx
        }
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
        let row: ChannelRow =
            match sqlx::query_as::<_, ChannelRow>("SELECT * FROM channels WHERE id = ?")
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
        call_chat_once(pool, &kb.name, &row, model, &chat_body, &trace_id).await?
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
            let af_start = std::time::Instant::now();
            let adaptor = get_adaptor(&selected.channel_type);
            match adaptor.forward(&proxy_req, &config).await {
                Ok((status, body, _usage, provider_request_id)) => {
                    let dur = af_start.elapsed().as_millis() as i64;
                    if (200..300).contains(&status) {
                        let pt = body
                            .get("usage")
                            .and_then(|u| u.get("prompt_tokens"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let ct = body
                            .get("usage")
                            .and_then(|u| u.get("completion_tokens"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let tt = body
                            .get("usage")
                            .and_then(|u| u.get("total_tokens"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(pt + ct);
                        log_rag_attempt(
                            pool.clone(),
                            &kb.name,
                            "rag",
                            model,
                            &selected.name,
                            status as i32,
                            pt as i64,
                            ct as i64,
                            tt as i64,
                            dur,
                            None,
                            Some(chat_body.to_string()),
                            Some(body.to_string()),
                            &trace_id,
                            provider_request_id,
                        );
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
                    log_rag_attempt(
                        pool.clone(),
                        &kb.name,
                        "rag",
                        model,
                        &selected.name,
                        status as i32,
                        0,
                        0,
                        0,
                        dur,
                        Some(msg.clone()),
                        Some(chat_body.to_string()),
                        Some(body.to_string()),
                        &trace_id,
                        provider_request_id,
                    );
                    let retryable =
                        status >= 500 || status == 429 || status == 408 || status == 409;
                    if !retryable || !fo.should_retry() {
                        return Err(AppError::Proxy(format!("上游返回 {}: {}", status, msg)));
                    }
                }
                Err(e) => {
                    let dur = af_start.elapsed().as_millis() as i64;
                    log_rag_attempt(
                        pool.clone(),
                        &kb.name,
                        "rag",
                        model,
                        &selected.name,
                        502,
                        0,
                        0,
                        0,
                        dur,
                        Some(e.to_string()),
                        Some(chat_body.to_string()),
                        None,
                        &trace_id,
                        None,
                    );
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
    kb_name: &str,
    model: &str,
    channel_id: Option<&str>,
    system: &str,
    user: &str,
    trace_id: &str,
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
        let row: ChannelRow =
            sqlx::query_as::<_, ChannelRow>("SELECT * FROM channels WHERE id = ?")
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
        call_chat_once(pool, kb_name, &row, model, &chat_body, trace_id).await
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
            let af_start = std::time::Instant::now();
            let adaptor = get_adaptor(&selected.channel_type);
            match adaptor.forward(&proxy_req, &config).await {
                Ok((status, body, _usage, provider_request_id)) => {
                    let dur = af_start.elapsed().as_millis() as i64;
                    if (200..300).contains(&status) {
                        let pt = body
                            .get("usage")
                            .and_then(|u| u.get("prompt_tokens"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let ct = body
                            .get("usage")
                            .and_then(|u| u.get("completion_tokens"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(0);
                        let tt = body
                            .get("usage")
                            .and_then(|u| u.get("total_tokens"))
                            .and_then(|v| v.as_u64())
                            .unwrap_or(pt + ct);
                        log_rag_attempt(
                            pool.clone(),
                            kb_name,
                            "deep-research",
                            model,
                            &selected.name,
                            status as i32,
                            pt as i64,
                            ct as i64,
                            tt as i64,
                            dur,
                            None,
                            Some(chat_body.to_string()),
                            Some(body.to_string()),
                            trace_id,
                            provider_request_id,
                        );
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
                    log_rag_attempt(
                        pool.clone(),
                        kb_name,
                        "deep-research",
                        model,
                        &selected.name,
                        status as i32,
                        0,
                        0,
                        0,
                        dur,
                        Some(msg.clone()),
                        Some(chat_body.to_string()),
                        Some(body.to_string()),
                        trace_id,
                        provider_request_id,
                    );
                    let retryable =
                        status >= 500 || status == 429 || status == 408 || status == 409;
                    if !retryable || !fo.should_retry() {
                        return Err(AppError::Proxy(format!("上游返回 {}: {}", status, msg)));
                    }
                }
                Err(e) => {
                    let dur = af_start.elapsed().as_millis() as i64;
                    log_rag_attempt(
                        pool.clone(),
                        kb_name,
                        "deep-research",
                        model,
                        &selected.name,
                        502,
                        0,
                        0,
                        0,
                        dur,
                        Some(e.to_string()),
                        Some(chat_body.to_string()),
                        None,
                        trace_id,
                        None,
                    );
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
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    s
}

/// Deep Research：多轮迭代检索 + 综合。
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

    // 链路追踪：本次 Deep Research 全部 LLM 子调用共用一个 trace_id。
    let trace_id = Uuid::new_v4().to_string();

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
                &kb.name,
                model,
                channel_id,
                "你是一个研究助手，根据已有发现生成下一步搜索查询。只返回查询本身。",
                &prompt,
                &trace_id,
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
            &kb.name,
            model,
            channel_id,
            "你是深度研究助手。基于 RAG 内容进行多轮迭代研究，逐步深入分析。",
            &round_prompt,
            &trace_id,
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
        &kb.name,
        model,
        channel_id,
        "你是深度研究助手，综合多轮研究发现给出最终回答。",
        &final_prompt,
        &trace_id,
    )
    .await?;

    Ok(AskResult {
        answer,
        sources: dedupe_sources(all_sources),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // ============================================================
    // channel_serves_model —— 渠道是否服务于给定模型
    // ============================================================

    fn row_with(models: &str, mapping: &str) -> ChannelRow {
        ChannelRow {
            id: "ch-1".into(),
            name: "test-channel".into(),
            protocol: "openai".into(),
            channel_type: "openai".into(),
            base_url: "https://api.example.com/v1".into(),
            cred_encrypted: String::new(),
            models: models.into(),
            status: 1,
            priority: 0,
            weight: 1,
            config: "{}".into(),
            model_mapping: mapping.into(),
            endpoints: "[]".into(),
            created_at: String::new(),
            updated_at: String::new(),
            last_test_at: None,
            last_test_ok: None,
        }
    }

    #[test]
    fn channel_serves_model_matches_models_array() {
        let row = row_with(r#"["gpt-4o","gpt-4o-mini"]"#, "{}");
        assert!(channel_serves_model(&row, "gpt-4o"));
        assert!(channel_serves_model(&row, "gpt-4o-mini"));
        assert!(!channel_serves_model(&row, "claude-3"));
    }

    #[test]
    fn channel_serves_model_matches_only_mapping_key_not_value() {
        let row = row_with("[]", r#"{"gpt-4o":"my-gpt4o"}"#);
        assert!(channel_serves_model(&row, "gpt-4o"));
        assert!(
            !channel_serves_model(&row, "my-gpt4o"),
            "映射的值不应被当作上游可服务模型"
        );
    }

    #[test]
    fn channel_serves_model_accepts_either_source() {
        let row = row_with(r#"["a"]"#, r#"{"b":"x"}"#);
        assert!(channel_serves_model(&row, "a"));
        assert!(channel_serves_model(&row, "b"));
        assert!(!channel_serves_model(&row, "c"));
    }

    #[test]
    fn channel_serves_model_tolerates_malformed_json() {
        // 两个字段都不是合法 JSON：应返回 false 而不是 panic
        let row = row_with("not-json", "also-not-json");
        assert!(!channel_serves_model(&row, "gpt-4o"));

        // models 是对象而非数组 -> 反序列化失败，回落到 mapping
        let row = row_with("{}", r#"{"gpt-4o":"x"}"#);
        assert!(channel_serves_model(&row, "gpt-4o"));

        // mapping 为 null -> .get() 恒为 None
        let row = row_with("[]", "null");
        assert!(!channel_serves_model(&row, "gpt-4o"));
    }

    #[test]
    fn channel_serves_model_empty_fields_yield_false() {
        let row = row_with("", "");
        assert!(!channel_serves_model(&row, "gpt-4o"));
    }

    // ============================================================
    // dedupe_sources —— 来源去重（键 = kb_id + doc_title + content）
    // ============================================================

    fn src(kb: &str, title: &str, content: &str, score: f32) -> Source {
        Source {
            kb_id: kb.into(),
            doc_title: title.into(),
            content: content.into(),
            score,
        }
    }

    fn keys(v: &[Source]) -> Vec<(String, String, String)> {
        v.iter()
            .map(|s| (s.kb_id.clone(), s.doc_title.clone(), s.content.clone()))
            .collect()
    }

    #[test]
    fn dedupe_sources_empty_in_empty_out() {
        assert!(dedupe_sources(vec![]).is_empty());
    }

    #[test]
    fn dedupe_sources_keeps_unique_in_order() {
        let out = dedupe_sources(vec![
            src("kb1", "a", "x", 0.9),
            src("kb1", "b", "y", 0.8),
            src("kb2", "c", "z", 0.7),
        ]);
        assert_eq!(
            keys(&out),
            vec![
                ("kb1".to_string(), "a".to_string(), "x".to_string()),
                ("kb1".to_string(), "b".to_string(), "y".to_string()),
                ("kb2".to_string(), "c".to_string(), "z".to_string()),
            ]
        );
    }

    #[test]
    fn dedupe_sources_collapses_full_key_match_keeping_first() {
        let out = dedupe_sources(vec![
            src("kb1", "t", "same", 0.9),
            src("kb1", "t", "same", 0.3), // 全键相同 -> 丢弃
            src("kb1", "t", "other", 0.8),
        ]);
        assert_eq!(out.len(), 2);
        assert_eq!(out[0].score, 0.9, "应保留首次出现的那条（含其分数）");
        assert_eq!(out[1].content, "other");
    }

    #[test]
    fn dedupe_sources_requires_all_three_fields_to_match() {
        let out = dedupe_sources(vec![
            src("kb1", "t", "c", 1.0),
            src("kb2", "t", "c", 0.9), // kb_id 不同 -> 保留
            src("kb1", "u", "c", 0.8), // doc_title 不同 -> 保留
            src("kb1", "t", "d", 0.7), // content 不同 -> 保留
            src("kb1", "t", "c", 0.6), // 三者全同 -> 去重
        ]);
        assert_eq!(out.len(), 4);
        assert_eq!(out.last().unwrap().score, 0.7);
    }

    #[test]
    fn dedupe_sources_handles_many_repeats() {
        let mut v = Vec::new();
        for i in 0..50 {
            v.push(src("kb", "t", &format!("c{}", i % 5), i as f32));
        }
        let out = dedupe_sources(v);
        assert_eq!(out.len(), 5);
        assert_eq!(out[0].content, "c0");
        assert_eq!(out[4].content, "c4");
    }

    // 上游 key 的「解密 + 加权挑选」已统一到 `core::upstream_key::pick_upstream_key`，
    // 其行为（含「数组内无有效 key 不再回退成 JSON 原文」这一修复）在
    // `core/upstream_key.rs` 的单测中固化，此处不再重复。
}
