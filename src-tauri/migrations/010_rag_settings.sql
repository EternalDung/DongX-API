-- 010_rag_settings.sql
-- RAG 知识库设置扩展：MCP 暴露、Embedding 批次、分块过滤字段。
--
-- 设计说明：「启用 RAG」开关复用既有 status 列（0=禁用 1=启用，Phase 1 已建），
-- 此处不再新增 enabled 列，避免同一状态出现两份真源。
-- 仅新增以下 5 列，均为后续「设置」页的持久化字段：
--   mcp_exposed         是否将本知识库暴露给 MCP 层（默认否）
--   embedding_batch_size 单次向量化的文档批大小（NULL=取引擎默认）
--   exclude_dirs        摄入时排除的目录（逗号分隔，NULL=不排除）
--   exclude_files       摄入时排除的文件（逗号分隔，NULL=不排除）
--   include_file_types  摄入时仅包含的文件类型（逗号分隔，NULL=全部）

ALTER TABLE knowledge_bases ADD COLUMN mcp_exposed          INTEGER NOT NULL DEFAULT 0;
ALTER TABLE knowledge_bases ADD COLUMN embedding_batch_size INTEGER;
ALTER TABLE knowledge_bases ADD COLUMN exclude_dirs         TEXT;
ALTER TABLE knowledge_bases ADD COLUMN exclude_files        TEXT;
ALTER TABLE knowledge_bases ADD COLUMN include_file_types   TEXT;
