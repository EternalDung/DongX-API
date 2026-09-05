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
use crate::rag::retrieve::RetrievalMode;
use serde::Serialize;
use sqlx::Row;
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
    /// 是否将本知识库暴露给 MCP 层（0=否 1=是）。
    pub mcp_exposed: i64,
    /// 单次向量化批大小（NULL=取引擎默认）。
    pub embedding_batch_size: Option<i64>,
    /// 摄入时排除的目录（逗号分隔，NULL=不排除）。
    pub exclude_dirs: Option<String>,
    /// 摄入时排除的文件（逗号分隔，NULL=不排除）。
    pub exclude_files: Option<String>,
    /// 摄入时仅包含的文件类型（逗号分隔，NULL=全部）。
    pub include_file_types: Option<String>,
    /// 分块大小（字符数，0=引擎默认）。
    pub chunk_size: i64,
    /// 分块重叠字符数（0=引擎默认）。
    pub chunk_overlap: i64,
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
    /// 绑定的嵌入渠道。省略（或为空）时由 [`resolve_embedding_channel`]
    /// 自动挑选，保持「只传模型」的老调用方式继续可用。
    #[serde(default)]
    pub embedding_channel_id: Option<String>,
}

/// 导入来源入参（对齐前端 `ImportSourceInput`）。
///
/// `source_type`：`git` | `url` | `local_dir`，决定哪些字段生效：
/// - git：需要 `repo_url`，可选 `branch` / `token`
/// - url：需要 `url`
/// - local_dir：需要 `dir_path`
///
/// 共享过滤：`subpath` / `excluded_dirs` / `included_files` 为逗号分隔字符串，
/// `max_file_size_mb` 以 MB 为单位（NULL/0 → 引擎默认 1MB）。
/// 这些过滤与知识库设置里的全局过滤在命令层合并后生效。
#[derive(Debug, serde::Deserialize)]
pub struct ImportSourceInput {
    pub source_type: String,
    pub repo_url: Option<String>,
    pub branch: Option<String>,
    pub token: Option<String>,
    pub url: Option<String>,
    pub dir_path: Option<String>,
    pub subpath: Option<String>,
    pub excluded_dirs: Option<String>,
    pub included_files: Option<String>,
    pub max_file_size_mb: Option<i64>,
}

/// 来源记录（对齐前端 `KbSource`）。
/// `FromRow` 映射 `kb_sources`；`Serialize` 供 Tauri 命令返回 JSON。
/// **注意**：不返回 `token` 字段，避免把密钥回传到前端。
#[derive(Debug, sqlx::FromRow, Serialize)]
pub struct KbSource {
    pub id: String,
    pub kb_id: String,
    pub source_type: String,
    pub repo_url: Option<String>,
    pub branch: Option<String>,
    pub url: Option<String>,
    pub dir_path: Option<String>,
    pub subpath: Option<String>,
    pub status: String,
    pub file_count: i64,
    pub error_message: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// 检索命中的单个分块（对应前端 `RetrievalHit`）。
#[derive(Debug, Serialize)]
pub struct RetrievalHit {
    pub doc_id: String,
    pub doc_title: String,
    pub content: String,
    /// 余弦相似度（0~1，越大越相关）。
    pub score: f32,
}

/// 更新知识库入参（对齐前端 `KnowledgeBaseUpdate`）。
/// 所有字段可选，仅传变更项；`updated_at` 由命令统一刷新。
#[derive(Debug, serde::Deserialize, Default)]
pub struct KnowledgeBaseUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    /// 启用 RAG 开关：0=禁用 1=启用（复用 status 列）。
    pub status: Option<i64>,
    /// MCP 暴露开关：0=否 1=是。
    pub mcp_exposed: Option<i64>,
    /// 嵌入模型。
    ///
    /// 不同嵌入模型的向量空间互不兼容，改了模型后旧分块的 `embedding_model`
    /// 与新值不一致，会被 [`compute_index_status`] 判定为 stale，
    /// 必须调用 `reindex_kb` 重建后才能正常检索（否则向量/混合检索会静默
    /// 返回无意义的结果；纯关键词检索不受影响）。
    pub embedding_model: Option<String>,
    /// 绑定的嵌入渠道（须为已启用渠道）。
    ///
    /// 与 `embedding_model` 配套：换渠道通常也要换模型，因为各渠道提供的
    /// 嵌入模型不同。这里只校验渠道存在且启用，不硬校验模型是否在该渠道的
    /// `models` 列表里——很多中转站的模型列表并不完整。
    pub embedding_channel_id: Option<String>,
    pub embedding_batch_size: Option<i64>,
    pub exclude_dirs: Option<String>,
    pub exclude_files: Option<String>,
    pub include_file_types: Option<String>,
    /// 分块大小（字符数，0=引擎默认）。
    pub chunk_size: Option<i64>,
    /// 分块重叠字符数（0=引擎默认）。
    pub chunk_overlap: Option<i64>,
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
                kb.embedding_channel_id, kb.status,
                kb.mcp_exposed, kb.embedding_batch_size,
                kb.exclude_dirs, kb.exclude_files, kb.include_file_types,
                kb.chunk_size, kb.chunk_overlap,
                kb.created_at, kb.updated_at,
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
        mcp_exposed: 0,
        embedding_batch_size: None,
        exclude_dirs: None,
        exclude_files: None,
        include_file_types: None,
        chunk_size: 0,
        chunk_overlap: 0,
        created_at: now.clone(),
        updated_at: now,
        doc_count: 0,
        chunk_count: 0,
    })
}

