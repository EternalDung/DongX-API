//! 来源导入：把 Git 仓库 / 单个 URL / 本地目录 中的文本文件摄入到知识库。
//!
//! 设计贴合 DongX 现有引擎：
//! - 复用 `rag::ingest::ingest_text`（分块 → 嵌入 → 落库），不在本模块重复管线；
//! - Git 直接 shell out 系统 `git`（零额外依赖；要求用户机装有 git）；
//! - 目录遍历用 `tokio::fs` 自实现（不引入 walkdir）；
//! - 一次导入是一个后台任务：命令层写 `kb_sources`(status='fetching') 后立即返回，
//!   后台任务跑完再回写 `done` / `error` + file_count，前端轮询即可看到进度。
//!
//! 过滤语义（与知识库设置里的全局过滤合并后生效）：
//! - `excluded_dirs`：跳过目录名命中的目录（含内置默认黑名单，如 node_modules/.git）。
//! - `exclude_files`：跳过文件名命中的文件。
//! - `included_files`：仅保留文件名/扩展名命中的文件；为空则回退内置扩展名白名单。
//! - `max_file_size`：跳过超过该字节数的文件。

use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use regex::Regex;
use sqlx::SqlitePool;
use tokio::process::Command;

/// 内置默认排除目录（大小写不敏感精确匹配目录名）。
const BUILTIN_EXCLUDE_DIRS: &[&str] = &[
    "node_modules",
    ".git",
    "target",
    "__pycache__",
    "dist",
    ".next",
    "build",
    "vendor",
    ".venv",
    "venv",
    ".idea",
    ".vscode",
    "out",
    "coverage",
    ".turbo",
    ".svelte-kit",
];

/// 内置默认支持扩展名（小写，不含点）。`included_files` 为空时回退到此白名单。
const BUILTIN_EXTS: &[&str] = &[
    "md",
    "markdown",
    "txt",
    "text",
    "rst",
    "org",
    "adoc",
    "json",
    "jsonl",
    "yaml",
    "yml",
    "toml",
    "csv",
    "tsv",
    "log",
    "xml",
    "html",
    "htm",
    "css",
    "scss",
    "sass",
    "less",
    "js",
    "jsx",
    "mjs",
    "cjs",
    "ts",
    "tsx",
    "py",
    "rb",
    "go",
    "rs",
    "java",
    "kt",
    "kts",
    "c",
    "cpp",
    "h",
    "hpp",
    "cc",
    "cs",
    "swift",
    "php",
    "scala",
    "dart",
    "sh",
    "bash",
    "zsh",
    "sql",
    "r",
    "lua",
    "vim",
    "dockerfile",
    "makefile",
    "env.example",
    "gradle",
];

/// 摄入过滤规则（已合并知识库全局默认值与来源级覆盖）。
#[derive(Debug, Clone)]
pub struct ImportFilters {
    pub excluded_dirs: Vec<String>,
    pub exclude_files: Vec<String>,
    pub included_files: Vec<String>,
    pub max_file_size: usize,
}

impl ImportFilters {
    /// 目录名命中的排除目录？
    fn is_excluded_dir(&self, name: &str) -> bool {
        let n = name.to_lowercase();
        BUILTIN_EXCLUDE_DIRS.contains(&n.as_str())
            || self
                .excluded_dirs
                .iter()
                .any(|d| d.trim().to_lowercase() == n)
    }

    /// 文件名命中的排除文件？
    fn is_excluded_file(&self, name: &str) -> bool {
        let n = name.to_lowercase();
        self.exclude_files
            .iter()
            .any(|f| f.trim().to_lowercase() == n)
    }

    /// 文件是否通过「仅包含」过滤？
    fn is_included(&self, name: &str) -> bool {
        let n = name.to_lowercase();
        if self.included_files.is_empty() {
            // 回退内置扩展名白名单
            return match n.rsplit('.').next() {
                Some(ext) => BUILTIN_EXTS.contains(&ext),
                None => false,
            };
        }
        self.included_files.iter().any(|p| {
            let p = p.trim().to_lowercase();
            !p.is_empty() && n.contains(&p)
        })
    }
}

