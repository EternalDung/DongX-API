//! MCP 工具实现：把 dongx 现有 RAG 能力（`rag::retrieve` / `rag::ask` / 直接 DB 读）
//! 包装成 MCP tools/list 注册的 5 个 tool。
//!
//! 重要：所有跨 KB 的入口都强制 `mcp_exposed = 1` 过滤，
//! 否则 KB 列表的「MCP 暴露」开关会被绕过。

use std::sync::Arc;
use std::sync::OnceLock;

use serde::{Deserialize, Serialize};
use serde_json::{json, Value};
use sqlx::Row;
use sqlx::SqlitePool;

use crate::mcp::protocol::{JsonRpcError, ERR_MCP_KB_NOT_FOUND, ERR_MCP_KB_NOT_EXPOSED};
use crate::rag::models::KnowledgeBaseRow;
use crate::rag::retrieve::{retrieve, RetrievalMode, RetrievedChunk};

/// 一个 MCP 工具的元数据（直接对应 MCP `tools/list` 返回的 `Tool` schema）。
///
/// `rename_all = "camelCase"` 是协议要求：MCP 客户端按 `inputSchema` 读字段，
/// 若发成 `input_schema` 客户端会认为该 tool 无参数约束。
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct McpToolSpec {
    pub name: &'static str,
    pub description: &'static str,
    /// JSON Schema draft-07 风格的 object schema（MCP 协议要求）。
    pub input_schema: Value,
}

/// 全部工具的静态元数据（MCP `tools/list` 直接返）。
///
/// 用 `OnceLock` 延迟初始化：`serde_json::Value` 非 const，构造不能放在静态上下文。
pub fn tool_specs() -> &'static [McpToolSpec] {
    TOOL_SPECS.get_or_init(|| {
        vec![
            McpToolSpec {
                name: "search_knowledge_base",
                description: "语义检索 RAG，返回匹配文本片段和相似度评分",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "kb_id": {"type": "string", "description": "知识库 id（必须已开启「MCP 暴露」）"},
                        "query": {"type": "string", "description": "查询文本"},
                        "top_k": {"type": "integer", "description": "返回条数（默认 5，范围 1-20）", "default": 5, "minimum": 1, "maximum": 20}
                    },
                    "required": ["kb_id", "query"],
                    "additionalProperties": false
                }),
            },
            McpToolSpec {
                name: "list_knowledge_bases",
                description: "列出所有已暴露的 RAG（ID/名称/文档数）",
                input_schema: json!({
                    "type": "object",
                    "properties": {},
                    "required": [],
                    "additionalProperties": false
                }),
            },
            McpToolSpec {
                name: "ask_knowledge_base",
                description: "RAG 问答，基于检索内容生成回答并返回来源引用",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "kb_id": {"type": "string", "description": "知识库 id（必须已开启「MCP 暴露」）"},
                        "question": {"type": "string", "description": "用户问题"},
                        "model": {"type": "string", "description": "用于生成回答的 chat 模型（可省略，使用任意可用模型由网关分发）"}
                    },
                    "required": ["kb_id", "question"],
                    "additionalProperties": false
                }),
            },
            McpToolSpec {
                name: "read_document",
                description: "读取指定文档的完整内容（含分片正文）",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "kb_id": {"type": "string", "description": "知识库 id"},
                        "doc_id": {"type": "string", "description": "文档 id"}
                    },
                    "required": ["kb_id", "doc_id"],
                    "additionalProperties": false
                }),
            },
            McpToolSpec {
                name: "get_knowledge_base_stats",
                description: "获取 RAG 统计信息（文档数 / 切片数 / token 数）",
                input_schema: json!({
                    "type": "object",
                    "properties": {
                        "kb_id": {"type": "string", "description": "知识库 id"}
                    },
                    "required": ["kb_id"],
                    "additionalProperties": false
                }),
            },
        ]
    })
}

static TOOL_SPECS: OnceLock<Vec<McpToolSpec>> = OnceLock::new();

