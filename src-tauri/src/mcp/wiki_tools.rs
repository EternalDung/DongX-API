//! Wiki MCP 工具：把 `crate::wiki` 的检索 / 问答 / 读取能力包装成 MCP tools。
//!
//! 与 [`super::tools`]（RAG 工具）同构：所有按 project 取数据的入口都强制
//! `mcp_exposed = 1` 过滤，否则 UI 上的「MCP 暴露」开关会被绕过。
//!
//! **当前只暴露只读能力**（检索 / 问答 / 读取）。写入类（建项目、存页面、
//! 删页面）暂不开放——让外部 agent 直接改写本地 Wiki，风险高于收益。
//!
//! 注意：Wiki 页面按 `slug` 寻址（不是 waliapi 那种 `path`），
//! 这是 DongX 的 `wiki_pages` 表自身的主键语义。

use serde::Deserialize;
use serde_json::{json, Value};
use sqlx::SqlitePool;

use super::protocol::{JsonRpcError, ERR_MCP_WIKI_NOT_EXPOSED, ERR_MCP_WIKI_NOT_FOUND};
use super::tools::{McpToolSpec, ToolCallResult};
use crate::wiki::store::{self as wiki_store, WikiPage, WikiProject};

/// Wiki 工具的静态元数据，由 [`super::tools::tool_specs`] 合并进 `tools/list`。
pub fn specs() -> Vec<McpToolSpec> {
    vec![
        McpToolSpec {
            name: "list_wiki_projects",
            description: "列出所有已开启 MCP 暴露的 Wiki 项目（ID / 名称 / 页面数 / 源数 / 链接数）",
            input_schema: json!({
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false
            }),
        },
        McpToolSpec {
            name: "get_wiki_project",
            description: "获取 Wiki 项目详情：描述、页面/源/链接统计、最近摄入时间。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {"type": "string", "description": "Wiki 项目 ID"}
                },
                "required": ["project_id"],
                "additionalProperties": false
            }),
        },
        McpToolSpec {
            name: "list_wiki_pages",
            description: "列出 Wiki 项目的所有页面（标题 / slug / 分类 / wikilink）。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {"type": "string", "description": "Wiki 项目 ID"}
                },
                "required": ["project_id"],
                "additionalProperties": false
            }),
        },
        McpToolSpec {
            name: "get_wiki_page",
            description: "读取指定 Wiki 页面的完整 Markdown 正文。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {"type": "string", "description": "Wiki 项目 ID"},
                    "slug": {"type": "string", "description": "页面 slug（如 'index' 或 'guides/setup'），可由 list_wiki_pages 获取"}
                },
                "required": ["project_id", "slug"],
                "additionalProperties": false
            }),
        },
        McpToolSpec {
            name: "search_wiki",
            description: "在 Wiki 页面内做关键词检索，返回命中页面与上下文片段（标题/slug/正文加权打分）。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {"type": "string", "description": "Wiki 项目 ID"},
                    "query": {"type": "string", "description": "检索关键词（空格分隔多词）"},
                    "top_k": {"type": "integer", "description": "返回条数（默认 10，范围 1-30）", "default": 10, "minimum": 1, "maximum": 30}
                },
                "required": ["project_id", "query"],
                "additionalProperties": false
            }),
        },
        McpToolSpec {
            name: "ask_wiki",
            description: "向 Wiki 提问：检索相关页面 → LLM 生成回答 → 返回回答 + 来源引用。回答模型取项目自身配置的 chat_model，无需传入。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {"type": "string", "description": "Wiki 项目 ID"},
                    "question": {"type": "string", "description": "问题"}
                },
                "required": ["project_id", "question"],
                "additionalProperties": false
            }),
        },
        McpToolSpec {
            name: "list_wiki_sources",
            description: "列出 Wiki 项目的源资料及其摄入状态。",
            input_schema: json!({
                "type": "object",
                "properties": {
                    "project_id": {"type": "string", "description": "Wiki 项目 ID"}
                },
                "required": ["project_id"],
                "additionalProperties": false
            }),
        },
    ]
}

/// 工具分发：RAG 工具未命中时由 [`super::tools::dispatch`] 兜底调到这里。
pub async fn dispatch_wiki(pool: &SqlitePool, name: &str, arguments: Value) -> ToolCallResult {
    match name {
        "list_wiki_projects" => list_wiki_projects(pool).await,
        "get_wiki_project" => get_wiki_project(pool, arguments).await,
        "list_wiki_pages" => list_wiki_pages(pool, arguments).await,
        "get_wiki_page" => get_wiki_page(pool, arguments).await,
        "search_wiki" => search_wiki(pool, arguments).await,
        "ask_wiki" => ask_wiki(pool, arguments).await,
        "list_wiki_sources" => list_wiki_sources(pool, arguments).await,
        other => ToolCallResult::err(format!("未知工具: {}", other)),
    }
}

