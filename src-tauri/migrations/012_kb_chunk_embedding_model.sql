-- 012_kb_chunk_embedding_model.sql
-- 分块级嵌入模型追踪：用于「重建索引」时判定哪些分块是用旧模型嵌入的（stale），
-- 以及在索引状态页展示「是否已全部用当前模型嵌入」。
-- 摄入（ingest / 来源导入）落库时一并写入当前知识库绑定的 embedding_model。
ALTER TABLE kb_chunks ADD COLUMN embedding_model TEXT;