/// MCP `tools/call` 入参（仅 `name` + `arguments`，与协议字段对齐）。
#[derive(Debug, Deserialize)]
pub struct ToolCallArgs {
    pub name: String,
    #[serde(default)]
    pub arguments: Value,
}

/// `rename_all = "camelCase"` 是协议要求：MCP 客户端按 `isError` 判定失败，
/// 若发成 `is_error` 客户端会把失败的工具调用当作成功结果渲染。
#[derive(Debug, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ToolCallResult {
    pub content: Vec<ToolContent>,
    /// `false` 时整字段省略（MCP 规范允许缺省即成功）。
    #[serde(skip_serializing_if = "std::ops::Not::not")]
    pub is_error: bool,
}

#[derive(Debug, Serialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum ToolContent {
    Text { text: String },
}

impl ToolCallResult {
    fn text(s: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: s.into() }],
            is_error: false,
        }
    }
    fn err(s: impl Into<String>) -> Self {
        Self {
            content: vec![ToolContent::Text { text: s.into() }],
            is_error: true,
        }
    }
}

/// 工具分发：按 `name` 路由到 5 个具体实现；未知 name 返回 -32001。
pub async fn dispatch(
    pool: Arc<SqlitePool>,
    name: &str,
    arguments: Value,
) -> ToolCallResult {
    match name {
        "search_knowledge_base" => search_knowledge_base(&pool, arguments).await,
        "list_knowledge_bases" => list_knowledge_bases(&pool).await,
        "ask_knowledge_base" => ask_knowledge_base(&pool, arguments).await,
        "read_document" => read_document(&pool, arguments).await,
        "get_knowledge_base_stats" => get_knowledge_base_stats(&pool, arguments).await,
        other => ToolCallResult::err(format!("未知工具: {}", other)),
    }
}

/// 把 tool 内部错误（含 KB 未暴露）映射成 `ToolCallResult::err`，
/// 避免网络层把内部错误堆栈泄给 MCP client。
fn tool_err(e: JsonRpcError) -> ToolCallResult {
    ToolCallResult::err(format!("[{}] {}", e.code, e.message))
}

// -----------------------------------------------------------------------------
// Tool 1: search_knowledge_base
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
struct SearchArgs {
    kb_id: String,
    query: String,
    #[serde(default)]
    top_k: Option<usize>,
}

async fn search_knowledge_base(pool: &SqlitePool, args: Value) -> ToolCallResult {
    let args: SearchArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return ToolCallResult::err(format!("参数非法: {}", e)),
    };
    let top_k = args.top_k.unwrap_or(5).clamp(1, 20);

    let kb = match require_exposed_kb(pool, &args.kb_id).await {
        Ok(kb) => kb,
        Err(e) => return tool_err(e),
    };

    // 向量化查询（用 KB 绑定的渠道 + 模型）
    let vec = match crate::rag::embed::embed_texts(
        pool,
        &kb.embedding_channel_id,
        &kb.embedding_model,
        vec![args.query.clone()],
    )
    .await
    {
        Ok(v) => v.into_iter().next().unwrap_or_default(),
        Err(e) => {
            return ToolCallResult::err(format!(
                "向量化失败（渠道 {} / 模型 {}）：{}",
                kb.embedding_channel_id, kb.embedding_model, e
            ))
        }
    };
    if vec.is_empty() {
        return ToolCallResult::err("向量化返回空向量");
    }

    let hits = match retrieve(
        pool,
        &[kb.id.clone()],
        &args.query,
        &vec,
        top_k,
        RetrievalMode::Vector,
        0.0,
    )
    .await
    {
        Ok(h) => h,
        Err(e) => return ToolCallResult::err(format!("检索失败: {}", e)),
    };

    if hits.is_empty() {
        return ToolCallResult::text(format!(
            "知识库「{}」中未找到与「{}」相关的分块。",
            kb.name, args.query
        ));
    }

    ToolCallResult::text(format_hits(&kb.name, &hits))
}