// -----------------------------------------------------------------------------
// 公共前置校验
// -----------------------------------------------------------------------------

/// 校验 Wiki 项目存在 + 已开启 MCP 暴露；返回项目行供上层取统计信息。
async fn require_exposed_project(
    pool: &SqlitePool,
    project_id: &str,
) -> Result<WikiProject, JsonRpcError> {
    if project_id.is_empty() {
        return Err(JsonRpcError {
            code: ERR_MCP_WIKI_NOT_FOUND,
            message: "project_id 不能为空".into(),
            data: None,
        });
    }

    // 任意错误（含不存在）一律按「不存在」回，避免把内部 DB 细节泄给 MCP client。
    let proj = match wiki_store::get_project(pool, project_id).await {
        Ok(p) => p,
        Err(_) => {
            return Err(JsonRpcError {
                code: ERR_MCP_WIKI_NOT_FOUND,
                message: format!("Wiki 项目不存在: {}", project_id),
                data: None,
            })
        }
    };

    if proj.mcp_exposed != 1 {
        return Err(JsonRpcError {
            code: ERR_MCP_WIKI_NOT_EXPOSED,
            message: format!("Wiki 项目「{}」未开启 MCP 暴露", proj.name),
            data: Some(json!({ "project_id": project_id, "name": proj.name })),
        });
    }
    Ok(proj)
}

// -----------------------------------------------------------------------------
// list_wiki_projects
// -----------------------------------------------------------------------------

async fn list_wiki_projects(pool: &SqlitePool) -> ToolCallResult {
    let rows = match wiki_store::list_projects(pool).await {
        Ok(r) => r,
        Err(e) => return ToolCallResult::err(format!("查询 Wiki 项目失败: {}", e)),
    };

    // 与 RAG 同构：只暴露「MCP 暴露」且启用的项目。
    let exposed: Vec<WikiProject> = rows
        .into_iter()
        .filter(|p| p.mcp_exposed == 1 && p.status == 1)
        .collect();

    if exposed.is_empty() {
        return ToolCallResult::text("当前没有已开启 MCP 暴露的启用 Wiki 项目。");
    }

    let mut s = String::from(
        "已暴露给 MCP 的 Wiki 项目：\n\n| ID | 名称 | 页面 | 源 | 链接 | 描述 |\n|---|---|---|---|---|---|\n",
    );
    for p in exposed {
        s.push_str(&format!(
            "| `{id}` | {name} | {pages} | {sources} | {links} | {desc} |\n",
            id = p.id,
            name = p.name,
            pages = p.page_count,
            sources = p.source_count,
            links = p.link_count,
            desc = p.description,
        ));
    }
    ToolCallResult::text(s)
}

// -----------------------------------------------------------------------------
// get_wiki_project
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
struct ProjectArgs {
    project_id: String,
}

async fn get_wiki_project(pool: &SqlitePool, args: Value) -> ToolCallResult {
    let args: ProjectArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return ToolCallResult::err(format!("参数非法: {}", e)),
    };

    let p = match require_exposed_project(pool, &args.project_id).await {
        Ok(p) => p,
        Err(e) => return super::tools::tool_err(e),
    };

    let s = format!(
        "## Wiki 项目「{name}」\n\n\
         - ID: `{id}`\n\
         - 描述: {desc}\n\
         - 页面数: {pages}\n\
         - 源资料数: {sources}\n\
         - wikilink 数: {links}\n\
         - 预估 token: {tokens}\n\
         - 摄入模型: {model}\n\
         - 问答模型: {chat_model}\n\
         - 最近摄入: {last}\n",
        name = p.name,
        id = p.id,
        desc = p.description,
        pages = p.page_count,
        sources = p.source_count,
        links = p.link_count,
        tokens = p.token_estimate,
        model = p.model,
        chat_model = p.chat_model,
        last = p.last_ingest_at.unwrap_or_else(|| "从未".into()),
    );
    ToolCallResult::text(s)
}

// -----------------------------------------------------------------------------
// list_wiki_pages
// -----------------------------------------------------------------------------

