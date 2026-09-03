-- 009_rag.sql
-- RAG 知识库基础表：知识库 / 文档 / 分块。
-- Phase 1 仅落地知识库 CRUD（list / create / delete），
-- 文档与分块表先建好结构，摄入（分块 + 向量化）在后续 Phase 填充。

-- 知识库：RAG 检索的数据源，每个绑定一个嵌入模型与提供该模型的渠道。
CREATE TABLE IF NOT EXISTS knowledge_bases (
  id                  TEXT PRIMARY KEY,
  name                TEXT NOT NULL,
  description         TEXT NOT NULL DEFAULT '',
  embedding_model     TEXT NOT NULL,
  embedding_channel_id TEXT NOT NULL DEFAULT '',
  status              INTEGER NOT NULL DEFAULT 1,  -- 0=disabled 1=enabled
  created_at          TEXT NOT NULL,
  updated_at          TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_kb_status ON knowledge_bases(status);

-- 文档：一个知识库可含多份文档（上传 / 本地目录 / 后续 Git/URL）。
CREATE TABLE IF NOT EXISTS kb_documents (
  id            TEXT PRIMARY KEY,
  kb_id         TEXT NOT NULL,
  title         TEXT NOT NULL,
  source_type   TEXT NOT NULL DEFAULT 'text',  -- text | file | url | git
  source_ref    TEXT NOT NULL DEFAULT '',      -- 文件路径 / URL / 等
  char_count    INTEGER NOT NULL DEFAULT 0,
  chunk_count   INTEGER NOT NULL DEFAULT 0,
  status        INTEGER NOT NULL DEFAULT 1,    -- 0=处理中 1=已就绪 2=失败
  error_message TEXT,
  created_at    TEXT NOT NULL,
  updated_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_kb_docs_kb ON kb_documents(kb_id);

-- 分块：文档按长度切分后的最小检索单元，含向量（暴力余弦 v1 存 JSON）。
CREATE TABLE IF NOT EXISTS kb_chunks (
  id            TEXT PRIMARY KEY,
  kb_id         TEXT NOT NULL,
  doc_id        TEXT NOT NULL,
  seq           INTEGER NOT NULL DEFAULT 0,
  content       TEXT NOT NULL,
  embedding     TEXT,                           -- JSON array of f32
  token_count   INTEGER NOT NULL DEFAULT 0,
  created_at    TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_kb_chunks_kb  ON kb_chunks(kb_id);
CREATE INDEX IF NOT EXISTS idx_kb_chunks_doc ON kb_chunks(doc_id);
