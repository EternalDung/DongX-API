//! Wiki 持久化：项目 / 来源 / 页面的读写与聚合统计。
//!
//! 聚合字段（`source_count` / `page_count` / `link_count` / `token_estimate` /
//! `last_ingest_at`）在读取时通过 LEFT JOIN 子查询实时算出，不落冗余列。

use serde::Serialize;
use sqlx::SqlitePool;

use crate::error::{AppError, AppResult};

/// RFC3339 时间戳（与库内其它模块一致）。
pub fn now() -> String {
    chrono::Utc::now().to_rfc3339()
}

/// UUID v4 主键（与库内其它模块一致）。
pub fn new_id() -> String {
    uuid::Uuid::new_v4().to_string()
}

/// 聚合查询的列清单（项目 + 派生统计），被 list / get 共用。
const PROJECT_SELECT: &str = "SELECT wp.id, wp.name, wp.description, wp.channel_id, wp.model,
                wp.maintenance_prompt, wp.chat_channel_id, wp.chat_model,
                wp.mcp_exposed, wp.status, wp.created_at, wp.updated_at,
                COALESCE(s.cnt, 0) AS source_count,
                COALESCE(p.cnt, 0) AS page_count,
                COALESCE(l.links, 0) AS link_count,
                COALESCE(t.tokens, 0) AS token_estimate,
                (SELECT MAX(updated_at) FROM wiki_sources WHERE project_id = wp.id AND status = 'ready') AS last_ingest_at
         FROM wiki_projects wp
         LEFT JOIN (SELECT project_id, COUNT(*) AS cnt FROM wiki_sources GROUP BY project_id) s ON s.project_id = wp.id
         LEFT JOIN (SELECT project_id, COUNT(*) AS cnt FROM wiki_pages GROUP BY project_id) p ON p.project_id = wp.id
         LEFT JOIN (SELECT project_id, SUM(json_array_length(links)) AS links FROM wiki_pages GROUP BY project_id) l ON l.project_id = wp.id
         LEFT JOIN (SELECT project_id, SUM(tokens) AS tokens FROM wiki_pages GROUP BY project_id) t ON t.project_id = wp.id";

// ===========================================================================
// 响应结构（字段名需与 src/types/index.ts 的 Wiki* 完全一致；Tauri 返回值
// 字段名原样透传，不做 camelCase 转换，故此处用蛇形命名对齐 TS）
// ===========================================================================

/// Wiki 项目（含派生统计）。
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct WikiProject {
    pub id: String,
    pub name: String,
    pub description: String,
    pub channel_id: String,
    pub model: String,
    pub maintenance_prompt: String,
    pub chat_channel_id: String,
    pub chat_model: String,
    pub mcp_exposed: i64,
    pub status: i64,
    pub source_count: i64,
    pub page_count: i64,
    pub link_count: i64,
    pub token_estimate: i64,
    pub last_ingest_at: Option<String>,
    pub created_at: String,
    pub updated_at: String,
}

/// Wiki 项目真实列（用于更新，不含派生字段）。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WikiProjectBase {
    pub id: String,
    pub name: String,
    pub description: String,
    pub channel_id: String,
    pub model: String,
    pub maintenance_prompt: String,
    pub chat_channel_id: String,
    pub chat_model: String,
    pub mcp_exposed: i64,
    pub status: i64,
    pub created_at: String,
    pub updated_at: String,
}

/// Wiki 来源。
#[derive(Debug, Clone, sqlx::FromRow, Serialize)]
pub struct WikiSource {
    pub id: String,
    pub project_id: String,
    pub kind: String,
    pub locator: String,
    pub branch: Option<String>,
    pub status: String,
    pub ingested: i64,
    pub total: i64,
    pub error: Option<String>,
    pub last_ingest_at: Option<String>,
    pub created_at: String,
}

/// 页面原始行（links 以 JSON 字符串落库）。
#[derive(Debug, Clone, sqlx::FromRow)]
pub struct WikiPageRow {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub slug: String,
    pub content: String,
    pub is_index: i64,
    pub kind: String,
    pub links: String,
    pub tokens: i64,
    pub updated_at: String,
    pub created_at: String,
}

/// 页面响应（links 解析为字符串数组，对齐前端 WikiPage）。
#[derive(Debug, Clone, Serialize)]
pub struct WikiPage {
    pub id: String,
    pub project_id: String,
    pub title: String,
    pub slug: String,
    pub content: String,
    pub is_index: bool,
    pub kind: String,
    pub links: Vec<String>,
    pub tokens: i64,
    pub updated_at: String,
    pub created_at: String,
}