async fn list_wiki_pages(pool: &SqlitePool, args: Value) -> ToolCallResult {
    let args: ProjectArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return ToolCallResult::err(format!("参数非法: {}", e)),
    };

    let proj = match require_exposed_project(pool, &args.project_id).await {
        Ok(p) => p,
        Err(e) => return super::tools::tool_err(e),
    };

    let pages = match wiki_store::list_pages(pool, &args.project_id).await {
        Ok(p) => p,
        Err(e) => return ToolCallResult::err(format!("查询页面失败: {}", e)),
    };

    if pages.is_empty() {
        return ToolCallResult::text(format!("Wiki 项目「{}」还没有任何页面。", proj.name));
    }

    let mut s = format!(
        "Wiki 项目「{name}」共 {n} 个页面：\n\n| 标题 | slug | 分类 | wikilink |\n|---|---|---|---|\n",
        name = proj.name,
        n = pages.len()
    );
    for p in &pages {
        s.push_str(&format!(
            "| {title} | `{slug}` | {kind} | {links} |\n",
            title = p.title,
            slug = p.slug,
            kind = p.kind,
            links = if p.links.is_empty() {
                "—".to_string()
            } else {
                p.links.join(", ")
            },
        ));
    }
    ToolCallResult::text(s)
}

// -----------------------------------------------------------------------------
// get_wiki_page
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
struct PageArgs {
    project_id: String,
    slug: String,
}

async fn get_wiki_page(pool: &SqlitePool, args: Value) -> ToolCallResult {
    let args: PageArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return ToolCallResult::err(format!("参数非法: {}", e)),
    };

    if let Err(e) = require_exposed_project(pool, &args.project_id).await {
        return super::tools::tool_err(e);
    }

    let pages = match wiki_store::list_pages(pool, &args.project_id).await {
        Ok(p) => p,
        Err(e) => return ToolCallResult::err(format!("查询页面失败: {}", e)),
    };

    // slug 精确匹配；找不到时给出「可用 slug」提示，避免 agent 盲猜。
    match pages.into_iter().find(|p| p.slug == args.slug) {
        Some(p) => {
            let mut s = format!(
                "# {title}\n\n- slug: `{slug}`\n- 分类: {kind}\n- token: {tokens}\n- 更新于: {updated}\n",
                title = p.title,
                slug = p.slug,
                kind = p.kind,
                tokens = p.tokens,
                updated = p.updated_at,
            );
            if !p.links.is_empty() {
                s.push_str(&format!("- wikilink: {}\n", p.links.join(", ")));
            }
            s.push_str(&format!("\n---\n\n{}\n", p.content));
            ToolCallResult::text(s)
        }
        None => ToolCallResult::err(format!(
            "页面不存在: slug = `{}`（可用 list_wiki_pages 查看该项目的全部 slug）",
            args.slug
        )),
    }
}

// -----------------------------------------------------------------------------
// search_wiki
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
struct SearchWikiArgs {
    project_id: String,
    query: String,
    #[serde(default)]
    top_k: Option<usize>,
}

/// 标题权重 4 / slug 权重 3 / 正文权重 1，多词累加。
fn score_page(page: &WikiPage, terms: &[String]) -> usize {
    let title = page.title.to_lowercase();
    let slug = page.slug.to_lowercase();
    let content = page.content.to_lowercase();
    let mut score = 0usize;
    for t in terms {
        if t.is_empty() {
            continue;
        }
        score += title.matches(t.as_str()).count() * 4;
        score += slug.matches(t.as_str()).count() * 3;
        score += content.matches(t.as_str()).count();
    }
    score
}

/// 截取首个命中位置附近的正文片段。按 **char** 下标切片，避免中文被截成乱码。
fn snippet(content: &str, terms: &[String], max: usize) -> String {
    let chars: Vec<char> = content.chars().collect();
    let lower = content.to_lowercase();

    let mut best: Option<usize> = None;
    for t in terms {
        if t.is_empty() {
            continue;
        }
        if let Some(byte_pos) = lower.find(t.as_str()) {
            let char_pos = lower[..byte_pos].chars().count();
            best = Some(match best {
                Some(p) => p.min(char_pos),
                None => char_pos,
            });
        }
    }

    let start = best.unwrap_or(0).saturating_sub(60).min(chars.len());
    let end = (start + max).min(chars.len());
    let body: String = chars[start..end].iter().collect();

    let mut s = String::new();
    if start > 0 {
        s.push('…');
    }
    s.push_str(&body);
    if end < chars.len() {
        s.push('…');
    }
    s
}

