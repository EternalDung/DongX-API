//! RAG 知识库管理命令（Phase 1）。
//!
//! 对应前端 `knowledgeApi`：
//! - `list_knowledge_bases`   → `knowledgeApi.list`
//! - `create_knowledge_base`  → `knowledgeApi.create`
//! - `delete_knowledge_base`  → `knowledgeApi.remove`
//!
//! 返回结构完全对齐 `src/types/index.ts` 的 `KnowledgeBase`；
//! 嵌入渠道在创建时按「启用 + OpenAI 系 + 勾选 Embeddings 端点」自动解析，
//! 因此 `KnowledgeBaseInput` 只需 `name / description / embedding_model`。

use crate::AppState;
use crate::rag::ask::AskResult;
use crate::rag::ingest::IngestResult;
use serde::Serialize;
use std::sync::Arc;
use tauri::State;

/// 知识库（对齐前端 `KnowledgeBase`）。
///
/// `FromRow` 供 `list_knowledge_bases` 的 `query_as` 直接映射；
/// `Serialize` 供 Tauri 命令返回 JSON 给前端。
/// `doc_count` / `chunk_count` 由 list 查询的 LEFT JOIN 子查询聚合得出。
#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct KnowledgeBase {
    pub id: String,
    pub name: String,
    pub description: String,
    pub embedding_model: String,
    pub embedding_channel_id: String,
    pub status: i64,
    pub created_at: String,
    pub updated_at: String,
    pub doc_count: i64,
    pub chunk_count: i64,
}

/// 新建知识库入参（对齐前端 `KnowledgeBaseInput`）。
#[derive(Debug, serde::Deserialize)]
pub struct KnowledgeBaseInput {
    pub name: String,
    pub description: String,
    pub embedding_model: String,
}

/// 解析一个支持 Embeddings 的启用渠道。
///
/// 这些 `type` 均走 OpenAI 适配器，上游标准 `/v1/embeddings` 端点可用：
/// `openai` / `deepseek` / `qwen` / `zhipu` / `doubao` / `moonshot` / `custom`
/// （`claude` / `gemini` / `ollama` 走各自协议，不支持标准 embeddings 端点，故排除）。
///
/// 优先选择显式勾选了 embeddings 端点的渠道；兜底选择任意启用的
/// OpenAI 兼容渠道（按优先级、创建时间排序）。取优先级最高、创建最早的那个。
async fn resolve_embedding_channel(pool: &sqlx::SqlitePool) -> Result<String, String> {
    // 兼容渠道 type 白名单（常量，无用户输入，可安全拼接到 SQL）。
    const COMPATIBLE: &str =
        "'openai','deepseek','qwen','zhipu','doubao','moonshot','custom'";
    let sql = format!(
        "SELECT id FROM channels \
         WHERE status = 1 AND type IN ({COMPATIBLE}) \
         ORDER BY (CASE WHEN endpoints LIKE '%embeddings%' THEN 0 ELSE 1 END), \
                  priority DESC, created_at DESC LIMIT 1"
    );
    sqlx::query_scalar::<_, String>(&sql)
        .fetch_optional(pool)
        .await
        .map_err(|e| e.to_string())?
        .ok_or_else(|| {
            "未找到支持 Embeddings 的启用渠道：请先在「渠道管理」中启用一个 OpenAI 兼容渠道\
             （如 OpenAI / DeepSeek / 通义千问 / 智谱 / 自定义 OpenAI 协议），\
             该渠道将用于文档向量化"
                .to_string()
        })
}