fn format_hits(kb_name: &str, hits: &[RetrievedChunk]) -> String {
    let mut s = format!("知识库「{}」Top-{} 命中：\n", kb_name, hits.len());
    for (i, h) in hits.iter().enumerate() {
        s.push_str(&format!(
            "\n[{i}] {title}（相似度 {score:.3}）\n{snippet}\n",
            i = i + 1,
            title = h.doc_title,
            score = h.score,
            snippet = truncate(&h.content, 320),
        ));
    }
    s
}

fn truncate(s: &str, max: usize) -> String {
    let mut out = String::with_capacity(max + 4);
    let mut count = 0;
    for ch in s.chars() {
        if count >= max {
            out.push('…');
            break;
        }
        out.push(ch);
        count += 1;
    }
    out
}

// -----------------------------------------------------------------------------
// Tool 2: list_knowledge_bases
// -----------------------------------------------------------------------------

async fn list_knowledge_bases(pool: &SqlitePool) -> ToolCallResult {
    // `doc_count` / `chunk_count` 是派生统计，`knowledge_bases` 表里没有这两列；
    // 用相关子查询算出（与 `commands/rag.rs::list_knowledge_bases` 同构）。
    let rows = match sqlx::query(
        "SELECT kb.id, kb.name, kb.description, kb.embedding_model,
                (SELECT COUNT(*) FROM kb_documents d
                 WHERE d.kb_id = kb.id AND d.status = 1) AS doc_count,
                (SELECT COUNT(*) FROM kb_chunks c WHERE c.kb_id = kb.id) AS chunk_count
         FROM knowledge_bases kb
         WHERE kb.mcp_exposed = 1 AND kb.status = 1
         ORDER BY kb.name",
    )
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => return ToolCallResult::err(format!("查询知识库失败: {}", e)),
    };

    if rows.is_empty() {
        return ToolCallResult::text("当前没有已开启 MCP 暴露的启用知识库。");
    }

    let mut s = String::from("已暴露给 MCP 的知识库：\n\n| ID | 名称 | 嵌入模型 | 文档 | 分片 |\n");
    s.push_str("|---|---|---|---|---|\n");
    for r in rows {
        let id: String = r.try_get("id").unwrap_or_default();
        let name: String = r.try_get("name").unwrap_or_default();
        let model: String = r.try_get("embedding_model").unwrap_or_default();
        let docs: i64 = r.try_get("doc_count").unwrap_or(0);
        let chunks: i64 = r.try_get("chunk_count").unwrap_or(0);
        s.push_str(&format!(
            "| `{id}` | {name} | {model} | {docs} | {chunks} |\n",
            id = id,
            name = name,
            model = model,
            docs = docs,
            chunks = chunks
        ));
    }
    ToolCallResult::text(s)
}

// -----------------------------------------------------------------------------
// Tool 3: ask_knowledge_base
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
struct AskArgs {
    kb_id: String,
    question: String,
    model: Option<String>,
}

async fn ask_knowledge_base(pool: &SqlitePool, args: Value) -> ToolCallResult {
    let args: AskArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return ToolCallResult::err(format!("参数非法: {}", e)),
    };

    if let Err(e) = require_exposed_kb(pool, &args.kb_id).await {
        return tool_err(e);
    }

    let model = args.model.unwrap_or_default();

    match crate::rag::ask::ask(
        pool,
        std::slice::from_ref(&args.kb_id),
        &args.question,
        &model,
        None,
        crate::rag::retrieve::RetrievalMode::Vector,
        5,
        0.3,
    )
    .await
    {
        Ok(res) => {
            let mut s = String::from("## 回答\n\n");
            s.push_str(&res.answer);
            if !res.sources.is_empty() {
                s.push_str("\n\n## 来源\n\n");
                for (i, src) in res.sources.iter().enumerate() {
                    s.push_str(&format!(
                        "[{i}] {title}（相似度 {score:.3}）\n{snippet}\n\n",
                        i = i + 1,
                        title = src.doc_title,
                        score = src.score,
                        snippet = truncate(&src.content, 280),
                    ));
                }
            }
            ToolCallResult::text(s)
        }
        Err(e) => ToolCallResult::err(format!("问答失败: {}", e)),
    }
}

// -----------------------------------------------------------------------------
// Tool 4: read_document
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
struct ReadArgs {
    kb_id: String,
    doc_id: String,
}