async fn search_wiki(pool: &SqlitePool, args: Value) -> ToolCallResult {
    let args: SearchWikiArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return ToolCallResult::err(format!("参数非法: {}", e)),
    };

    let proj = match require_exposed_project(pool, &args.project_id).await {
        Ok(p) => p,
        Err(e) => return super::tools::tool_err(e),
    };

    let top_k = args.top_k.unwrap_or(10).clamp(1, 30);
    let terms: Vec<String> = args
        .query
        .to_lowercase()
        .split_whitespace()
        .map(|s| s.to_string())
        .collect();

    if terms.is_empty() {
        return ToolCallResult::err("query 不能为空");
    }

    let pages = match wiki_store::list_pages(pool, &args.project_id).await {
        Ok(p) => p,
        Err(e) => return ToolCallResult::err(format!("查询页面失败: {}", e)),
    };

    let mut hits: Vec<(usize, &WikiPage)> = pages
        .iter()
        .map(|p| (score_page(p, &terms), p))
        .filter(|(score, _)| *score > 0)
        .collect();
    hits.sort_by_key(|a| std::cmp::Reverse(a.0));
    hits.truncate(top_k);

    if hits.is_empty() {
        return ToolCallResult::text(format!(
            "Wiki 项目「{}」中没有匹配「{}」的页面。",
            proj.name, args.query
        ));
    }

    let mut s = format!(
        "Wiki 项目「{name}」中匹配「{q}」的页面 Top-{n}：\n",
        name = proj.name,
        q = args.query,
        n = hits.len()
    );
    for (i, (score, p)) in hits.iter().enumerate() {
        s.push_str(&format!(
            "\n[{i}] {title}（slug: `{slug}`，得分 {score}）\n{snippet}\n",
            i = i + 1,
            title = p.title,
            slug = p.slug,
            score = score,
            snippet = snippet(&p.content, &terms, 320),
        ));
    }
    ToolCallResult::text(s)
}

// -----------------------------------------------------------------------------
// ask_wiki
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
struct AskWikiArgs {
    project_id: String,
    question: String,
}

async fn ask_wiki(pool: &SqlitePool, args: Value) -> ToolCallResult {
    let args: AskWikiArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return ToolCallResult::err(format!("参数非法: {}", e)),
    };

    if let Err(e) = require_exposed_project(pool, &args.project_id).await {
        return super::tools::tool_err(e);
    }

    match crate::wiki::ask::ask(pool, &args.project_id, &args.question).await {
        Ok(res) => {
            let mut s = String::from("## 回答\n\n");
            s.push_str(&res.answer);
            if !res.citations.is_empty() {
                s.push_str("\n\n## 来源\n\n");
                for (i, c) in res.citations.iter().enumerate() {
                    s.push_str(&format!(
                        "[{i}] {title}（slug: `{slug}`）\n{excerpt}\n\n",
                        i = i + 1,
                        title = c.title,
                        slug = c.slug,
                        excerpt = c.excerpt,
                    ));
                }
            }
            ToolCallResult::text(s)
        }
        Err(e) => ToolCallResult::err(format!("问答失败: {}", e)),
    }
}

// -----------------------------------------------------------------------------
// list_wiki_sources
// -----------------------------------------------------------------------------

async fn list_wiki_sources(pool: &SqlitePool, args: Value) -> ToolCallResult {
    let args: ProjectArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return ToolCallResult::err(format!("参数非法: {}", e)),
    };

    let proj = match require_exposed_project(pool, &args.project_id).await {
        Ok(p) => p,
        Err(e) => return super::tools::tool_err(e),
    };

    let sources = match wiki_store::list_sources(pool, &args.project_id).await {
        Ok(s) => s,
        Err(e) => return ToolCallResult::err(format!("查询源资料失败: {}", e)),
    };

    if sources.is_empty() {
        return ToolCallResult::text(format!("Wiki 项目「{}」还没有任何源资料。", proj.name));
    }

    let mut s = format!(
        "Wiki 项目「{name}」的源资料：\n\n| ID | 类型 | 位置 | 状态 | 进度 | 最近摄入 |\n|---|---|---|---|---|---|\n",
        name = proj.name
    );
    for src in sources {
        let progress = if src.total > 0 {
            format!("{}/{}", src.ingested, src.total)
        } else {
            "—".to_string()
        };
        s.push_str(&format!(
            "| `{id}` | {kind} | {locator} | {status} | {progress} | {last} |\n",
            id = src.id,
            kind = src.kind,
            locator = src.locator,
            status = src.status,
            progress = progress,
            last = src.last_ingest_at.unwrap_or_else(|| "—".into()),
        ));
    }
    ToolCallResult::text(s)
}