/// 列出全部知识库（含文档 / 分块统计），按创建时间倒序。
#[tauri::command]
pub async fn list_knowledge_bases(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<KnowledgeBase>, String> {
    let pool = &state.db;
    let rows = sqlx::query_as::<_, KnowledgeBase>(
        "SELECT kb.id, kb.name, kb.description, kb.embedding_model,
                kb.embedding_channel_id, kb.status, kb.created_at, kb.updated_at,
                COALESCE(d.cnt, 0) AS doc_count,
                COALESCE(c.cnt, 0) AS chunk_count
         FROM knowledge_bases kb
         LEFT JOIN (SELECT kb_id, COUNT(*) AS cnt FROM kb_documents GROUP BY kb_id) d
           ON d.kb_id = kb.id
         LEFT JOIN (SELECT kb_id, COUNT(*) AS cnt FROM kb_chunks GROUP BY kb_id) c
           ON c.kb_id = kb.id
         ORDER BY kb.created_at DESC",
    )
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// 新建知识库。嵌入渠道自动解析（见 `resolve_embedding_channel`）。
#[tauri::command]
pub async fn create_knowledge_base(
    state: State<'_, Arc<AppState>>,
    input: KnowledgeBaseInput,
) -> Result<KnowledgeBase, String> {
    let pool = &state.db;

    let name = input.name.trim().to_string();
    if name.is_empty() {
        return Err("知识库名称不能为空".to_string());
    }
    let embedding_model = input.embedding_model.trim().to_string();
    if embedding_model.is_empty() {
        return Err("嵌入模型不能为空".to_string());
    }

    let channel_id = resolve_embedding_channel(pool).await?;

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO knowledge_bases
           (id, name, description, embedding_model, embedding_channel_id, status, created_at, updated_at)
         VALUES (?, ?, ?, ?, ?, 1, ?, ?)",
    )
    .bind(&id)
    .bind(&name)
    .bind(&input.description)
    .bind(&embedding_model)
    .bind(&channel_id)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(KnowledgeBase {
        id,
        name,
        description: input.description,
        embedding_model,
        embedding_channel_id: channel_id,
        status: 1,
        created_at: now.clone(),
        updated_at: now,
        doc_count: 0,
        chunk_count: 0,
    })
}

/// 删除知识库，级联删除其文档与分块。
#[tauri::command]
pub async fn delete_knowledge_base(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    let pool = &state.db;
    // 先删子表，避免外键式孤儿（本项目未开 FK，需手动级联）。
    sqlx::query("DELETE FROM kb_chunks WHERE kb_id = ?")
        .bind(&id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("DELETE FROM kb_documents WHERE kb_id = ?")
        .bind(&id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    let res = sqlx::query("DELETE FROM knowledge_bases WHERE id = ?")
        .bind(&id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    if res.rows_affected() == 0 {
        return Err(format!("知识库不存在: {}", id));
    }
    Ok(())
}

/// 摄入一段文本到指定知识库：分块 → 向量化 → 落库。
/// 返回新建文档 id 与分块数。
#[tauri::command]
pub async fn ingest_kb_text(
    state: State<'_, Arc<AppState>>,
    kb_id: String,
    title: String,
    text: String,
) -> Result<IngestResult, String> {
    crate::rag::ingest::ingest_text(&state.db, &kb_id, &title, &text)
        .await
        .map_err(|e| e.to_string())
}

/// 在指定知识库范围内问答：检索相关分块 → 构造上下文 → 复用网关分发发起 chat。
#[tauri::command]
pub async fn ask_kb(
    state: State<'_, Arc<AppState>>,
    kb_ids: Vec<String>,
    question: String,
    model: String,
) -> Result<AskResult, String> {
    crate::rag::ask::ask(&state.db, &kb_ids, &question, &model)
        .await
        .map_err(|e| e.to_string())
}

/// 知识库文档（对齐前端 `KbDocument`）。
/// `FromRow` 映射 `kb_documents`；`Serialize` 供 Tauri 命令返回 JSON。
#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct KbDocument {
    pub id: String,
    pub kb_id: String,
    pub title: String,
    pub source_type: String,
    pub source_ref: String,
    pub char_count: i64,
    pub chunk_count: i64,
    pub status: i64,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 列出某知识库下的全部文档（含片段数与状态），按创建时间倒序。
#[tauri::command]
pub async fn list_documents(
    state: State<'_, Arc<AppState>>,
    kb_id: String,
) -> Result<Vec<KbDocument>, String> {
    let rows = sqlx::query_as::<_, KbDocument>(
        "SELECT id, kb_id, title, source_type, source_ref, char_count,
                chunk_count, status, error_message, created_at, updated_at
         FROM kb_documents WHERE kb_id = ? ORDER BY created_at DESC",
    )
    .bind(&kb_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// 删除文档，并级联删除其下全部向量分块（本项目未开 FK，需手动级联）。
#[tauri::command]
pub async fn delete_document(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    let pool = &state.db;
    sqlx::query("DELETE FROM kb_chunks WHERE doc_id = ?")
        .bind(&id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    let res = sqlx::query("DELETE FROM kb_documents WHERE id = ?")
        .bind(&id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    if res.rows_affected() == 0 {
        return Err(format!("文档不存在: {}", id));
    }
    Ok(())
}
