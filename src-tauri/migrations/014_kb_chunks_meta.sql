-- 014_kb_chunks_meta.sql
-- 类型感知分块的块级语义元数据：标题 / 语言 / 符号 / 行范围 / 来源路径。
-- 这些列不参与 FTS5 索引（仅 content 被索引），纯存储用于溯源与后续引用展示。
-- 行范围列为 NOT NULL 并给默认 0，避免存量数据迁移时缺列报错。

ALTER TABLE kb_chunks ADD COLUMN heading TEXT;
ALTER TABLE kb_chunks ADD COLUMN language TEXT;
ALTER TABLE kb_chunks ADD COLUMN symbol_name TEXT;
ALTER TABLE kb_chunks ADD COLUMN symbol_kind TEXT;
ALTER TABLE kb_chunks ADD COLUMN signature TEXT;
ALTER TABLE kb_chunks ADD COLUMN line_start INTEGER NOT NULL DEFAULT 0;
ALTER TABLE kb_chunks ADD COLUMN line_end INTEGER NOT NULL DEFAULT 0;
ALTER TABLE kb_chunks ADD COLUMN source_path TEXT;