/// 已解析的来源配置（命令层负责把原始输入 + 知识库默认值合并成它）。
pub struct ResolvedImport {
    pub source_type: String, // git | url | local_dir
    pub repo_url: Option<String>,
    pub branch: Option<String>,
    pub token: Option<String>,
    pub url: Option<String>,
    pub dir_path: Option<String>,
    pub subpath: Option<String>,
    pub filters: ImportFilters,
}

/// 入口：后台任务调用。根据来源类型分发，结束后写回 `kb_sources.status`。
pub async fn run_import(
    pool: SqlitePool,
    kb_id: String,
    source_id: String,
    r: ResolvedImport,
) -> Result<(), String> {
    let result = match r.source_type.as_str() {
        "git" => import_git(&pool, &kb_id, &source_id, &r).await,
        "url" => import_url(&pool, &kb_id, &r).await,
        "local_dir" => import_local_dir(&pool, &kb_id, &source_id, &r).await,
        other => Err(format!("不支持的来源类型: {}", other)),
    };
    match result {
        Ok(count) => {
            set_status(&pool, &source_id, "done", Some(count as i64), None).await;
        }
        Err(e) => {
            tracing::error!("来源导入失败 source={} : {}", source_id, e);
            set_status(&pool, &source_id, "error", None, Some(e)).await;
        }
    }
    Ok(())
}

async fn set_status(
    pool: &SqlitePool,
    id: &str,
    status: &str,
    file_count: Option<i64>,
    err: Option<String>,
) {
    let now = chrono::Utc::now().to_rfc3339();
    let _ = sqlx::query(
        "UPDATE kb_sources SET status = ?, file_count = COALESCE(?, file_count), \
         error_message = ?, updated_at = ? WHERE id = ?",
    )
    .bind(status)
    .bind(file_count)
    .bind(err)
    .bind(&now)
    .bind(id)
    .execute(pool)
    .await;
}

// ---------------------------------------------------------------------------
// Git 导入：clone → 遍历 → 摄入 → 清理临时目录
// ---------------------------------------------------------------------------

async fn import_git(
    pool: &SqlitePool,
    kb_id: &str,
    source_id: &str,
    r: &ResolvedImport,
) -> Result<usize, String> {
    let repo = r
        .repo_url
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "Git 导入需要仓库地址".to_string())?;

    // 把 access token 注入 https URL（https://<token>@host/...）
    let mut clone_url = repo.clone();
    if let Some(t) = &r.token {
        let t = t.trim();
        if !t.is_empty() {
            clone_url = clone_url.replacen("https://", &format!("https://{}@", t), 1);
        }
    }

    let target = std::env::temp_dir().join(format!("dongx_git_{}", uuid::Uuid::new_v4()));

    let mut cmd = Command::new("git");
    cmd.arg("clone").arg("--depth").arg("1");
    if let Some(b) = &r.branch {
        let b = b.trim();
        if !b.is_empty() {
            cmd.arg("--branch").arg(b);
        }
    }
    cmd.arg(&clone_url).arg(&target);

    let out = cmd.output().await.map_err(|e| {
        format!(
            "执行 git clone 失败: {}（请确认本机已安装 git 且在 PATH 中）",
            e
        )
    })?;
    if !out.status.success() {
        let stderr = String::from_utf8_lossy(&out.stderr).to_string();
        let _ = tokio::fs::remove_dir_all(&target).await;
        let non_empty: Vec<&str> = stderr.lines().filter(|l| !l.is_empty()).collect();
        let last = non_empty.last().copied().unwrap_or("");
        return Err(format!("git clone 失败: {}", last));
    }

    let scan_root = match &r.subpath {
        Some(s) if !s.trim().is_empty() => target.join(s.trim()),
        _ => target.clone(),
    };

    let count = scan_and_ingest(pool, kb_id, source_id, &scan_root, &r.filters, "git").await;

    // 无论成败都清理临时克隆目录（best-effort）
    let _ = tokio::fs::remove_dir_all(&target).await;
    count
}

// ---------------------------------------------------------------------------
// 本地目录导入
// ---------------------------------------------------------------------------

