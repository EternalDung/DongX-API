-- 016_kb_chunk_config.sql
-- 知识库级分块配置：分块大小与重叠字符数。
-- 0 表示「使用引擎默认」（1500 / 200），由摄入流程在 0 时回落默认。

ALTER TABLE knowledge_bases ADD COLUMN chunk_size   INTEGER NOT NULL DEFAULT 0;
ALTER TABLE knowledge_bases ADD COLUMN chunk_overlap INTEGER NOT NULL DEFAULT 0;
