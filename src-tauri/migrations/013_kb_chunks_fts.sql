-- 013_kb_chunks_fts.sql
-- FTS5 全文索引（trigram tokenizer）替代手写 BM25：用于关键词 / 混合检索的关键词打分。
--
-- 采用「独立表 + 应用层显式同步」而非 external content + 触发器：
--   kb_chunks 主键为 TEXT uuid，无 INTEGER rowid，而 FTS5 external content
--   要求 content 表具备 INTEGER rowid，故不满足。独立表完全不动 kb_chunks 结构，
--   由 store 层在写入 / 删除分块时同步维护。
--
-- 注意：FTS5 是虚拟表，不支持普通二级索引（CREATE INDEX 会报错），
--       kb_id 过滤直接走 WHERE 约束，本地单用户规模下足够。

CREATE VIRTUAL TABLE IF NOT EXISTS kb_chunks_fts USING fts5(
    chunk_id UNINDEXED,   -- 对应 kb_chunks.id (TEXT uuid)
    kb_id    UNINDEXED,   -- 过滤用（WHERE 约束，无二级索引）
    content,              -- 被索引正文
    tokenize = 'trigram'  -- trigram 对中文子串匹配最友好，无需空格分词
);
