-- 019: Wiki 知识库模块
-- 项目 / 来源 / 页面三张表。聚合统计（来源数、页面数、引用数、token 估算、
-- 最近摄入时间）在读取时通过 LEFT JOIN 子查询实时算出，不落冗余列。
-- 图谱边由前端依据页面 links 推导，故不单独建边表。

CREATE TABLE IF NOT EXISTS wiki_projects (
    id                  TEXT PRIMARY KEY,
    name                TEXT NOT NULL,
    description         TEXT NOT NULL DEFAULT '',
    channel_id          TEXT NOT NULL DEFAULT '',
    model               TEXT NOT NULL DEFAULT '',
    maintenance_prompt  TEXT NOT NULL DEFAULT '',
    chat_channel_id     TEXT NOT NULL DEFAULT '',
    chat_model          TEXT NOT NULL DEFAULT '',
    mcp_exposed         INTEGER NOT NULL DEFAULT 0,
    status              INTEGER NOT NULL DEFAULT 1,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL
);

CREATE TABLE IF NOT EXISTS wiki_sources (
    id               TEXT PRIMARY KEY,
    project_id       TEXT NOT NULL,
    kind             TEXT NOT NULL DEFAULT 'local_dir', -- git | url | local_dir
    locator          TEXT NOT NULL DEFAULT '',          -- 仓库 URL / 网页 URL / 本地绝对路径
    branch           TEXT,
    status           TEXT NOT NULL DEFAULT 'pending',  -- pending | ingesting | ready | failed
    ingested         INTEGER NOT NULL DEFAULT 0,
    total            INTEGER NOT NULL DEFAULT 0,
    error            TEXT,
    last_ingest_at   TEXT,
    created_at       TEXT NOT NULL,
    updated_at       TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_wiki_sources_project ON wiki_sources(project_id);

CREATE TABLE IF NOT EXISTS wiki_pages (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL,
    title       TEXT NOT NULL,
    slug        TEXT NOT NULL,
    content     TEXT NOT NULL DEFAULT '',
    is_index    INTEGER NOT NULL DEFAULT 0,
    kind        TEXT NOT NULL DEFAULT '概念', -- 概念 | 实体 | 日志 | 索引 | 摘要
    links       TEXT NOT NULL DEFAULT '[]',   -- JSON 数组：[[wikilink]] 指向的页面标题
    tokens      INTEGER NOT NULL DEFAULT 0,
    source_id   TEXT NOT NULL DEFAULT '',      -- 用于按来源级联删除页面
    created_at  TEXT NOT NULL,
    updated_at  TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_wiki_pages_project ON wiki_pages(project_id);
CREATE UNIQUE INDEX IF NOT EXISTS idx_wiki_pages_proj_slug ON wiki_pages(project_id, slug);