async fn read_document(pool: &SqlitePool, args: Value) -> ToolCallResult {
    let args: ReadArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return ToolCallResult::err(format!("参数非法: {}", e)),
    };

    if let Err(e) = require_exposed_kb(pool, &args.kb_id).await {
        return tool_err(e);
    }

    let doc_row = match sqlx::query(
        "SELECT id, title, source_type, source_ref, char_count, chunk_count, status, error_message
         FROM kb_documents WHERE id = ? AND kb_id = ?",
    )
    .bind(&args.doc_id)
    .bind(&args.kb_id)
    .fetch_optional(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => return ToolCallResult::err(format!("查询文档失败: {}", e)),
    };
    let doc_row = match doc_row {
        Some(r) => r,
        None => return ToolCallResult::err(format!("文档不存在: {}", args.doc_id)),
    };

    let title: String = doc_row.try_get("title").unwrap_or_default();
    let source_type: String = doc_row.try_get("source_type").unwrap_or_default();
    let source_ref: String = doc_row.try_get("source_ref").unwrap_or_default();
    let char_count: i64 = doc_row.try_get("char_count").unwrap_or(0);
    let chunk_count: i64 = doc_row.try_get("chunk_count").unwrap_or(0);
    let status: i64 = doc_row.try_get("status").unwrap_or(1);

    // 拉取分片正文（按 seq 排序）
    let chunks = match sqlx::query(
        "SELECT seq, content, token_count FROM kb_chunks WHERE doc_id = ? ORDER BY seq",
    )
    .bind(&args.doc_id)
    .fetch_all(pool)
    .await
    {
        Ok(r) => r,
        Err(e) => return ToolCallResult::err(format!("查询分片失败: {}", e)),
    };

    let mut s = format!(
        "# {title}\n\n来源类型: {source_type}\n来源引用: `{source_ref}`\n分片数: {chunks}\n字符数: {chars}\n状态: {status}\n\n## 正文\n\n",
        title = title,
        source_type = source_type,
        source_ref = source_ref,
        chunks = chunk_count,
        chars = char_count,
        status = match status {
            1 => "就绪",
            2 => "失败",
            _ => "处理中",
        },
    );

    for r in chunks {
        let seq: i32 = r.try_get("seq").unwrap_or(0);
        let content: String = r.try_get("content").unwrap_or_default();
        s.push_str(&format!("### 分片 {}\n{}\n\n", seq + 1, content));
    }
    ToolCallResult::text(s)
}

// -----------------------------------------------------------------------------
// Tool 5: get_knowledge_base_stats
// -----------------------------------------------------------------------------

#[derive(Deserialize)]
struct StatsArgs {
    kb_id: String,
}

async fn get_knowledge_base_stats(pool: &SqlitePool, args: Value) -> ToolCallResult {
    let args: StatsArgs = match serde_json::from_value(args) {
        Ok(a) => a,
        Err(e) => return ToolCallResult::err(format!("参数非法: {}", e)),
    };
    let kb = match require_exposed_kb(pool, &args.kb_id).await {
        Ok(kb) => kb,
        Err(e) => return tool_err(e),
    };

    let doc_count: i64 = sqlx::query_scalar(
        "SELECT COUNT(*) FROM kb_documents WHERE kb_id = ? AND status = 1",
    )
    .bind(&kb.id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let chunk_count: i64 = sqlx::query_scalar("SELECT COUNT(*) FROM kb_chunks WHERE kb_id = ?")
        .bind(&kb.id)
        .fetch_one(pool)
        .await
        .unwrap_or(0);

    let total_chars: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(char_count), 0) FROM kb_documents WHERE kb_id = ?",
    )
    .bind(&kb.id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    let total_tokens: i64 = sqlx::query_scalar(
        "SELECT COALESCE(SUM(token_count), 0) FROM kb_chunks WHERE kb_id = ?",
    )
    .bind(&kb.id)
    .fetch_one(pool)
    .await
    .unwrap_or(0);

    ToolCallResult::text(format!(
        "知识库「{name}」统计：\n\n- ID: `{id}`\n- 描述: {desc}\n- 嵌入模型: {model}（渠道 {channel}）\n- 状态: {status}\n- 已就绪文档: {docs}\n- 分片总数: {chunks}\n- 总字符数: {chars}\n- 总 token 数: {tokens}\n",
        name = kb.name,
        id = kb.id,
        desc = if kb.description.is_empty() { "（无）" } else { &kb.description },
        model = kb.embedding_model,
        channel = kb.embedding_channel_id,
        status = if kb.status == 1 { "启用" } else { "禁用" },
        docs = doc_count,
        chunks = chunk_count,
        chars = total_chars,
        tokens = total_tokens,
    ))
}