/// 更新知识库设置（部分更新，仅传变更字段）。
///
/// `updated_at` 由命令统一刷新；返回更新后的完整 `KnowledgeBase`
/// （含实时重算的 doc_count / chunk_count）。
///
/// 注意：改 `embedding_model` / `embedding_channel_id` **不会**自动重建存量
/// 分块的向量。命令本身成功返回，但旧分块会立刻变成 stale（见
/// [`compute_index_status`]），在调用 `reindex_kb` 之前，向量与混合检索会
/// 拿新模型的查询向量去比旧模型的分块向量，结果是静默的错误排序。
/// 前端应在保存后提示并引导重建索引。
#[tauri::command]
pub async fn update_knowledge_base(
    state: State<'_, Arc<AppState>>,
    id: String,
    patch: KnowledgeBaseUpdate,
) -> Result<KnowledgeBase, String> {
    let pool = &state.db;

    // 嵌入配置校验：模型名不能为空；渠道必须存在且处于启用状态，
    // 否则后续摄入会在嵌入阶段才失败（错误被推迟、难以定位）。
    if let Some(v) = &patch.embedding_model {
        if v.trim().is_empty() {
            return Err("嵌入模型不能为空".to_string());
        }
    }
    if let Some(v) = &patch.embedding_channel_id {
        if v.trim().is_empty() {
            return Err("绑定渠道不能为空".to_string());
        }
        let alive = sqlx::query_scalar::<_, i64>(
            "SELECT COUNT(*) FROM channels WHERE id = ? AND status = 1",
        )
        .bind(v)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;
        if alive == 0 {
            return Err("绑定渠道不存在或已停用".to_string());
        }
    }

    // 动态拼 SET 子句：列名全部来自本函数常量，无用户输入，可安全拼接；
    // 占位值均经 bind 传入，杜绝注入。
    let mut sets: Vec<&str> = Vec::new();
    if patch.name.is_some() {
        sets.push("name = ?");
    }
    if patch.description.is_some() {
        sets.push("description = ?");
    }
    if patch.status.is_some() {
        sets.push("status = ?");
    }
    if patch.mcp_exposed.is_some() {
        sets.push("mcp_exposed = ?");
    }
    if patch.embedding_model.is_some() {
        sets.push("embedding_model = ?");
    }
    if patch.embedding_channel_id.is_some() {
        sets.push("embedding_channel_id = ?");
    }
    if patch.embedding_batch_size.is_some() {
        sets.push("embedding_batch_size = ?");
    }
    if patch.exclude_dirs.is_some() {
        sets.push("exclude_dirs = ?");
    }
    if patch.exclude_files.is_some() {
        sets.push("exclude_files = ?");
    }
    if patch.include_file_types.is_some() {
        sets.push("include_file_types = ?");
    }
    if patch.chunk_size.is_some() {
        sets.push("chunk_size = ?");
    }
    if patch.chunk_overlap.is_some() {
        sets.push("chunk_overlap = ?");
    }
    sets.push("updated_at = ?");

    let sql = format!(
        "UPDATE knowledge_bases SET {} WHERE id = ?",
        sets.join(", ")
    );
    let mut q = sqlx::query(&sql);
    if let Some(v) = &patch.name {
        q = q.bind(v);
    }
    if let Some(v) = &patch.description {
        q = q.bind(v);
    }
    if let Some(v) = &patch.status {
        q = q.bind(v);
    }
    if let Some(v) = &patch.mcp_exposed {
        q = q.bind(v);
    }
    if let Some(v) = &patch.embedding_model {
        q = q.bind(v.trim());
    }
    if let Some(v) = &patch.embedding_channel_id {
        q = q.bind(v.trim());
    }
    if let Some(v) = &patch.embedding_batch_size {
        q = q.bind(v);
    }
    if let Some(v) = &patch.exclude_dirs {
        q = q.bind(v);
    }
    if let Some(v) = &patch.exclude_files {
        q = q.bind(v);
    }
    if let Some(v) = &patch.include_file_types {
        q = q.bind(v);
    }
    if let Some(v) = &patch.chunk_size {
        q = q.bind(v);
    }
    if let Some(v) = &patch.chunk_overlap {
        q = q.bind(v);
    }
    let now = chrono::Utc::now().to_rfc3339();
    q = q.bind(&now);
    q = q.bind(&id);

    let affected = q
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?
        .rows_affected();
    if affected == 0 {
        return Err(format!("知识库不存在: {}", id));
    }

    let updated = sqlx::query_as::<_, KnowledgeBase>(
        "SELECT id, name, description, embedding_model, embedding_channel_id, status,
                mcp_exposed, embedding_batch_size, exclude_dirs, exclude_files, include_file_types,
                chunk_size, chunk_overlap,
                created_at, updated_at,
                (SELECT COUNT(*) FROM kb_documents WHERE kb_id = knowledge_bases.id) AS doc_count,
                (SELECT COUNT(*) FROM kb_chunks WHERE kb_id = knowledge_bases.id) AS chunk_count
         FROM knowledge_bases WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    Ok(updated)
}

/// 删除知识库，级联删除其文档与分块。
#[tauri::command]
pub async fn delete_knowledge_base(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    let pool = &state.db;
    // 先删子表，避免外键式孤儿（本项目未开 FK，需手动级联）。
    // purge_kb_chunks 同时清理 FTS5 全文索引，防止索引与正文漂移。
    crate::rag::store::purge_kb_chunks(pool, &id)
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
    file_size: i64,
) -> Result<IngestResult, String> {
    crate::rag::ingest::ingest_text(&state.db, &kb_id, &title, &text, file_size)
        .await
        .map_err(|e| e.to_string())
}

/// 在指定知识库范围内问答：检索相关分块 → 构造上下文 → 复用网关分发发起 chat。
///
/// `channel_id` 可选：若传入则锁定单一渠道直接发（不走 Failover/熔断），
/// 由前端 UI 显式选择「渠道」时使用。
#[tauri::command]
pub async fn ask_kb(
    state: State<'_, Arc<AppState>>,
    kb_ids: Vec<String>,
    question: String,
    model: String,
    channel_id: Option<String>,
    mode: Option<String>,
    top_k: Option<u32>,
    keyword_weight: Option<f32>,
) -> Result<AskResult, String> {
    let mode = RetrievalMode::from_str_opt(mode.as_deref());
    let top_k = top_k.unwrap_or(5).max(1) as usize;
    let kw = keyword_weight.unwrap_or(0.3);
    crate::rag::ask::ask(
        &state.db,
        &kb_ids,
        &question,
        &model,
        channel_id.as_deref(),
        mode,
        top_k,
        kw,
    )
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
    pub file_size: i64,
    pub token_count: i64,
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
                chunk_count, status, error_message, file_size, token_count,
                created_at, updated_at
         FROM kb_documents WHERE kb_id = ? ORDER BY created_at DESC",
    )
    .bind(&kb_id)
    .fetch_all(&state.db)
        .await
        .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// 文档分片摘要（用于前端「查看分片」下钻预览）。
/// 返回完整 `content`，由前端按需内联展开全文；分页已限制单次体量。
#[derive(Debug, Serialize)]
pub struct DocumentChunk {
    pub seq: i64,
    pub token_count: i64,
    pub symbol_name: Option<String>,
    pub symbol_kind: Option<String>,
    pub language: Option<String>,
    pub line_start: Option<i64>,
    pub line_end: Option<i64>,
    pub content: String,
}

/// 分页分片结果。
#[derive(Debug, Serialize)]
pub struct DocumentChunksPage {
    pub total: i64,
    pub chunks: Vec<DocumentChunk>,
}

/// 列出某文档下的分片（分页），按 `seq` 升序。
/// `limit` 默认 50（上限 200），`offset` 默认 0；用于前端「查看分片」下钻预览。
#[tauri::command]
pub async fn list_document_chunks(
    state: State<'_, Arc<AppState>>,
    doc_id: String,
    limit: Option<i64>,
    offset: Option<i64>,
) -> Result<DocumentChunksPage, String> {
    let pool = &state.db;
    let total: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM kb_chunks WHERE doc_id = ?")
        .bind(&doc_id)
        .fetch_one(pool)
        .await
        .map_err(|e| e.to_string())?;

    let limit = limit.unwrap_or(50).clamp(1, 200);
    let offset = offset.unwrap_or(0).max(0);

    let rows = sqlx::query(
        "SELECT seq, token_count, symbol_name, symbol_kind, language,
                line_start, line_end, content
         FROM kb_chunks WHERE doc_id = ? ORDER BY seq ASC LIMIT ? OFFSET ?",
    )
    .bind(&doc_id)
    .bind(limit)
    .bind(offset)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;

    let chunks = rows
        .into_iter()
        .map(|row| DocumentChunk {
            seq: row.get("seq"),
            token_count: row.get("token_count"),
            symbol_name: row.get("symbol_name"),
            symbol_kind: row.get("symbol_kind"),
            language: row.get("language"),
            line_start: row.get("line_start"),
            line_end: row.get("line_end"),
            content: row.get("content"),
        })
        .collect();

    Ok(DocumentChunksPage { total, chunks })
}

/// 删除文档，并级联删除其下全部向量分块（本项目未开 FK，需手动级联）。
#[tauri::command]
pub async fn delete_document(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    let pool = &state.db;
    // 同步清理 FTS5 索引后再删文档行
    crate::rag::store::purge_document_chunks(pool, &id)
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

/// 把逗号分隔字符串拆成 trimmed 非空向量。
fn split_csv(s: &Option<String>) -> Vec<String> {
    match s {
        Some(v) if !v.trim().is_empty() => v
            .split(',')
            .map(|x| x.trim().to_string())
            .filter(|x| !x.is_empty())
            .collect(),
        _ => Vec::new(),
    }
}

/// 合并知识库全局过滤与来源级覆盖：两者都生效（并集）。
fn merge_csv(base: &Option<String>, override_: &Option<String>) -> Vec<String> {
    let mut out = split_csv(base);
    for x in split_csv(override_) {
        if !out.contains(&x) {
            out.push(x);
        }
    }
    out
}

/// 导入一个来源（Git / URL / 本地目录）。
///
/// 流程：
/// 1. 校验来源类型与必填字段；
/// 2. 读取知识库全局过滤默认值，与来源级覆盖合并；
/// 3. 写 `kb_sources` 行（status='fetching'）；
/// 4. `tokio::spawn` 后台任务跑实际导入（git clone / url fetch / 目录遍历），
///    结束后回写 `done`/`error` + file_count；
/// 5. 立即返回刚创建的行，前端轮询即可看到进度。
#[tauri::command]
pub async fn import_source(
    state: State<'_, Arc<AppState>>,
    kb_id: String,
    input: ImportSourceInput,
) -> Result<KbSource, String> {
    let pool = &state.db;

    let source_type = input.source_type.trim().to_string();
    if !["git", "url", "local_dir"].contains(&source_type.as_str()) {
        return Err("未知的导入来源类型".to_string());
    }

    // 读取知识库全局过滤默认值
    let kb = crate::rag::store::get_kb(pool, &kb_id)
        .await
        .map_err(|e| e.to_string())?;

    let excluded_dirs = merge_csv(&kb.exclude_dirs, &input.excluded_dirs);
    // 导入对话框只暴露「排除目录」与「包含文件类型」；
    // 知识库级 exclude_files 仍作为全局默认生效。
    let exclude_files = split_csv(&kb.exclude_files);
    // included_files：来源级优先，否则回退知识库级
    let included_files = match &input.included_files {
        Some(s) if !s.trim().is_empty() => split_csv(&Some(s.clone())),
        _ => split_csv(&kb.include_file_types),
    };
    let max_file_size = match input.max_file_size_mb {
        Some(mb) if mb > 0 => (mb as usize) * 1024 * 1024,
        _ => 1024 * 1024, // 默认 1MB
    };

    let filters = crate::rag::importer::ImportFilters {
        excluded_dirs,
        exclude_files,
        included_files,
        max_file_size,
    };

    let id = uuid::Uuid::new_v4().to_string();
    let now = chrono::Utc::now().to_rfc3339();

    sqlx::query(
        "INSERT INTO kb_sources \
         (id, kb_id, source_type, repo_url, branch, token, url, dir_path, subpath, \
          excluded_dirs, included_files, max_file_size, status, file_count, error_message, \
          created_at, updated_at) \
         VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, ?, 'fetching', 0, NULL, ?, ?)",
    )
    .bind(&id)
    .bind(&kb_id)
    .bind(&source_type)
    .bind(&input.repo_url)
    .bind(&input.branch)
    .bind(&input.token)
    .bind(&input.url)
    .bind(&input.dir_path)
    .bind(&input.subpath)
    .bind(&input.excluded_dirs)
    .bind(&input.included_files)
    .bind(input.max_file_size_mb)
    .bind(&now)
    .bind(&now)
    .execute(pool)
    .await
    .map_err(|e| e.to_string())?;

    let resolved = crate::rag::importer::ResolvedImport {
        source_type,
        repo_url: input.repo_url.clone(),
        branch: input.branch.clone(),
        token: input.token.clone(),
        url: input.url.clone(),
        dir_path: input.dir_path.clone(),
        subpath: input.subpath.clone(),
        filters,
    };

    // 克隆连接池进后台任务（SqlitePool 内部是 Arc，clone 廉价）
    let pool2 = pool.clone();
    let kb_id2 = kb_id.clone();
    let source_id = id.clone();
    tauri::async_runtime::spawn(async move {
        if let Err(e) =
            crate::rag::importer::run_import(pool2, kb_id2, source_id, resolved).await
        {
            tracing::error!("来源导入后台任务异常: {}", e);
        }
    });

    let row = sqlx::query_as::<_, KbSource>(
        "SELECT id, kb_id, source_type, repo_url, branch, url, dir_path, subpath, \
                status, file_count, error_message, created_at, updated_at \
         FROM kb_sources WHERE id = ?",
    )
    .bind(&id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;
    Ok(row)
}

/// 列出某知识库下的全部来源（含状态与进度），按创建时间倒序。
#[tauri::command]
pub async fn list_sources(
    state: State<'_, Arc<AppState>>,
    kb_id: String,
) -> Result<Vec<KbSource>, String> {
    let rows = sqlx::query_as::<_, KbSource>(
        "SELECT id, kb_id, source_type, repo_url, branch, url, dir_path, subpath, \
                status, file_count, error_message, created_at, updated_at \
         FROM kb_sources WHERE kb_id = ? ORDER BY created_at DESC",
    )
    .bind(&kb_id)
    .fetch_all(&state.db)
    .await
    .map_err(|e| e.to_string())?;
    Ok(rows)
}

/// 删除来源记录（级联删除其下文档与分块）。
///
/// 删除来源即移除「该次导入」的所有文档：按 `source_ref` 前缀
/// `<source_id>::` 定位本次导入产生的文档并清理，避免孤儿分块。
#[tauri::command]
pub async fn delete_source(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    let pool = &state.db;

    // 先按 source_ref 前缀清理本次导入的文档与分块
    let prefix = format!("{}::", id);
    let ids: Vec<String> = sqlx::query_scalar(
        "SELECT id FROM kb_documents WHERE source_ref LIKE ? ESCAPE '\\'",
    )
    .bind(format!("{}%", prefix))
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?;
    for doc_id in ids {
        crate::rag::store::purge_document_chunks(pool, &doc_id)
            .await
            .map_err(|e| e.to_string())?;
    }
    sqlx::query("DELETE FROM kb_documents WHERE source_ref LIKE ? ESCAPE '\\'")
        .bind(format!("{}%", prefix))
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;

    let res = sqlx::query("DELETE FROM kb_sources WHERE id = ?")
        .bind(&id)
        .execute(pool)
        .await
        .map_err(|e| e.to_string())?;
    if res.rows_affected() == 0 {
        return Err(format!("来源不存在: {}", id));
    }
    Ok(())
}

/// 检索调试：对单个知识库执行查询，返回 Top-K 最相似分块（含内容与相似度）。
///
/// `mode` 支持 `vector`（默认）/ `keyword` / `hybrid`，语义同 `ask_kb`；
/// `keyword_weight` 仅在混合模式生效，缺省 0.3。
///
/// 关键：只有向量 / 混合模式才把查询词向量化。关键词模式走 FTS5，必须保持
/// 纯本地——若此处无条件调嵌入，嵌入渠道不可用时会让本可离线完成的关键词
/// 检索一起失败（功能倒退）。`top_k` 缺省 5。
#[tauri::command]
pub async fn retrieve_kb(
    state: State<'_, Arc<AppState>>,
    kb_id: String,
    query: String,
    top_k: Option<i64>,
    mode: Option<String>,
    keyword_weight: Option<f32>,
) -> Result<Vec<RetrievalHit>, String> {
    let pool = &state.db;
    let q = query.trim().to_string();
    if q.is_empty() {
        return Err("查询内容不能为空".to_string());
    }

    let mode = RetrievalMode::from_str_opt(mode.as_deref());
    // 与问答命令保持同一默认权重口径。
    let kw = keyword_weight.unwrap_or(0.3).clamp(0.0, 1.0);

    let kb = crate::rag::store::get_kb(pool, &kb_id)
        .await
        .map_err(|e| e.to_string())?;

    let q_vec: Vec<f32> = if matches!(mode, RetrievalMode::Vector | RetrievalMode::Hybrid) {
        crate::rag::embed::embed_texts(
            pool,
            &kb.embedding_channel_id,
            &kb.embedding_model,
            vec![q.clone()],
        )
        .await
        .map_err(|e| e.to_string())?
        .0
        .into_iter()
        .next()
        .ok_or_else(|| "嵌入结果为空".to_string())?
    } else {
        Vec::new()
    };

    let k = top_k.unwrap_or(5).max(1) as usize;
    let hits = crate::rag::retrieve::retrieve(pool, &[kb_id.clone()], &q, &q_vec, k, mode, kw)
        .await
        .map_err(|e| e.to_string())?;

    Ok(hits
        .into_iter()
        .map(|h| RetrievalHit {
            doc_id: h.doc_id,
            doc_title: h.doc_title,
            content: h.content,
            score: h.score,
        })
        .collect())
}

/// 索引状态（对齐前端 `IndexStatus`）。
///
/// - `embedded_count`：已向量化（embedding 非空且非 `[]`）的分块数；
/// - `stale_count`：分块记录的嵌入模型与知识库当前 `embedding_model` 不一致的分块数
///   （改了嵌入模型后旧分块即 stale，需要重建索引）；
/// - `is_complete`：全部分块都已向量化；
/// - `is_stale`：存在 stale 分块。
#[derive(Debug, Serialize)]
pub struct IndexStatus {
    pub doc_count: i64,
    pub chunk_count: i64,
    pub embedded_count: i64,
    pub stale_count: i64,
    /// 全部分块的 token 总数（来自上游嵌入响应的 prompt_tokens 汇总）。
    pub total_tokens: i64,
    /// 知识库当前绑定的嵌入模型（判定 stale 的基准）。
    pub embedding_model: String,
    pub is_complete: bool,
    pub is_stale: bool,
}

/// 计算索引状态（命令与重建索引共用，避免重复 SQL）。
async fn compute_index_status(pool: &sqlx::SqlitePool, kb_id: &str) -> Result<IndexStatus, String> {
    let kb = crate::rag::store::get_kb(pool, kb_id)
        .await
        .map_err(|e| e.to_string())?;
    let stats = sqlx::query(
        "SELECT \
            (SELECT COUNT(*) FROM kb_documents WHERE kb_id = ?) AS doc_count, \
            (SELECT COUNT(*) FROM kb_chunks WHERE kb_id = ?) AS chunk_count, \
            (SELECT COUNT(*) FROM kb_chunks \
                WHERE kb_id = ? AND embedding IS NOT NULL \
                  AND embedding <> '' AND embedding <> '[]') AS embedded_count, \
            (SELECT COUNT(*) FROM kb_chunks \
                WHERE kb_id = ? AND (embedding_model IS NULL OR embedding_model <> ?)) AS stale_count, \
            (SELECT COALESCE(SUM(token_count), 0) FROM kb_documents WHERE kb_id = ?) AS total_tokens",
    )
    .bind(kb_id)
    .bind(kb_id)
    .bind(kb_id)
    .bind(kb_id)
    .bind(&kb.embedding_model)
    .bind(kb_id)
    .fetch_one(pool)
    .await
    .map_err(|e| e.to_string())?;

    let doc_count: i64 = stats.try_get("doc_count").unwrap_or(0);
    let chunk_count: i64 = stats.try_get("chunk_count").unwrap_or(0);
    let embedded_count: i64 = stats.try_get("embedded_count").unwrap_or(0);
    let stale_count: i64 = stats.try_get("stale_count").unwrap_or(0);
    let total_tokens: i64 = stats.try_get("total_tokens").unwrap_or(0);

    Ok(IndexStatus {
        doc_count,
        chunk_count,
        embedded_count,
        stale_count,
        total_tokens,
        embedding_model: kb.embedding_model,
        is_complete: chunk_count > 0 && embedded_count == chunk_count,
        is_stale: stale_count > 0,
    })
}

/// 查询索引状态：文档数 / 分块数 / 已向量化数 / stale 数 / 是否完整。
#[tauri::command]
pub async fn get_index_status(
    state: State<'_, Arc<AppState>>,
    kb_id: String,
) -> Result<IndexStatus, String> {
    compute_index_status(&state.db, &kb_id).await
}

/// 重建索引：按知识库「当前」嵌入模型，重新向量化全部分块并写回
/// （embedding + embedding_model），用于切换嵌入模型后的存量刷新。
///
/// 按 `embedding_batch_size`（缺省 16）分批调用嵌入接口，避免一次性把全文
/// 堆进内存。处理中阻塞该命令，前端以 Spinner 等待；本地单用户量下可接受。
#[tauri::command]
pub async fn reindex_kb(
    state: State<'_, Arc<AppState>>,
    kb_id: String,
) -> Result<IndexStatus, String> {
    let pool = &state.db;
    let kb = crate::rag::store::get_kb(pool, &kb_id)
        .await
        .map_err(|e| e.to_string())?;

    // 读全部分块（无论是否已向量化，都按当前模型重嵌）
    let rows: Vec<(String, String)> = sqlx::query(
        "SELECT id, content FROM kb_chunks WHERE kb_id = ? ORDER BY seq ASC",
    )
    .bind(&kb_id)
    .fetch_all(pool)
    .await
    .map_err(|e| e.to_string())?
    .into_iter()
    .map(|r: sqlx::sqlite::SqliteRow| {
        let id: String = r.try_get("id").unwrap_or_default();
        let content: String = r.try_get("content").unwrap_or_default();
        (id, content)
    })
    .collect();

    if rows.is_empty() {
        return compute_index_status(pool, &kb_id).await;
    }

    let batch = kb.embedding_batch_size.filter(|&b| b > 0).unwrap_or(16) as usize;

    for chunk in rows.chunks(batch) {
        let contents: Vec<String> = chunk.iter().map(|(_, c)| c.clone()).collect();
        let (vecs, _tokens) = crate::rag::embed::embed_texts(
            pool,
            &kb.embedding_channel_id,
            &kb.embedding_model,
            contents,
        )
        .await
        .map_err(|e| e.to_string())?;
        if vecs.len() != chunk.len() {
            return Err("重建索引时嵌入返回的向量数量与分块数量不一致".to_string());
        }
        for ((id, _), emb) in chunk.iter().zip(vecs) {
            let emb_json =
                serde_json::to_string(&emb).map_err(|e| e.to_string())?;
            sqlx::query(
                "UPDATE kb_chunks SET embedding = ?, embedding_model = ? WHERE id = ?",
            )
            .bind(emb_json)
            .bind(&kb.embedding_model)
            .bind(id)
            .execute(pool)
            .await
            .map_err(|e| e.to_string())?;
        }
    }

    compute_index_status(pool, &kb_id).await
}
