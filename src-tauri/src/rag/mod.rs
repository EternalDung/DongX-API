//! RAG 引擎模块（Phase 1 摄入 / 检索 / 问答）。
//!
//! 与 `commands/rag.rs`（Tauri 管理面命令）区分：本模块是纯引擎逻辑，
//! 被 Tauri 命令与 KnowledgeService 的 `/v1/rag/ask` 路由共用。
//! 向量存储 v1 采用暴力余弦（embeddings 落 `kb_chunks.embedding` JSON）。

pub mod ask;
pub mod chunk;
pub mod code_parser;
pub mod embed;
pub mod importer;
pub mod ingest;
pub mod models;
pub mod parser;
pub mod retrieve;
pub mod store;