/// Wiki 问答引用片段。
#[derive(Debug, Clone, Serialize)]
pub struct WikiCitation {
    pub title: String,
    pub slug: String,
    pub excerpt: String,
}

/// Wiki 问答结果。
#[derive(Debug, Clone, Serialize)]
pub struct WikiAskResult {
    pub answer: String,
    pub citations: Vec<WikiCitation>,
    pub prompt_tokens: i64,
    pub completion_tokens: i64,
    pub duration_ms: i64,
}

// ===========================================================================
// 项目
// ===========================================================================

pub async fn list_projects(pool: &SqlitePool) -> AppResult<Vec<WikiProject>> {
    let sql = format!("{PROJECT_SELECT} ORDER BY wp.created_at DESC");
    let rows = sqlx::query_as::<_, WikiProject>(&sql)
        .fetch_all(pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(rows)
}

pub async fn get_project(pool: &SqlitePool, id: &str) -> AppResult<WikiProject> {
    let sql = format!("{PROJECT_SELECT} WHERE wp.id = ? LIMIT 1");
    let row = sqlx::query_as::<_, WikiProject>(&sql)
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Wiki 项目不存在: {id}")))?;
    Ok(row)
}

pub async fn get_project_base(pool: &SqlitePool, id: &str) -> AppResult<WikiProjectBase> {
    let row = sqlx::query_as::<_, WikiProjectBase>("SELECT * FROM wiki_projects WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("Wiki 项目不存在: {id}")))?;
    Ok(row)
}

pub async fn insert_project(pool: &SqlitePool, b: &WikiProjectBase) -> AppResult<()> {
    sqlx::query(
        "INSERT INTO wiki_projects (id, name, description, channel_id, model, maintenance_prompt,
            chat_channel_id, chat_model, mcp_exposed, status, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
    )
    .bind(&b.id)
    .bind(&b.name)
    .bind(&b.description)
    .bind(&b.channel_id)
    .bind(&b.model)
    .bind(&b.maintenance_prompt)
    .bind(&b.chat_channel_id)
    .bind(&b.chat_model)
    .bind(b.mcp_exposed)
    .bind(b.status)
    .bind(&b.created_at)
    .bind(&b.updated_at)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

pub async fn update_project(pool: &SqlitePool, b: &WikiProjectBase) -> AppResult<()> {
    sqlx::query(
        "UPDATE wiki_projects SET name=?, description=?, channel_id=?, model=?, \
            maintenance_prompt=?, chat_channel_id=?, chat_model=?, mcp_exposed=?, status=?, updated_at=? \
         WHERE id = ?",
    )
    .bind(&b.name)
    .bind(&b.description)
    .bind(&b.channel_id)
    .bind(&b.model)
    .bind(&b.maintenance_prompt)
    .bind(&b.chat_channel_id)
    .bind(&b.chat_model)
    .bind(b.mcp_exposed)
    .bind(b.status)
    .bind(&b.updated_at)
    .bind(&b.id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

pub async fn delete_project(pool: &SqlitePool, id: &str) -> AppResult<()> {
    let mut tx = pool
        .begin()
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    sqlx::query("DELETE FROM wiki_pages WHERE project_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    sqlx::query("DELETE FROM wiki_sources WHERE project_id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    sqlx::query("DELETE FROM wiki_projects WHERE id = ?")
        .bind(id)
        .execute(&mut *tx)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    tx.commit()
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

// ===========================================================================
// 来源
// ===========================================================================

pub async fn list_sources(pool: &SqlitePool, project_id: &str) -> AppResult<Vec<WikiSource>> {
    let rows = sqlx::query_as::<_, WikiSource>(
        "SELECT * FROM wiki_sources WHERE project_id = ? ORDER BY created_at DESC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(rows)
}

pub async fn get_source(pool: &SqlitePool, id: &str) -> AppResult<WikiSource> {
    let row = sqlx::query_as::<_, WikiSource>("SELECT * FROM wiki_sources WHERE id = ?")
        .bind(id)
        .fetch_optional(pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?
        .ok_or_else(|| AppError::NotFound(format!("来源不存在: {id}")))?;
    Ok(row)
}

pub async fn create_source(
    pool: &SqlitePool,
    project_id: &str,
    kind: &str,
    locator: &str,
    branch: Option<String>,
) -> AppResult<WikiSource> {
    let ts = now();
    let src = WikiSource {
        id: new_id(),
        project_id: project_id.to_string(),
        kind: kind.to_string(),
        locator: locator.to_string(),
        branch,
        status: "pending".into(),
        ingested: 0,
        total: 0,
        error: None,
        last_ingest_at: None,
        created_at: ts.clone(),
    };
    sqlx::query(
        "INSERT INTO wiki_sources (id, project_id, kind, locator, branch, status, ingested, total, error, last_ingest_at, created_at, updated_at)
         VALUES (?1,?2,?3,?4,?5,'pending',0,0,NULL,NULL,?6,?6)",
    )
    .bind(&src.id)
    .bind(&src.project_id)
    .bind(&src.kind)
    .bind(&src.locator)
    .bind(&src.branch)
    .bind(&ts)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(src)
}

pub async fn update_source_progress(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    ingested: i64,
    total: i64,
    error: Option<String>,
    last_ingest_at: Option<String>,
) -> AppResult<()> {
    sqlx::query(
        "UPDATE wiki_sources SET status=?, ingested=?, total=?, error=?, last_ingest_at=?, updated_at=? WHERE id=?",
    )
    .bind(status)
    .bind(ingested)
    .bind(total)
    .bind(&error)
    .bind(&last_ingest_at)
    .bind(now())
    .bind(id)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

pub async fn delete_source(pool: &SqlitePool, id: &str) -> AppResult<()> {
    delete_pages_by_source(pool, id).await?;
    sqlx::query("DELETE FROM wiki_sources WHERE id = ?")
        .bind(id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

pub async fn delete_pages_by_source(pool: &SqlitePool, source_id: &str) -> AppResult<()> {
    sqlx::query("DELETE FROM wiki_pages WHERE source_id = ?")
        .bind(source_id)
        .execute(pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    Ok(())
}

// ===========================================================================
// 页面
// ===========================================================================

pub async fn list_pages(pool: &SqlitePool, project_id: &str) -> AppResult<Vec<WikiPage>> {
    let rows = sqlx::query_as::<_, WikiPageRow>(
        "SELECT * FROM wiki_pages WHERE project_id = ? ORDER BY is_index DESC, updated_at DESC",
    )
    .bind(project_id)
    .fetch_all(pool)
    .await
    .map_err(|e| AppError::Database(e.to_string()))?;

    let mut out = Vec::with_capacity(rows.len());
    for r in rows {
        let links: Vec<String> = serde_json::from_str(&r.links).unwrap_or_default();
        out.push(WikiPage {
            id: r.id,
            project_id: r.project_id,
            title: r.title,
            slug: r.slug,
            content: r.content,
            is_index: r.is_index != 0,
            kind: r.kind,
            links,
            tokens: r.tokens,
            updated_at: r.updated_at,
            created_at: r.created_at,
        });
    }
    Ok(out)
}

/// 按 (project_id, slug) upsert 页面；存在则更新正文/分类/链接，不存在则插入。
pub async fn upsert_page(
    pool: &SqlitePool,
    project_id: &str,
    source_id: &str,
    title: &str,
    slug: &str,
    content: &str,
    is_index: bool,
    kind: &str,
    links_json: &str,
    tokens: i64,
) -> AppResult<()> {
    let n = sqlx::query(
        "UPDATE wiki_pages SET title=?, content=?, is_index=?, kind=?, links=?, tokens=?, source_id=?, updated_at=? \
         WHERE project_id=? AND slug=?",
    )
    .bind(title)
    .bind(content)
    .bind(is_index as i64)
    .bind(kind)
    .bind(links_json)
    .bind(tokens)
    .bind(source_id)
    .bind(now())
    .bind(project_id)
    .bind(slug)
    .execute(pool)
    .await
    .map_err(|e| AppError::Database(e.to_string()))?
    .rows_affected();

    if n == 0 {
        sqlx::query(
            "INSERT INTO wiki_pages (id, project_id, title, slug, content, is_index, kind, links, tokens, source_id, created_at, updated_at)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?11)",
        )
        .bind(new_id())
        .bind(project_id)
        .bind(title)
        .bind(slug)
        .bind(content)
        .bind(is_index as i64)
        .bind(kind)
        .bind(links_json)
        .bind(tokens)
        .bind(source_id)
        .bind(now())
        .execute(pool)
        .await
        .map_err(|e| AppError::Database(e.to_string()))?;
    }
    Ok(())
}
