//! Rust -> TypeScript 契约导出（ts-rs）。
//!
//! 只在 `ts-export` feature 下参与编译：
//!
//! ```bash
//! cargo test --features ts-export --lib export_ts_bindings
//! ```
//!
//! 导出目录固定为前端的 `src/types/generated/`，产出 `rag.ts` / `wiki.ts`
//! 两个文件（分组由各类型上的 `#[ts(export_to = "..")]` 决定）。前端直接
//! 引用这些类型，不再手写同步。
//!
//! `export_all_to` 会连同依赖类型一起写出，因此每个 `export_to` 分组只需
//! 列出根类型；同组内其余类型会被自动合并进同一个文件。

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use ts_rs::TS;

    /// 前端类型目录：`src-tauri/../src/types/generated`
    fn out_dir() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("..")
            .join("src")
            .join("types")
            .join("generated")
    }

    #[test]
    fn export_ts_bindings() {
        let dir = out_dir();
        std::fs::create_dir_all(&dir).expect("创建 src/types/generated 失败");

        // ---- RAG ----
        crate::commands::rag::KnowledgeBase::export_all_to(&dir).unwrap();
        crate::commands::rag::KnowledgeBaseInput::export_all_to(&dir).unwrap();
        crate::commands::rag::KnowledgeBaseUpdate::export_all_to(&dir).unwrap();
        crate::commands::rag::ImportSourceInput::export_all_to(&dir).unwrap();
        crate::commands::rag::KbDocument::export_all_to(&dir).unwrap();
        crate::commands::rag::KbSource::export_all_to(&dir).unwrap();
        crate::commands::rag::DocumentChunksPage::export_all_to(&dir).unwrap();
        crate::commands::rag::IndexStatus::export_all_to(&dir).unwrap();
        crate::commands::rag::RetrievalHit::export_all_to(&dir).unwrap();
        crate::rag::ingest::IngestResult::export_all_to(&dir).unwrap();
        crate::rag::ask::AskResult::export_all_to(&dir).unwrap();

        // ---- Wiki ----
        crate::wiki::store::WikiProject::export_all_to(&dir).unwrap();
        crate::wiki::store::WikiSource::export_all_to(&dir).unwrap();
        crate::wiki::store::WikiPage::export_all_to(&dir).unwrap();
        crate::wiki::store::WikiAskResult::export_all_to(&dir).unwrap();
        crate::commands::wiki::WikiProjectInput::export_all_to(&dir).unwrap();
        crate::commands::wiki::WikiProjectUpdate::export_all_to(&dir).unwrap();
        crate::commands::wiki::WikiSourceInput::export_all_to(&dir).unwrap();
    }
}