async fn import_local_dir(
    pool: &SqlitePool,
    kb_id: &str,
    source_id: &str,
    r: &ResolvedImport,
) -> Result<usize, String> {
    let dir = r
        .dir_path
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "本地目录导入需要目录路径".to_string())?;
    let root = Path::new(&dir);
    if !root.exists() || !root.is_dir() {
        return Err(format!("目录不存在: {}", dir));
    }
    let scan_root = match &r.subpath {
        Some(s) if !s.trim().is_empty() => root.join(s.trim()),
        _ => root.to_path_buf(),
    };
    scan_and_ingest(pool, kb_id, source_id, &scan_root, &r.filters, "file").await
}

// ---------------------------------------------------------------------------
// URL 导入：fetch → 去标签 → 摄入单个文档
// ---------------------------------------------------------------------------

async fn import_url(pool: &SqlitePool, kb_id: &str, r: &ResolvedImport) -> Result<usize, String> {
    let url = r
        .url
        .as_ref()
        .map(|s| s.trim().to_string())
        .filter(|s| !s.is_empty())
        .ok_or_else(|| "URL 导入需要链接".to_string())?;

    let resp = reqwest::get(&url)
        .await
        .map_err(|e| format!("请求 URL 失败: {}", e))?;
    if !resp.status().is_success() {
        return Err(format!("URL 返回状态码 {}", resp.status()));
    }
    let ct = resp
        .headers()
        .get(reqwest::header::CONTENT_TYPE)
        .and_then(|v| v.to_str().ok())
        .unwrap_or("")
        .to_string();
    let raw = resp
        .text()
        .await
        .map_err(|e| format!("读取响应体失败: {}", e))?;
    // HTML 页面去掉标签再摄入；其它（json/yaml/markdown 等）直接用原文
    let text = if ct.contains("html") {
        strip_html(&raw)
    } else {
        raw
    };
    if text.trim().is_empty() {
        return Err("该 URL 无可提取的文本内容".to_string());
    }
    if text.len() > r.filters.max_file_size {
        return Err("URL 文本内容超过最大文件大小限制".to_string());
    }

    let source_ref = url.clone();
    // 复用 ingest_file 以便正确记录 source_type = "url"；命中重复则跳过但不报错。
    match ingest_file(pool, kb_id, &url, &text, "url", &source_ref).await {
        Ok(o) => {
            if o.duplicate {
                tracing::info!("URL 内容已存在，已跳过: {}", url);
            }
            Ok(1)
        }
        Err(e) => Err(e),
    }
}

// ---------------------------------------------------------------------------
// 共享：目录遍历 + 逐文件摄入
// ---------------------------------------------------------------------------

/// 异步迭代遍历 `root`，应用过滤后收集 eligible 文件到 `out`。
async fn collect_files(
    root: &Path,
    filters: &ImportFilters,
    out: &mut Vec<PathBuf>,
) -> Result<(), String> {
    let mut stack: Vec<PathBuf> = vec![root.to_path_buf()];
    while let Some(dir) = stack.pop() {
        let mut entries = tokio::fs::read_dir(&dir)
            .await
            .map_err(|e| format!("读取目录失败 {}: {}", dir.display(), e))?;
        loop {
            let entry = match entries.next_entry().await {
                Ok(Some(e)) => e,
                Ok(None) => break,
                Err(e) => return Err(format!("遍历目录出错: {}", e)),
            };
            let p = entry.path();
            if p.is_dir() {
                let name = p
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if filters.is_excluded_dir(&name) {
                    continue;
                }
                stack.push(p);
            } else if p.is_file() {
                let name = p
                    .file_name()
                    .and_then(|s| s.to_str())
                    .unwrap_or("")
                    .to_string();
                if filters.is_excluded_file(&name) || !filters.is_included(&name) {
                    continue;
                }
                match tokio::fs::metadata(&p).await {
                    Ok(m) if m.len() as usize > filters.max_file_size => continue,
                    Ok(_) => {}
                    Err(_) => continue,
                }
                out.push(p);
            }
        }
    }
    Ok(())
}

