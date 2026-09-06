//! Wiki 知识库管理命令（对应前端 `wikiApi`）。
//!
//! 命令名与参数键严格对齐 `src/lib/api.ts` 的 `wikiApi`：
//! - `list_wiki_projects`    → `wikiApi.list`
//! - `create_wiki_project`   → `wikiApi.create`
//! - `update_wiki_project`   → `wikiApi.update`
//! - `delete_wiki_project`   → `wikiApi.remove`
//! - `list_wiki_pages`       → `wikiApi.pages`
//! - `list_wiki_sources`     → `wikiApi.sources`
//! - `add_wiki_source`       → `wikiApi.addSource`
//! - `delete_wiki_source`    → `wikiApi.removeSource`
//! - `ingest_wiki_source`    → `wikiApi.ingestSource`
//! - `ask_wiki`              → `wikiApi.ask`
//!
//! 返回结构字段名对齐 `src/types/index.ts` 的 `Wiki*`（Tauri 返回值原样透传）。

use serde::Deserialize;
use std::sync::Arc;

use sqlx::SqlitePool;
use tauri::State;

use crate::wiki::ask;
use crate::wiki::ingest;
use crate::wiki::store::{self, WikiAskResult, WikiPage, WikiProject, WikiProjectBase, WikiSource};
use crate::AppState;

// ---------------------------------------------------------------------------
// 输入结构（对齐前端 WikiProjectInput / WikiProjectUpdate / WikiSourceInput）
// ---------------------------------------------------------------------------

#[derive(Debug, Deserialize)]
pub struct WikiProjectInput {
    pub name: String,
    pub description: String,
    pub channel_id: String,
    pub model: String,
    pub mcp_exposed: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct WikiProjectUpdate {
    pub name: Option<String>,
    pub description: Option<String>,
    pub status: Option<i64>,
    pub channel_id: Option<String>,
    pub model: Option<String>,
    pub maintenance_prompt: Option<String>,
    pub chat_channel_id: Option<String>,
    pub chat_model: Option<String>,
    pub mcp_exposed: Option<i64>,
}

#[derive(Debug, Deserialize)]
pub struct WikiSourceInput {
    pub kind: String,
    pub locator: String,
    pub branch: Option<String>,
}

// ---------------------------------------------------------------------------
// 命令
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn list_wiki_projects(
    state: State<'_, Arc<AppState>>,
) -> Result<Vec<WikiProject>, String> {
    store::list_projects(&state.db)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn create_wiki_project(
    state: State<'_, Arc<AppState>>,
    input: WikiProjectInput,
) -> Result<WikiProject, String> {
    let ts = store::now();
    let base = WikiProjectBase {
        id: store::new_id(),
        name: input.name,
        description: input.description,
        channel_id: input.channel_id,
        model: input.model,
        maintenance_prompt: String::new(),
        chat_channel_id: String::new(),
        chat_model: String::new(),
        mcp_exposed: input.mcp_exposed.unwrap_or(0),
        status: 1,
        created_at: ts.clone(),
        updated_at: ts,
    };
    store::insert_project(&state.db, &base)
        .await
        .map_err(|e| e.to_string())?;
    store::get_project(&state.db, &base.id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn update_wiki_project(
    state: State<'_, Arc<AppState>>,
    id: String,
    patch: WikiProjectUpdate,
) -> Result<WikiProject, String> {
    let pool: &SqlitePool = &state.db;
    let mut base = store::get_project_base(pool, &id)
        .await
        .map_err(|e| e.to_string())?;

    if let Some(v) = patch.name {
        base.name = v;
    }
    if let Some(v) = patch.description {
        base.description = v;
    }
    if let Some(v) = patch.status {
        base.status = v;
    }
    if let Some(v) = patch.channel_id {
        base.channel_id = v;
    }
    if let Some(v) = patch.model {
        base.model = v;
    }
    if let Some(v) = patch.maintenance_prompt {
        base.maintenance_prompt = v;
    }
    if let Some(v) = patch.chat_channel_id {
        base.chat_channel_id = v;
    }
    if let Some(v) = patch.chat_model {
        base.chat_model = v;
    }
    if let Some(v) = patch.mcp_exposed {
        base.mcp_exposed = v;
    }
    base.updated_at = store::now();

    store::update_project(pool, &base)
        .await
        .map_err(|e| e.to_string())?;
    store::get_project(pool, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_wiki_project(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<(), String> {
    store::delete_project(&state.db, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_wiki_pages(
    state: State<'_, Arc<AppState>>,
    project_id: String,
) -> Result<Vec<WikiPage>, String> {
    store::list_pages(&state.db, &project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn list_wiki_sources(
    state: State<'_, Arc<AppState>>,
    project_id: String,
) -> Result<Vec<WikiSource>, String> {
    store::list_sources(&state.db, &project_id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn add_wiki_source(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    input: WikiSourceInput,
) -> Result<WikiSource, String> {
    store::create_source(
        &state.db,
        &project_id,
        &input.kind,
        &input.locator,
        input.branch,
    )
    .await
    .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn delete_wiki_source(state: State<'_, Arc<AppState>>, id: String) -> Result<(), String> {
    store::delete_source(&state.db, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ingest_wiki_source(
    state: State<'_, Arc<AppState>>,
    id: String,
) -> Result<WikiSource, String> {
    ingest::ingest_source(&state.db, &id)
        .await
        .map_err(|e| e.to_string())
}

#[tauri::command]
pub async fn ask_wiki(
    state: State<'_, Arc<AppState>>,
    project_id: String,
    question: String,
) -> Result<WikiAskResult, String> {
    ask::ask(&state.db, &project_id, &question)
        .await
        .map_err(|e| e.to_string())
}
