//! 内部嵌入调用：直接复用网关的渠道分发 / 密钥解密 / 适配器转发机制，
//! 绕过 HTTP 鉴权与限流（进程内可信调用）。报文为 OpenAI embeddings 格式，
//! 与 Phase 0 的 `handler::embeddings` 同构，但不走 HTTP 层。

use sqlx::SqlitePool;

use crate::adapter::{get_adaptor, ChannelConfig, ProxyRequest};
use crate::core::dispatcher;
use crate::db::repository::channels;
use crate::error::AppError;
use crate::models::ChannelRow;

/// 单次嵌入请求的最大输入条数。
///
/// 部分上游对批量嵌入的 `input` 数组长度有硬限制（如 DashScope
/// text-embedding 系列为 20 条，超限直接 400：`batch size is invalid,
/// it should not be larger than 20`）。这里取更保守的 10 条切批，
/// 逐批转发后按原顺序拼接，对调用方透明（入参/返回语义不变）。
const EMBED_BATCH_SIZE: usize = 10;

/// 对一批文本批量嵌入，返回与输入等长的向量列表，以及该批次的总 token 数。
///
/// 返回的 token 数取自 OpenAI 风格响应的 `usage.prompt_tokens`
/// （整批合计），用于文档级 token 统计；响应缺 `usage` 时回退 0。
///
/// - `channel_id`：知识库绑定的嵌入渠道（已校验存在）
/// - `model`：嵌入模型名（如 `text-embedding-3-small`）
/// - 复用 `dispatcher::pick_one` 解密上游密钥并构建 `SelectedChannel`
pub async fn embed_texts(
    pool: &SqlitePool,
    channel_id: &str,
    model: &str,
    inputs: Vec<String>,
) -> Result<(Vec<Vec<f32>>, i64), AppError> {
    if inputs.is_empty() {
        return Ok((Vec::new(), 0));
    }

    let row: ChannelRow = channels::get_by_id(pool, channel_id)
        .await?
        .ok_or_else(|| AppError::NotFound(format!("嵌入渠道不存在: {}", channel_id)))?;

    // pick_one 在单候选上运行：解密上游密钥并产出 SelectedChannel。
    let selected = dispatcher::pick_one(std::slice::from_ref(&row))?;

    let config = ChannelConfig {
        base_url: selected.base_url.clone(),
        api_key: selected.upstream_api_key.clone(),
        models: selected.models.clone(),
        model_mapping: selected.model_mapping.clone(),
        extra: selected.extra.clone(),
        timeout_secs: selected.timeout_secs,
        stream: false,
    };
    let adaptor = get_adaptor(&row.channel_type);

    // 大文档一次摄入可能产生数十上百个分块，超出上游单批上限会被 400
    // 拒绝，因此按 EMBED_BATCH_SIZE 切批逐批请求，结果按原顺序拼接。
    let mut out: Vec<Vec<f32>> = Vec::with_capacity(inputs.len());
    let mut total_tokens: i64 = 0;

    for batch in inputs.chunks(EMBED_BATCH_SIZE) {
        let body = serde_json::json!({ "model": model, "input": batch });
        let proxy_req = ProxyRequest {
            model: model.to_string(),
            body,
            stream: false,
        };

        let (status, resp) = adaptor
            .forward_embeddings(&proxy_req, &config)
            .await
            .map_err(AppError::from)?;

        if !(200..300).contains(&status) {
            let msg = resp
                .get("error")
                .and_then(|e| e.get("message"))
                .and_then(|m| m.as_str())
                .unwrap_or("嵌入请求失败")
                .to_string();
            return Err(AppError::Proxy(format!("嵌入上游返回 {}: {}", status, msg)));
        }

        let data = resp
            .get("data")
            .and_then(|d| d.as_array())
            .ok_or_else(|| AppError::Proxy("嵌入返回缺少 data 字段".into()))?;

        if data.len() != batch.len() {
            return Err(AppError::Proxy(format!(
                "嵌入返回的向量数量({})与输入数量({})不一致",
                data.len(),
                batch.len()
            )));
        }

        for item in data {
            let emb = item
                .get("embedding")
                .and_then(|e| e.as_array())
                .ok_or_else(|| AppError::Proxy("嵌入返回缺少 embedding 字段".into()))?;
            let vec: Vec<f32> = emb
                .iter()
                .filter_map(|v| v.as_f64().map(|f| f as f32))
                .collect();
            out.push(vec);
        }

        // 整批 token 数：OpenAI 风格 `usage.prompt_tokens`（缺失则 0）。
        total_tokens += resp
            .get("usage")
            .and_then(|u| u.get("prompt_tokens").or_else(|| u.get("total_tokens")))
            .and_then(|t| t.as_i64())
            .unwrap_or(0);
    }

    Ok((out, total_tokens))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn batch_size_is_conservative() {
        // 上游单批上限常见为 10~20，切批大小必须不超过最严格的上限，
        // 否则大文档摄入会被上游 400（batch size is invalid）拒绝。
        const {
            assert!(EMBED_BATCH_SIZE <= 10);
        }
    }
}
