//! Wiki 来源摄入：解析本地文件/目录，按 `#` 标题切分为页面，提取
//! `[[wikilink]]` 引用，按 slug upsert 进 `wiki_pages`。
//!
//! v1 仅支持本地来源（`local_dir`：目录或单文件）。git / url 在 v1 返回明确错误，
//! 待后续接入远程抓取。

use std::fs;
use std::path::Path;

use regex::Regex;
use sqlx::SqlitePool;

use crate::error::AppError;
use crate::wiki::store;

/// 触发某个来源的摄入。返回更新后的来源行（无论成功或失败都会刷新状态）。
pub async fn ingest_source(pool: &SqlitePool, source_id: &str) -> Result<store::WikiSource, AppError> {
    let src = store::get_source(pool, source_id).await?;
    store::update_source_progress(pool, source_id, "ingesting", 0, 0, None, None).await?;

    match do_ingest(pool, &src).await {
        Ok((pages, total)) => {
            store::update_source_progress(
                pool,
                source_id,
                "ready",
                pages as i64,
                total,
                None,
                Some(store::now()),
            )
            .await?;
            store::get_source(pool, source_id).await
        }
        Err(e) => {
            let msg = e.to_string();
            store::update_source_progress(pool, source_id, "failed", 0, 0, Some(msg), None).await?;
            Err(e)
        }
    }
}

async fn do_ingest(
    pool: &SqlitePool,
    src: &store::WikiSource,
) -> Result<(usize, i64), AppError> {
    let path = Path::new(&src.locator);
    if !path.exists() {
        return Err(AppError::Validation(format!("来源路径不存在: {}", src.locator)));
    }

    let mut files: Vec<String> = Vec::new();
    if path.is_dir() {
        collect_files(path, &mut files);
    } else {
        files.push(src.locator.clone());
    }

    if files.is_empty() {
        return Err(AppError::Validation(
            "未在来源中找到可摄入的文件（支持 .md / .markdown / .txt / .mdx）".into(),
        ));
    }
    let total = files.len() as i64;

    let mut page_count = 0usize;
    for f in &files {
        let content = match fs::read_to_string(f) {
            Ok(c) => c,
            Err(_) => continue, // 跳过非 UTF-8 / 二进制
        };
        for p in split_pages(&content) {
            let links_json =
                serde_json::to_string(&p.links).unwrap_or_else(|_| "[]".into());
            let tokens = (p.content.chars().count() as f64 / 4.0).ceil() as i64;
            store::upsert_page(
                pool,
                &src.project_id,
                &src.id,
                &p.title,
                &p.slug,
                &p.content,
                p.is_index,
                &p.kind,
                &links_json,
                tokens,
            )
            .await?;
            page_count += 1;
        }
    }

    Ok((page_count, total))
}

/// 递归收集目录下所有受支持扩展名的文件。
fn collect_files(dir: &Path, out: &mut Vec<String>) {
    if let Ok(entries) = fs::read_dir(dir) {
        for e in entries.flatten() {
            let p = e.path();
            if p.is_dir() {
                collect_files(&p, out);
            } else if let Some(ext) = p.extension().and_then(|s| s.to_str()) {
                let ext = ext.to_lowercase();
                if matches!(ext.as_str(), "md" | "markdown" | "txt" | "mdx") {
                    out.push(p.to_string_lossy().to_string());
                }
            }
        }
    }
}

// ---------------------------------------------------------------------------
// 页面切分与解析
// ---------------------------------------------------------------------------

struct RawPage {
    title: String,
    slug: String,
    content: String,
    is_index: bool,
    kind: String,
    links: Vec<String>,
}

/// 按一级标题 `#` 切分 Markdown 为多个页面；首个页面标记为索引页。
fn split_pages(content: &str) -> Vec<RawPage> {
    let re_head = Regex::new(r"(?m)^#\s+(.+?)\s*$").unwrap();
    let matches: Vec<_> = re_head.captures_iter(content).collect();

    if matches.is_empty() {
        let title = content
            .lines()
            .find(|l| !l.trim().is_empty())
            .unwrap_or("未命名")
            .trim()
            .to_string();
        return vec![RawPage {
            title: title.clone(),
            slug: slugify(&title),
            content: content.to_string(),
            is_index: true,
            kind: infer_kind(&title),
            links: extract_links(content),
        }];
    }

    let mut pages = Vec::new();
    for (i, caps) in matches.iter().enumerate() {
        let whole = caps.get(0).expect("captures_iter 必含整段匹配");
        let title = caps[1].trim().to_string();
        let start = whole.end();
        let end = if i + 1 < matches.len() {
            matches[i + 1].get(0).expect("captures_iter 必含整段匹配").start()
        } else {
            content.len()
        };
        let mut body = content[start..end].to_string();
        // 首个标题之前的序言并入首个页面
        if i == 0 {
            let preamble = content[..whole.start()].to_string();
            body = format!("{preamble}\n{body}");
        }
        pages.push(RawPage {
            title: title.clone(),
            slug: slugify(&title),
            links: extract_links(&body),
            is_index: i == 0,
            kind: infer_kind(&title),
            content: body,
        });
    }
    pages
}

/// 提取正文中的 `[[页面标题]]` 引用（去重、保序）。
fn extract_links(content: &str) -> Vec<String> {
    let re = Regex::new(r"\[\[([^\[\]]+)\]\]").unwrap();
    let mut out = Vec::new();
    for m in re.captures_iter(content) {
        let inner = m[1].trim().to_string();
        if !inner.is_empty() && !out.contains(&inner) {
            out.push(inner);
        }
    }
    out
}

/// 依据标题启发式推断页面分类。
fn infer_kind(title: &str) -> String {
    let t = title.to_lowercase();
    if t.contains("目录")
        || t.contains("索引")
        || t.contains("index")
        || t.contains("导航")
        || t.contains("概览")
    {
        "索引".into()
    } else if t.contains("概念") || t.contains("concept") {
        "概念".into()
    } else if t.contains("日志")
        || t.contains("log")
        || t.contains("变更")
        || t.contains("记录")
    {
        "日志".into()
    } else if t.contains("实体")
        || t.contains("entity")
        || t.contains("组件")
        || t.contains("模块")
        || t.contains("服务")
        || t.contains("接口")
    {
        "实体".into()
    } else if t.contains("摘要") || t.contains("summary") || t.contains("总结") {
        "摘要".into()
    } else {
        "概念".into()
    }
}

/// 生成 URL 友好 slug：非字母数字（含中文）与数字外的字符折叠为 `-`。
fn slugify(title: &str) -> String {
    let re = Regex::new(r"[^a-z0-9\u4e00-\u9fff]+").unwrap();
    let s = re.replace_all(&title.to_lowercase(), "-").to_string();
    s.trim_matches('-').to_string()
}