/// 遍历目录、对每个文本文件复用 `ingest_text` 摄入，返回成功文件数。
async fn scan_and_ingest(
    pool: &SqlitePool,
    kb_id: &str,
    source_id: &str,
    root: &Path,
    filters: &ImportFilters,
    source_type: &str,
) -> Result<usize, String> {
    let mut files: Vec<PathBuf> = Vec::new();
    collect_files(root, filters, &mut files).await?;

    let mut count = 0usize;
    let mut skipped = 0usize;
    for p in files {
        let text = match tokio::fs::read_to_string(&p).await {
            Ok(t) => t,
            Err(_) => continue, // 二进制或不可读 → 跳过
        };
        if text.trim().is_empty() {
            continue;
        }
        let rel = p
            .strip_prefix(root)
            .unwrap_or(&p)
            .to_string_lossy()
            .replace('\\', "/");
        let source_ref = format!("{}::{}", source_id, rel);

        match ingest_file(pool, kb_id, &rel, &text, source_type, &source_ref).await {
            Ok(o) if !o.duplicate => count += 1,
            Ok(_) => skipped += 1, // 命中内容去重
            Err(e) => tracing::warn!("摄入文件失败 {}: {}", rel, e),
        }
    }
    if skipped > 0 {
        tracing::info!(
            "批量导入完成（source={}）：处理 {} 个，跳过 {} 个重复",
            source_id,
            count,
            skipped
        );
    }
    Ok(count)
}

/// 摄入单个文件的返回（用于内部去重信号）。
pub(crate) struct IngestOutcome {
    /// 是否命中内容去重（true=跳过，未重复摄入）。
    pub duplicate: bool,
}

/// 摄入单个文件：分块 → 向量化 → 落库，并写入正确的 `source_type`
/// （git/file/url），使文档列表能如实反映来源。复用引擎的 chunk/embed/store。
async fn ingest_file(
    pool: &SqlitePool,
    kb_id: &str,
    title: &str,
    text: &str,
    source_type: &str,
    source_ref: &str,
) -> Result<IngestOutcome, String> {
    if text.trim().is_empty() {
        return Ok(IngestOutcome { duplicate: false });
    }

    // 去重：先按内容哈希查重，命中已就绪文档则直接复用，避免对同一内容
    // 重复分块+向量化。覆盖 URL / 本地目录 / Git 三条摄入路径，与单文件上传走同一查询。
    let hash = crate::rag::store::content_hash(text);
    if crate::rag::store::find_document_by_hash(pool, kb_id, &hash)
        .await
        .map_err(|e| e.to_string())?
        .is_some()
    {
        tracing::info!(
            "内容哈希命中已就绪文档，跳过摄入（kb={}, source_ref={}）",
            kb_id,
            source_ref
        );
        return Ok(IngestOutcome { duplicate: true });
    }

    let kb = crate::rag::store::get_kb(pool, kb_id)
        .await
        .map_err(|e| e.to_string())?;
    // 按文件名判定类型（含扩展名），分块时记录来源路径与语言/符号元数据
    let kind = crate::rag::parser::detect_kind_by_name(title);
    let config = crate::rag::chunk::SplitConfig::from_kb(kb.chunk_size, kb.chunk_overlap);
    let chunks = crate::rag::chunk::split(text, kind, Some(title), &config);
    if chunks.is_empty() {
        return Ok(IngestOutcome { duplicate: false });
    }
    let contents: Vec<String> = chunks.iter().map(|c| c.content.clone()).collect();
    let (vecs, token_count) = crate::rag::embed::embed_texts(
        pool,
        &kb.embedding_channel_id,
        &kb.embedding_model,
        contents,
    )
    .await
    .map_err(|e| e.to_string())?;
    if vecs.len() != chunks.len() {
        return Err("嵌入返回的向量数量与分块数量不一致".to_string());
    }
    let inputs: Vec<crate::rag::store::ChunkInput> = chunks
        .into_iter()
        .zip(vecs)
        .map(|(c, e)| crate::rag::store::ChunkInput {
            content: c.content,
            embedding: e,
            meta: c.meta,
        })
        .collect();
    crate::rag::store::insert_document(
        pool,
        kb_id,
        title,
        source_type,
        source_ref,
        &kb.embedding_model,
        text.len() as i64,
        token_count,
        &hash,
        inputs,
    )
    .await
    .map_err(|e| e.to_string())?;
    Ok(IngestOutcome { duplicate: false })
}

/// 朴素 HTML 标签剥离（保留可见文本）。
fn strip_html(s: &str) -> String {
    static RE_HTML: OnceLock<Regex> = OnceLock::new();
    let re = RE_HTML.get_or_init(|| Regex::new(r"<[^>]*>").unwrap());
    re.replace_all(s, " ").to_string()
}