// -----------------------------------------------------------------------------
// helpers
// -----------------------------------------------------------------------------

/// 校验 KB 存在 + 已开启 MCP 暴露；返回完整行供上层做向量化等后续操作。
async fn require_exposed_kb(pool: &SqlitePool, kb_id: &str) -> Result<KnowledgeBaseRow, JsonRpcError> {
    if kb_id.is_empty() {
        return Err(JsonRpcError {
            code: ERR_MCP_KB_NOT_FOUND,
            message: "kb_id 不能为空".into(),
            data: None,
        });
    }
    let row: Option<KnowledgeBaseRow> =
        sqlx::query_as("SELECT * FROM knowledge_bases WHERE id = ?")
            .bind(kb_id)
            .fetch_optional(pool)
            .await
            .map_err(|e| {
                JsonRpcError::internal_error(format!("查询知识库失败: {}", e))
            })?;
    let row = row.ok_or_else(|| JsonRpcError {
        code: ERR_MCP_KB_NOT_FOUND,
        message: format!("知识库不存在: {}", kb_id),
        data: None,
    })?;

    // mcp_exposed = 1 过滤
    let exposed: i64 = sqlx::query_scalar("SELECT mcp_exposed FROM knowledge_bases WHERE id = ?")
        .bind(kb_id)
        .fetch_one(pool)
        .await
        .map_err(|e| JsonRpcError::internal_error(e.to_string()))?;
    if exposed != 1 {
        return Err(JsonRpcError {
            code: ERR_MCP_KB_NOT_EXPOSED,
            message: format!("知识库「{}」未开启 MCP 暴露", row.name),
            data: Some(json!({ "kb_id": kb_id, "name": row.name })),
        });
    }
    Ok(row)
}

#[cfg(test)]
mod tests {
    use super::*;
    use sqlx::sqlite::SqlitePoolOptions;

    /// 测试用 in-mem SQLite：单连接 + 跑全量迁移（含 010/011/012，加 `mcp_exposed` 列）。
    async fn test_pool() -> SqlitePool {
        let pool = SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite::memory:")
            .await
            .expect("in-memory pool");
        sqlx::migrate!("./migrations")
            .run(&pool)
            .await
            .expect("migrations");
        pool
    }

