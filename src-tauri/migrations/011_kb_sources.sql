-- 011: 知识库摄入来源（Git 仓库 / 单个 URL / 本地目录）
--
-- 与「文档」(kb_documents) 区分：来源是「一次导入任务」的元信息记录，
-- 用于追踪进度（fetching / done / error）、复跑与删除。
-- 实际文本经分块向量化后落入 kb_documents（source_ref = "<source_id>::<相对路径>"）。
--
-- 过滤字段（excluded_dirs / included_files / max_file_size）与知识库设置里的
-- 全局过滤合并后写入，便于在来源列表里回看本次实际使用的规则。

CREATE TABLE IF NOT EXISTS kb_sources (
  id             TEXT PRIMARY KEY,
  kb_id          TEXT NOT NULL,
  source_type    TEXT NOT NULL,             -- git | url | local_dir
  repo_url       TEXT,                      -- git: 仓库地址
  branch         TEXT,                      -- git: 分支（可选）
  token          TEXT,                      -- git: 访问令牌（本地单用户明文，与 api_keys 同策略）
  url            TEXT,                      -- url: 链接
  dir_path       TEXT,                      -- local_dir: 本地目录路径
  subpath        TEXT,                      -- 仅扫描根目录下的子路径（可选）
  excluded_dirs  TEXT,                      -- 逗号分隔，本次实际使用的排除目录
  included_files TEXT,                      -- 逗号分隔，本次实际使用的包含文件（扩展名/子串）
  max_file_size  INTEGER,                   -- 字节；NULL=引擎默认(1MB)
  status         TEXT NOT NULL DEFAULT 'fetching',  -- fetching | done | error
  file_count     INTEGER NOT NULL DEFAULT 0,
  error_message  TEXT,
  created_at     TEXT NOT NULL,
  updated_at     TEXT NOT NULL
);
CREATE INDEX IF NOT EXISTS idx_kb_sources_kb ON kb_sources(kb_id);
