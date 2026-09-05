//! KnowledgeService —— 本地 RAG 知识库服务（Phase 0 骨架）。
//!
//! 本阶段只落地服务外壳：自述信息、`/v1/rag/health` 健康端点、`status()`
//! 统计（kb_* 表尚未随 009 迁移建立时以 0 兜底）。真实的摄入/检索/问答路由
//! 在后续 Phase 接入。

use super::{Service, ServiceStatus};
use crate::AppState;
use async_trait::async_trait;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use axum::routing::{get, post};
use axum::{Json, Router};
use serde::Deserialize;
use serde_json::json;
use std::sync::Arc;
use tauri::AppHandle;
use tauri::Manager;

pub struct KnowledgeService;

#[async_trait]
impl Service for KnowledgeService {
    fn id(&self) -> &'static str {
        "knowledge"
    }

    fn name(&self) -> &'static str {
        "RAG"
    }

    fn description(&self) -> &'static str {
        "本地 RAG 知识库：创建私有知识库，上传文档自动分块、嵌入并落库，通过检索增强问答（后续 Phase 提供 /v1/rag/ask 等端点）"
    }

    fn enabled(&self) -> bool {
        true
    }

    async fn status(&self, state: &AppState) -> ServiceStatus {
        // kb_* 表尚未随 009 迁移建立时，查询会失败 —— `unwrap_or(0)` 兜底返回 0，
        // 保证 Phase 0（未建表）也能正常出状态而不报错。
        let pool = &state.db;
        let kb_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM knowledge_bases")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        let doc_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM kb_documents")
            .fetch_one(pool)
            .await
            .unwrap_or(0);
        let chunk_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM kb_chunks")
            .fetch_one(pool)
            .await
            .unwrap_or(0);

        ServiceStatus {
            id: self.id().to_string(),
            name: self.name().to_string(),
            description: self.description().to_string(),
            enabled: self.enabled(),
            running: true,
            stats: json!({
                "knowledge_bases": kb_count,
                "documents": doc_count,
                "chunks": chunk_count,
            }),
        }
    }

    fn routes(&self) -> Router<AppHandle> {
        Router::new()
            .route("/v1/rag/health", get(health))
            .route("/v1/rag/ask", post(ask_route))
    }
}

/// GET /v1/rag/health —— 服务自述，验证路由注册与合并生效。
async fn health(State(_app): State<AppHandle>) -> impl IntoResponse {
    (
        StatusCode::OK,
        Json(json!({
            "service": "knowledge",
            "enabled": true,
            "running": true,
        })),
    )
}

/// POST /v1/rag/ask —— 知识库问答（A 型：独立 KB 问答）。
async fn ask_route(
    State(app): State<AppHandle>,
    Json(payload): Json<AskRequest>,
) -> impl IntoResponse {
    let state: Arc<AppState> = app.state::<Arc<AppState>>().inner().clone();
    let result = if payload.deep_research {
        crate::rag::ask::ask_deep_research(
            &state.db,
            &payload.kb_ids,
            &payload.question,
            &payload.model,
            payload.channel_id.as_deref(),
            crate::rag::retrieve::RetrievalMode::Vector,
            5,
            0.3,
            payload.max_rounds as usize,
        )
        .await
    } else {
        crate::rag::ask::ask(
            &state.db,
            &payload.kb_ids,
            &payload.question,
            &payload.model,
            payload.channel_id.as_deref(),
            crate::rag::retrieve::RetrievalMode::Vector,
            5,
            0.3,
        )
        .await
    };
    match result {
        Ok(res) => (StatusCode::OK, Json(res)).into_response(),
        Err(e) => e.into_response(),
    }
}

/// `/v1/rag/ask` 请求体。
#[derive(Deserialize)]
struct AskRequest {
    /// 参与检索的知识库 id 列表
    kb_ids: Vec<String>,
    /// 用户问题
    question: String,
    /// 用于生成回答的 chat 模型（经网关分发，独立于嵌入模型）
    #[serde(default)]
    model: String,
    /// 可选：锁定单一渠道直接发（不走 Failover/熔断/加权）。
    /// 前端 RAG 问答 UI 在用户显式选择渠道时传此字段。
    #[serde(default)]
    channel_id: Option<String>,
    /// 可选：开启 Deep Research 多轮迭代检索
    #[serde(default)]
    deep_research: bool,
    /// 可选：Deep Research 最大轮数（默认 5）
    #[serde(default = "default_max_rounds")]
    max_rounds: u32,
}

/// Deep Research 默认轮数，与 Tauri 命令 `ask_kb` 保持一致。
fn default_max_rounds() -> u32 {
    5
}
