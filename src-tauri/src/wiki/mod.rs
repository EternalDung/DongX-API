//! Wiki 知识库模块：项目 / 来源 / 页面 的持久化、摄入与问答。
//!
//! 与 `commands/wiki.rs`（Tauri 管理面命令）区分：本模块是纯引擎逻辑，
//! 被 Tauri 命令调用。图谱数据由前端依据页面 `links` 在前端推导，
//! 后端只负责页面与其 wikilink 关系的落库。

pub mod ask;
pub mod ingest;
pub mod store;
