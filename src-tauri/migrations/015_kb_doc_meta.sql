-- 015_kb_doc_meta.sql
-- 文档级元数据：文件大小、token 总数、内容哈希（用于重复上传去重）。
-- 均为向后兼容的新增列，NOT NULL + DEFAULT 保证旧行自动补 0/空串。

ALTER TABLE kb_documents ADD COLUMN file_size   INTEGER NOT NULL DEFAULT 0;
ALTER TABLE kb_documents ADD COLUMN token_count INTEGER NOT NULL DEFAULT 0;
ALTER TABLE kb_documents ADD COLUMN content_hash TEXT    NOT NULL DEFAULT '';
