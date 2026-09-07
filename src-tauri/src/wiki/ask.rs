//! Wiki 问答：取项目配置的对话渠道与模型，加载页面做关键词+链接检索，
//! 复用网关分发机制（decrypt → adaptor.forward）发起 chat 完成，
//! 返回答案与引用片段。对应 Tauri 命令 `ask_wiki`。

use serde_json::Value;
use sqlx::SqlitePool;

use crate::adapter::{get_adaptor, ChannelConfig, ProxyRequest};
use crate::core::upstream_key::pick_upstream_key;
use crate::db::repository::request_logs;
use crate::error::{AppError, AppResult};
use crate::models::ChannelRow;
use crate::wiki::store;
use crate::wiki::store::{WikiAskResult, WikiCitation, WikiPage};

/// Wiki 问答主入口。
pub async fn ask(pool: &SqlitePool, project_id: &str, question: &str) -> AppResult<WikiAskResult> {
    let proj = store::get_project_base(pool, project_id).await?;
    if proj.chat_channel_id.is_empty() || proj.chat_model.is_empty() {
        return Err(AppError::Validation(
            "未配置对话渠道或对话模型，请先在设置中指定".into(),
        ));
    }
    let channel = sqlx::query_as::<_, ChannelRow>("SELECT * FROM channels WHERE id = ?")
        .bind(&proj.chat_channel_id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::Validation("对话渠道不存在或已被删除".into()))?;

    let pages = store::list_pages(pool, project_id).await?;
    if pages.is_empty() {
        return Err(AppError::Validation(
            "该项目还没有任何页面，请先摄入来源".into(),
        ));
    }

    let ranked = retrieve(&pages, question, 5);

    // 构造检索上下文
    let mut ctx = String::new();
    for p in &ranked {
        let excerpt: String = p.content.chars().take(1600).collect();
        ctx.push_str(&format!("\n## {}\n{}\n", p.title, excerpt));
        if !p.links.is_empty() {
            ctx.push_str(&format!("相关页面：{}\n", p.links.join("、")));
        }
    }

    let system = format!(
        "你是「{}」知识库的问答助手。仅依据下面提供的页面内容作答；引用页面时使用 [[页面标题]] 形式。\
         若资料中没有答案，请明确说明无法从现有 Wiki 中找到。",
        proj.name
    );
    let user = format!("{question}\n\n--- 相关资料 ---{ctx}");
    let body = serde_json::json!({
        "model": proj.chat_model,
        "messages": [
            {"role": "system", "content": system},
            {"role": "user", "content": user}
        ],
        "temperature": 0.3
    });

    let (answer, pt, ct, dur) =
        call_chat_once(pool, &proj.name, &channel, &proj.chat_model, &body).await?;

    let citations = ranked
        .iter()
        .map(|p| WikiCitation {
            title: p.title.clone(),
            slug: p.slug.clone(),
            excerpt: p.content.chars().take(220).collect(),
        })
        .collect();

    Ok(WikiAskResult {
        answer,
        citations,
        prompt_tokens: pt,
        completion_tokens: ct,
        duration_ms: dur,
    })
}

/// 关键词+链接检索：按标题/正文/链接与问题的重叠打分，取 top-N。
fn retrieve(pages: &[WikiPage], question: &str, top: usize) -> Vec<WikiPage> {
    let q = question.to_lowercase();
    let qtokens: Vec<&str> = q
        .split(|c: char| !c.is_alphanumeric() && !c.is_alphabetic())
        .filter(|s| !s.is_empty())
        .collect();

    let mut scored: Vec<(i64, &WikiPage)> = pages
        .iter()
        .map(|p| {
            let mut score = 0i64;
            let title = p.title.to_lowercase();
            let content = p.content.to_lowercase();
            let links = p.links.join(" ").to_lowercase();
            if title.contains(&q) {
                score += 50;
            }
            for t in &qtokens {
                if title.contains(t) {
                    score += 10;
                }
                if content.contains(t) {
                    score += 3;
                }
                if links.contains(t) {
                    score += 5;
                }
            }
            (score, p)
        })
        .collect();

    scored.retain(|(s, _)| *s > 0);
    scored.sort_by_key(|a| std::cmp::Reverse(a.0));
    scored
        .into_iter()
        .take(top)
        .map(|(_, p)| p.clone())
        .collect()
}

/// 单次直接对指定渠道发起 chat 调用：解密 key、选择上游 key、转发并解析 answer。
/// 每次调用都会经 `log_wiki_attempt` 落库一条请求日志（标识为 `WIKI: {项目名}`）。
async fn call_chat_once(
    pool: &SqlitePool,
    kb_name: &str,
    row: &ChannelRow,
    model: &str,
    chat_body: &Value,
) -> AppResult<(String, i64, i64, i64)> {
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
        Ok((status, body, _usage)) => {
            if !(200..300).contains(&status) {
                let msg = body
                    .get("error")
                    .and_then(|e| e.get("message"))
                    .and_then(|m| m.as_str())
                    .unwrap_or("上游返回错误")
                    .to_string();
                log_wiki_attempt(
                    pool.clone(),
                    kb_name,
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
                );
                return Err(AppError::Proxy(format!("上游返回 {status}: {msg}")));
            }
            let pt = body
                .get("usage")
                .and_then(|u| u.get("prompt_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as i64;
            let ct = body
                .get("usage")
                .and_then(|u| u.get("completion_tokens"))
                .and_then(|v| v.as_u64())
                .unwrap_or(0) as i64;
            let answer = body
                .get("choices")
                .and_then(|c| c.get(0))
                .and_then(|c| c.get("message"))
                .and_then(|m| m.get("content"))
                .and_then(|c| c.as_str())
                .unwrap_or("")
                .to_string();
            log_wiki_attempt(
                pool.clone(),
                kb_name,
                model,
                &row.name,
                status as i32,
                pt,
                ct,
                pt + ct,
                dur,
                None,
                Some(chat_body.to_string()),
                Some(body.to_string()),
            );
            Ok((answer, pt, ct, dur))
        }
        Err(e) => {
            let msg = e.to_string();
            log_wiki_attempt(
                pool.clone(),
                kb_name,
                model,
                &row.name,
                0,
                0,
                0,
                0,
                dur,
                Some(msg.clone()),
                Some(chat_body.to_string()),
                None,
            );
            Err(AppError::Proxy(msg))
        }
    }
}

/// Wiki 内部 LLM 调用的落库助手：与网关 `spawn_log` 写入同一张 `request_logs` 表，
/// 但 Wiki 不经网关鉴权、无稳定网关 key，故 `api_key_id` 恒为 NULL，
/// `api_key_name` 统一标注为 `WIKI: {项目名}` 以区分来源。
fn log_wiki_attempt(
    pool: SqlitePool,
    kb_name: &str,
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
) {
    let api_key_name = format!("WIKI: {kb_name}");
    let channel = channel_name.to_string();
    let model = model.to_string();
    tokio::spawn(async move {
        if let Err(e) = request_logs::insert(
            &pool,
            Some(api_key_name.as_str()),
            None,
            Some(channel.as_str()),
            &model,
            None,
            "wiki",
            status,
            prompt_tokens,
            completion_tokens,
            total_tokens,
            duration_ms,
            error_message.as_deref(),
            false,
            false,
            request_body.as_deref(),
            response_body.as_deref(),
            "none",
            0,
            None,
            "allow",
            false,
            None,
        )
        .await
        {
            tracing::warn!("Wiki 请求日志写入失败: {e}");
        }
    });
}