    /// 插入一条知识库（暴露开关由 `exposed` 控制）。
    async fn insert_kb(pool: &SqlitePool, id: &str, name: &str, exposed: bool) {
        let now = chrono::Utc::now().to_rfc3339();
        sqlx::query(
            "INSERT INTO knowledge_bases (id, name, description, embedding_model,
                embedding_channel_id, status, mcp_exposed, created_at, updated_at)
             VALUES (?, ?, '', 'text-embedding-3-small', 'ch-1', 1, ?, ?, ?)",
        )
        .bind(id)
        .bind(name)
        .bind(if exposed { 1i64 } else { 0i64 })
        .bind(&now)
        .bind(&now)
        .execute(pool)
        .await
        .expect("insert kb");
    }

    // ---------- 静态元数据（无需 DB） ----------

    /// `tool_specs()` 必须暴露 5 个工具且名称与契约一致。
    #[test]
    fn tool_specs_exposes_five_known_tools() {
        let specs = tool_specs();
        assert_eq!(specs.len(), 5);
        let names: Vec<&str> = specs.iter().map(|s| s.name).collect();
        assert!(names.contains(&"search_knowledge_base"));
        assert!(names.contains(&"list_knowledge_bases"));
        assert!(names.contains(&"ask_knowledge_base"));
        assert!(names.contains(&"read_document"));
        assert!(names.contains(&"get_knowledge_base_stats"));
    }

    /// 每个 tool 的 `input_schema` 必须是合法 JSON Schema object（MCP 协议要求）。
    #[test]
    fn tool_specs_schemas_are_valid_objects() {
        for s in tool_specs() {
            let schema = &s.input_schema;
            assert_eq!(
                schema.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "tool {} schema type 应为 object",
                s.name
            );
        }
    }

    /// 未知的 tool 名 dispatch 必须以 `isError=true` 文本回包（不 panic、不返 RPC error）。
    #[tokio::test]
    async fn dispatch_unknown_tool_returns_iserror() {
        let pool = test_pool().await;
        let result = dispatch(Arc::new(pool), "nope_unknown_tool", json!({})).await;
        assert!(result.is_error);
        let text = match &result.content[0] {
            ToolContent::Text { text } => text,
        };
        assert!(text.contains("未知工具"), "应说明未知工具，实际: {text}");
    }

    // ---------- `require_exposed_kb` 关卡测试 ----------

    /// KB 不存在 → ERR_MCP_KB_NOT_FOUND。
    #[tokio::test]
    async fn require_exposed_kb_blocks_missing_kb() {
        let pool = test_pool().await;
        let err = require_exposed_kb(&pool, "no-such-kb")
            .await
            .expect_err("KB 不存在应报错");
        assert_eq!(err.code, ERR_MCP_KB_NOT_FOUND);
    }

    /// KB 存在但 `mcp_exposed=0` → ERR_MCP_KB_NOT_EXPOSED（核心安全关卡）。
    #[tokio::test]
    async fn require_exposed_kb_blocks_unexposed() {
        let pool = test_pool().await;
        insert_kb(&pool, "kb-private", "未暴露 KB", false).await;

        let err = require_exposed_kb(&pool, "kb-private")
            .await
            .expect_err("未暴露 KB 必须被拒");
        assert_eq!(err.code, ERR_MCP_KB_NOT_EXPOSED);
    }

    /// `mcp_exposed=1` → 顺利通过，返回完整行。
    #[tokio::test]
    async fn require_exposed_kb_passes_when_exposed() {
        let pool = test_pool().await;
        insert_kb(&pool, "kb-public", "已暴露 KB", true).await;

        let row = require_exposed_kb(&pool, "kb-public")
            .await
            .expect("已暴露 KB 应通过");
        assert_eq!(row.id, "kb-public");
        assert_eq!(row.name, "已暴露 KB");
    }

    // ---------- `list_knowledge_bases` 过滤测试 ----------

    /// `list_knowledge_bases` SQL 把 `mcp_exposed=1` 且 `status=1` 才是 MCP 可见。
    /// 这里走 dispatch 路径验证整条管线的输出文本不包含未暴露 KB 的 id。
    #[tokio::test]
    async fn list_knowledge_bases_filters_by_mcp_exposed() {
        let pool = test_pool().await;
        insert_kb(&pool, "kb-public", "Public RAG", true).await;
        insert_kb(&pool, "kb-private", "Private RAG", false).await;
        insert_kb(&pool, "kb-disabled", "Disabled RAG", true).await;
        // 把 disabled 那条 status 设为 0
        sqlx::query("UPDATE knowledge_bases SET status = 0 WHERE id = ?")
            .bind("kb-disabled")
            .execute(&pool)
            .await
            .unwrap();

        let result = dispatch(Arc::new(pool), "list_knowledge_bases", json!({})).await;
        assert!(!result.is_error, "{result:?}");
        let text = match &result.content[0] {
            ToolContent::Text { text } => text,
        };

        assert!(text.contains("kb-public"), "应包含已暴露 KB");
        assert!(!text.contains("kb-private"), "不应包含未暴露 KB（核心安全关卡）");
        assert!(!text.contains("kb-disabled"), "不应包含已禁用的 KB");
    }
}

