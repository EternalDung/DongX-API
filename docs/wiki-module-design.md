# DongX Wiki 模块设计方案（草案 v1）

> 状态：**待评审**。本文件为设计产物，代码尚未实现。
> 关联模块：`src-tauri/src/{rag,core,adapter,db,models,commands,server}`、`src/pages/ServicesPage.tsx`、`src-tauri/migrations/`。
> 前置：本方案建立在 RAG（知识库）模块已落地的基础上，与 `docs/rag-architecture.md` 配套阅读。

---

## 0. 一句话定位

**RAG 是搜索引擎，Wiki 是研究员。**

- **知识库（RAG）**：把文档切成片段、向量化，提问时检索 top-k 原文片段拼进 prompt。适合"这句话在哪个文档的哪里"。
- **Wiki**：LLM 读完资料后**自己写出结构化的知识页面**（实体页/概念页/摘要页），用 `[[wikilink]]` 互相引用，并维护 `index.md` 目录与 `log.md` 变更日志；新资料进来时**增量更新已有页面**。适合"把这个主题讲清楚"。

核心差异只有一条：**知识是否积累**。RAG 每次都从头检索原文，Wiki 的知识体随摄入持续增长、被反复修订。

---

## 1. 与知识库的关系：互补，不替代

| 维度 | 知识库 RAG（已有） | Wiki（本方案） |
|---|---|---|
| 知识粒度 | chunk（段落级切片） | page（页面级，LLM 生成） |
| 构建方式 | 切分 + 向量化 | LLM 阅读 → 理解 → 重写 → 成页 |
| 检索方式 | 向量 + FTS5 混合 top-k | `index.md` 导航 + 关键词（+ 可选向量） |
| 知识形态 | 扁平原文片段 | 层级结构 + 交叉引用 + 知识图谱 |
| 是否积累 | 否，每次从头检索 | **是，增量更新** |
| 输出 | 片段拼接 + 来源引用 | 综合回答 + `[[页面]]` 引用 |
| 典型提问 | "XX 配置在哪份文档里提到" | "总结一下 XX 主题的现状" |

架构差异过大，强行合并会破坏各自优势。**独立模块 + 共享基础设施**是更合理的选择。二者可同时被 Agent 调用：RAG 找原文出处，Wiki 找综合分析。

---

## 2. 设计原则

1. **复用优先**：不重写解析、不重写模型调用、不重写日志。LLM 调用一律走现有 dispatcher/adapter，从而**自动获得渠道加权、渠道禁用、按 mode 熔断、Failover 重试**能力（这些是刚在 `channel-disable` 分支落地的资产）。
2. **与知识库并列**：服务页新增独立 Tab，数据模型完全独立（新表，不动 `kb_*`）。
3. **项目隔离**：一个 Wiki 项目 = 一个独立目录（`raw/` + `wiki/` + `schema/`）+ SQLite 元数据。
4. **闭环先行**：MVP 只做"摄入 → 成页 → 查询"，知识图谱 / Lint / 深度研究全部后置。
5. **迁移一次到位**：已应用的迁移禁止再编辑（sqlx SHA-384 校验），因此建表时把可预见的字段（如 `tags`）一次加齐，避免后续 `ALTER TABLE` 补字段。

---

## 3. 复用盘点（决定工作量）

### 3.1 可直接复用

| 能力 | 现有实现 | 复用方式 |
|---|---|---|
| 来源导入（Git / URL / 本地目录） | `rag/importer.rs` | **直接复用**，Wiki 源类型与之对齐 |
| 文本解析、文件类型识别 | `rag/parser.rs::detect_kind_by_name` | **直接复用** |
| 代码解析 | `rag/code_parser.rs` | 复用（Wiki 摄入代码仓库时有用） |
| 长文本切分 | `rag/chunk.rs` | 复用（长文档需切分后分批喂给 LLM） |
| LLM 调用（选渠道/加权/熔断/Failover） | `core/dispatcher.rs` + `adapter/*` | **直接复用**，`run_chat_pipeline` |
| 请求日志 | `db/repository.rs::request_logs::insert` | 复用，`api_key_name = "Wiki: {项目名}"`，mode = `wiki` |
| 前端组件体系 | shadcn/ui、`StatusBadge`、`CallStatsCard` | 复用 |

### 3.2 可选复用（**v1 明确不做**）

| 能力 | 现有实现 | 说明 |
|---|---|---|
| 向量化 | `rag/embed.rs` | **v1 不做**。Wiki 靠 `index.md` 导航 + 关键词检索，见 §11 决策 3 |
| 向量检索 | `rag/store.rs`、`rag/retrieve.rs` | **v1 不做**。页面量级大后再作为增强接入 |

### 3.3 必须新建

| 缺口 | 说明 | 建议 |
|---|---|---|
| Wiki 数据模型 | 7 张新表 | 迁移 `019_wiki_module.sql` |
| 摄入引擎 | LLM 分析 → 生成/更新页面 → 维护 index/log/wikilink | 核心工作量 |
| 查询引擎 | index 导航 → 读页 → 综合回答 | 核心工作量 |
| 前端 Wiki Tab | 服务页新增 Tab + 项目/页面/源/搜索子视图 | **参考实现前端是空的，这块必须自研** |
| PDF / Office 解析 | **当前完全没有**（解析器是文本/代码向） | MVP 不做；需要时再引入，属独立工作量 |

> ⚠️ **工作量提示**：现有解析只覆盖文本与代码。若 Wiki 要吃 PDF/Office，那是**额外的新增模块**，不应算进 Wiki 本体排期。
>
> 🐛 **附带发现的既有缺陷（与 Wiki 无关，建议顺手修）**：知识库详情页文案 `src/pages/KnowledgeBaseDetailPage.tsx:322` 写着"支持 … / .pdf 等文本类文件"，但**后端并无任何 PDF 解析**（`grep -i pdf src-tauri/src/` 零命中，`Cargo.toml` 无 PDF 依赖）。实际摄入 PDF 会走到 `rag/importer.rs:367` 的 `read_to_string`，因二进制非法 UTF-8 而**报错失败**（响亮失败，不会产生垃圾分块）。属"文案承诺了未实现的能力"，修复成本极低（改文案或移除 pdf 字样）。

---

## 4. 模块布局

遵循现有约定（模块一律入目录，根目录只留 `main.rs` / `lib.rs` / `tray.rs` / `crypto.rs`）：

```
src-tauri/src/
├── wiki/                      # ★ 新增，与 rag/ 平级
│   ├── mod.rs                 # 模块导出
│   ├── models.rs              # 数据模型（与 TS 类型对齐）
│   ├── repository.rs          # SQLite CRUD
│   ├── project.rs             # 项目管理（创建/导入/导出/删除）
│   ├── ingest.rs              # 摄入引擎（解析 → LLM → 成页 → 更新 index/log）
│   ├── query.rs               # 查询引擎（index 导航 → 读页 → 综合回答）
│   ├── lint.rs                # 【Phase 4】矛盾/孤儿/缺失/过时检测
│   └── graph.rs               # 【Phase 4】知识图谱（四信号 + 社区检测）
├── commands/wiki.rs           # ★ Tauri 管理面命令
└── ...
```

- **管理面**：`commands/wiki.rs` 暴露 Tauri invoke（与 `commands/key.rs`、`commands/channel.rs` 同构），在 `lib.rs` 注册。
- **数据面（可选）**：若需 HTTP 访问，挂 `/api/wiki/*` 到现有 9842 端口的 Axum 路由，与 `/v1/rag/*` 并列。**MVP 可先只做管理面**，外部 Agent 通过后续 MCP 暴露。

---

## 5. 数据模型（迁移 `019_wiki_module.sql`）

> 铁律：迁移文件一旦应用禁止再编辑。下列字段（含 `tags`）请一次设计到位。

```sql
-- Wiki 项目
CREATE TABLE IF NOT EXISTS wiki_projects (
    id                TEXT PRIMARY KEY,
    name              TEXT NOT NULL,
    description       TEXT,
    status            INTEGER NOT NULL DEFAULT 1,   -- 0=停用 1=启用
    schema_text       TEXT,                          -- 维护规则（Wiki 的"宪法"）
    wiki_dir          TEXT NOT NULL,                 -- 项目目录路径
    ingest_channel_id TEXT,                          -- 摄入用渠道
    ingest_model      TEXT,                          -- 摄入用模型（建议强模型）
    chat_channel_id   TEXT,                          -- 查询用渠道
    chat_model        TEXT,                          -- 查询用模型（建议快模型）
    source_count      INTEGER NOT NULL DEFAULT 0,
    page_count        INTEGER NOT NULL DEFAULT 0,
    last_ingest_at    TEXT,
    created_at        TEXT NOT NULL,
    updated_at        TEXT NOT NULL
);

-- Wiki 页面（LLM 生成）
CREATE TABLE IF NOT EXISTS wiki_pages (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL,
    path          TEXT NOT NULL,                     -- wiki/ 下相对路径
    title         TEXT NOT NULL,
    page_type     TEXT NOT NULL,                     -- entity|concept|summary|review|index|log
    content_hash  TEXT NOT NULL,
    token_count   INTEGER NOT NULL DEFAULT 0,
    wikilinks     TEXT NOT NULL DEFAULT '[]',        -- JSON 数组
    tags          TEXT NOT NULL DEFAULT '[]',        -- JSON 数组（一次加齐，避免后续 ALTER）
    frontmatter   TEXT NOT NULL DEFAULT '{}',
    status        TEXT NOT NULL DEFAULT 'active',    -- active|stale|orphan
    created_at    TEXT NOT NULL,
    updated_at    TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE,
    UNIQUE(project_id, path)
);
CREATE INDEX IF NOT EXISTS idx_wiki_pages_project ON wiki_pages(project_id);
CREATE INDEX IF NOT EXISTS idx_wiki_pages_type    ON wiki_pages(project_id, page_type);
CREATE INDEX IF NOT EXISTS idx_wiki_pages_tags    ON wiki_pages(project_id, status);

-- 源资料
CREATE TABLE IF NOT EXISTS wiki_sources (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL,
    source_type   TEXT NOT NULL,                     -- git|url|local_dir（与 rag/importer 对齐）
    filename      TEXT NOT NULL,
    file_path     TEXT,
    source_url    TEXT,
    content_hash  TEXT,
    status        TEXT NOT NULL DEFAULT 'pending',   -- pending|ingested|failed
    error_message TEXT,
    created_at    TEXT NOT NULL,
    ingested_at   TEXT,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_wiki_sources_project ON wiki_sources(project_id);

-- 摄入任务队列（长耗时，需进度可见）
CREATE TABLE IF NOT EXISTS wiki_ingest_queue (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL,
    source_id     TEXT,
    task_type     TEXT NOT NULL,                     -- ingest|lint|reindex
    status        TEXT NOT NULL DEFAULT 'pending',   -- pending|running|done|failed|cancelled
    progress      INTEGER NOT NULL DEFAULT 0,
    error_message TEXT,
    created_at    TEXT NOT NULL,
    completed_at  TEXT,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_wiki_queue_project ON wiki_ingest_queue(project_id);
CREATE INDEX IF NOT EXISTS idx_wiki_queue_status  ON wiki_ingest_queue(status);

-- 审核项【Phase 4 启用】
CREATE TABLE IF NOT EXISTS wiki_reviews (
    id            TEXT PRIMARY KEY,
    project_id    TEXT NOT NULL,
    review_type   TEXT NOT NULL,                     -- contradiction|orphan|missing_page|stale
    title         TEXT NOT NULL,
    description   TEXT,
    affected_pages TEXT NOT NULL DEFAULT '[]',
    resolved      INTEGER NOT NULL DEFAULT 0,
    created_at    TEXT NOT NULL,
    resolved_at   TEXT,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_wiki_reviews_project ON wiki_reviews(project_id);

-- 问答会话【Phase 3 启用】
CREATE TABLE IF NOT EXISTS wiki_sessions (
    id           TEXT PRIMARY KEY,
    project_id   TEXT NOT NULL,
    role         TEXT NOT NULL,
    content      TEXT NOT NULL,
    sources_json TEXT,                               -- 引用的页面路径
    model        TEXT,
    tokens_used  INTEGER NOT NULL DEFAULT 0,
    created_at   TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE
);
CREATE INDEX IF NOT EXISTS idx_wiki_sessions_project ON wiki_sessions(project_id);

-- 知识图谱边【Phase 4 启用】
CREATE TABLE IF NOT EXISTS wiki_graph_edges (
    id          TEXT PRIMARY KEY,
    project_id  TEXT NOT NULL,
    source_page TEXT NOT NULL,
    target_page TEXT NOT NULL,
    edge_type   TEXT NOT NULL,                       -- direct|source_overlap|type_affinity
    weight      REAL NOT NULL DEFAULT 0.0,
    created_at  TEXT NOT NULL,
    FOREIGN KEY (project_id) REFERENCES wiki_projects(id) ON DELETE CASCADE,
    UNIQUE(project_id, source_page, target_page, edge_type)
);
CREATE INDEX IF NOT EXISTS idx_wiki_edges_project ON wiki_graph_edges(project_id);
```

**与渠道的绑定**：`ingest_channel_id` / `chat_channel_id` 让摄入与查询可走不同渠道与模型——摄入用强模型保证页面质量，查询用快模型压成本。未配置时 fallback 到默认路由。

---

## 6. 文件系统布局

```
%APPDATA%\com.wei.dongx\wiki\
├── projects\
│   └── {project_id}\
│       ├── raw\           # 不可变原始资料
│       ├── wiki\          # LLM 生成的页面
│       │   ├── index.md   # 内容目录（查询导航入口）
│       │   ├── log.md     # 变更日志
│       │   ├── entities\  # 实体页
│       │   └── concepts\  # 概念页
│       └── schema\
│           └── rules.md   # 维护规则（可编辑）
└── config.json
```

元数据（页面 hash、wikilinks、状态）存 SQLite；正文以 Markdown 落盘——**让人可以直接用编辑器打开阅读和手改**，这是 Wiki 相对 RAG 的重要体验优势。

---

## 7. 核心流程

### 7.1 摄入（Ingest）

```
添加源 → 复用 rag/importer 拉取 → 复用 rag/parser 解析为文本
   → 长文档按 rag/chunk 切分
   → 逐块调用 LLM（走 dispatcher，ingest_channel/ingest_model）
   → LLM 输出 JSON：新增/更新哪些页面
   → 写盘（entities/*.md）+ 更新 index.md + 追加 log.md
   → 维护 [[wikilink]] → 更新 wiki_pages 元数据与 hash
   → 写入 request_logs（api_key_name="Wiki: {项目名}", mode="wiki"）
```

**增量关键**：源 `content_hash` 未变则跳过，避免重复烧 token。

**摄入 Prompt 骨架**：

```text
你是 Wiki 维护者。按以下规则处理新资料。

## 维护规则
{schema_text}

## 现有 Wiki 目录
{index_md_content}

## 新资料
{document_content}

## 任务
1. 提取关键实体与概念
2. 对每个新增/更新项生成 Markdown 页面
3. 用 [[wikilink]] 关联已有页面
4. 更新 index.md（新增条目 + 一句话摘要）
5. 追加 log.md 变更记录
6. 标注潜在的矛盾与缺失

输出 JSON：
{"pages":[{"path":"","content":"","action":"create|update"}],
 "index_update":"","log_entry":"",
 "reviews":[{"type":"","title":"","description":""}]}
```

### 7.2 查询（Query）

```
提问 → LLM 读 index.md 定位相关页面路径
     → 读取 3-5 个页面正文
     → 走 run_chat_pipeline 综合回答（chat_channel/chat_model）
     → 返回答案 + [[页面]] 引用，存入 wiki_sessions
```

**v1 不做向量**：页面定位靠 `index.md` 导航 + 标题/路径关键词匹配（可复用 FTS5），与向量检索完全解耦。理由见 §11 决策 3。

---

## 8. 实施路线

| Phase | 内容 | 产出 | 建议 |
|---|---|---|---|
| **1 骨架** | 迁移 019、`wiki/{mod,models,repository,project}.rs`、`commands/wiki.rs` 项目 CRUD、前端 Wiki Tab 空壳 | 项目可创建/配置 | 做 |
| **2 摄入** | 复用 importer/parser/chunk + 摄入引擎 + 队列与进度 | 源 → Wiki 页面 | 做 |
| **3 查询 + 前端** | 查询引擎 + 页面列表/预览/编辑 + 搜索问答 | **闭环可用** | 做 |
| **4 图谱 + Lint** | 图谱构建与可视化、矛盾/孤儿/缺失检测 | 知识治理 | 后置 |
| **5 MCP + Skill** | 暴露 `wiki_search` / `wiki_ask` 等工具，接入服务页 Skill Tab | 外部 Agent 可用 | 后置 |

**建议 MVP 边界 = Phase 1 + 2 + 3。** 先把"摄入 → 成页 → 查询"闭环跑通并配一套能用的前端；图谱与 Lint 属于锦上添花，可等闭环验证价值后再投。

> 说明：参考实现（同类开源产品）在 Wiki 上**只做了后端 Phase 1-2，前端是占位路由**。这意味着前端是我们必须自研的部分，同时也是做出差异化的地方。

---

## 9. 前端设计

### 9.1 入口

服务页 `ServicesPage` 的 `TABS` 中 **Wiki Tab 已存在**（当前为 `Placeholder` 占位），只需填充内容，无需改路由表。

| 页面 | 位置 | 说明 |
|---|---|---|
| Wiki 主页面（列表） | 服务页 `TabsContent value="wiki"` 内 | 与 RAG Tab 同构：头部说明 + 刷新/新建，下方项目行 |
| 新建 Wiki | Dialog | 与「新建知识库」弹窗同构 |
| Wiki 详情 | 新增路由 `/services/wiki/:projectId` → `WikiDetailPage` | 与 `KnowledgeBaseDetailPage` 同构（返回 + 标题 + 横向 Tabs） |

### 9.2 主页面（项目列表）

行结构对齐 RAG 列表（`KnowledgeBaseRow`）：

```
[头像] 名称 + 状态徽章          [启用开关] [删除]
       描述（单行截断）
       3 源 · 24 页面 · claude-sonnet-4 · 更新 09-06 17:20
```

- **头像**：复用 `avatarColor(name)` hash 配色（Wiki 用冷色系变体区别于知识库的暖色）+ `Globe` 图标。
- **状态徽章**：**只有两个状态** —— `就绪`（绿 success）/ `禁用`（橙 warning）。
  **没有「摄入中」**：摄入的是文档，进度属于「源」粒度，不冒泡成项目状态
  （一个项目可并行多个源，项目级状态表达不了「哪个在跑」，且会与禁用开关语义打架）。
- **启用/禁用控件**：用 `Switch` 开关（与 RAG 列表的 MCP / 启用开关同一控件），
  **不再用图标按钮**；下方带「启用」文字标签。
- **元信息**：源数 · 页面数 · 引用数 · 更新时间。
- **操作区**：启用开关 + 删除图标按钮，均 `stopPropagation` 阻止行点击。
- **状态色沿用统一约定**：启用=绿（`success`）、禁用=橙（`warning`），与渠道/密钥一致。

### 9.3 新建 Wiki（Dialog）

按"**新建空白项目、独立源**"决策，**创建时不选源**，源在详情页添加。

| 字段 | 必填 | 说明 |
|---|---|---|
| 名称 | 是 | 项目名，同时决定头像字符与配色 |
| 描述 | 否 | 单行 |
| 摄入渠道 / 摄入模型 | 否 | 用于生成页面，建议强模型；留空走默认路由 |
| 查询渠道 / 查询模型 | 否 | 用于问答，建议快模型；留空走默认路由 |
| 维护规则 | 否 | schema 文本，留空用内置默认模板 |

### 9.4 Wiki 详情（6 个 Tab，概览置前）

RAG 详情有 7 个 Tab（文档/来源/检索/问答/索引/设置/MCP），Wiki 按自身语义裁剪为 **6 个**：

| Tab | 内容 | 对应 RAG |
|---|---|---|
| **概览** | 项目描述 + 四项统计（源/页面/引用/Token 估算）+ 来源状态分布 + 最近更新页面 | 新增 |
| **页面** | `index.md` 置顶（page_type=`索引`，即首页/导航入口） + 每页带**分类徽标**（概念/实体/日志/索引/摘要 共 5 类，对齐 waliapi）；顶部保留搜索条、右侧加**分类过滤**下拉；列表含摘要 / inline wikilink 引用（可点击跳转）/ tokens / 更新时间；整个列表包进白卡，与 RAG 文档列表基线一致 | ≈ 文档 |
| **源** | **直接复制 RAG 文档上传组件**（拖拽区 + 上传中 chips + 自动摄入），上传后**自动触发摄入**；来源列表包进白卡（与 RAG 文档列表基线一致），不展示绝对路径，仅显来源类型 / 进度 / 状态；保留按源进度条 | ≈ 来源 |
| **搜索** | 基于 Wiki 的检索/问答，返回命中页面 + `[[页面]]` 引用 + token/耗时 | = 问答（前端已更名「问答」→「搜索」） |
| 图谱 | antv/g6 力导向原型：用页面 `[[wikilink]]` 关系在前端推导渲染，支持缩放/平移/拖拽节点/悬停高亮关联；后端 019（迁移 + `commands/wiki.rs`）已落地，图谱边由前端据页面 links 推导（不建独立边表） | 前端 ✅ 原型 / 后端 ✅ 019 |
| **设置** | 名称/描述、**模型配置**（对话渠道 `chat_channel_id` + 对话模型 `chat_model` 下拉选择）、维护规则、危险区删除项目 | = 设置（「生成配置」→「模型配置」） |

**Tab 均带 lucide 图标**（概览 `LayoutDashboard` / 页面 `FileText` / 源 `Database` / 搜索 `Search` / 图谱 `Network` / 设置 `Settings`），与 RAG 详情页一致。

**去掉的 Tab 及理由**：`索引`（v1 无向量，无索引概念）、`MCP`（v1 不做数据面）、`检索`（v1 无向量，页面列表自带搜索即可）。

**头部只留「返回 + 项目名 + 状态徽章」**：源数/页面数等统计信息**不再放在标题下**，统一移入「概览」Tab，避免详情页头部信息过载。

**摄入进度只在「源」Tab 的单个源上体现**（状态徽章 + 进度条），项目级状态保持就绪/禁用两态不变。

**WIKI 项目列表预留 MCP 开关**：每行在「启用」开关旁预留一个 disabled 的 MCP 开关（tooltip「后端接入后开放」）；字段 `mcp_exposed` 已加入 `WikiProject` 类型、迁移 019 与 `commands/wiki.rs`（更新命令已接受该字段）；前端开关仍 disabled 占位，待明确「MCP 暴露」的语义（暴露哪些工具/检索接口）后再启用。

**页面预览**：v1 无 Markdown 渲染依赖，点击页面行弹出 Dialog 以纯文本（`whitespace-pre-wrap`）展示正文 + 引用 chips。

> 按项目约定：UI 动手前先出 mockup 确认，再写代码。三张 mockup（主页面 / 新建 / 详情）已在评审时交付确认，本节的调整为评审后修订。

### 9.5 前端落地状态（评审后）

| 文件 | 内容 |
|---|---|
| `src/types/index.ts` | `WikiProject` / `WikiPage` / `WikiSource` / `WikiAskResult` 等类型 |
| `src/lib/api.ts` | `wikiApi`（10 个方法），直接调用真实 Tauri 命令（迁移 019 + `commands/wiki.rs`） |
| （已删除）`src/lib/wiki-mock.ts` | 原内存数据源，后端就绪后已由真实命令取代并删除 |
| `src/components/wiki/WikiTabPanel.tsx` | 主页面：项目列表 + 新建 + 删除 + 启用开关 |
| `src/pages/WikiDetailPage.tsx` | 详情页：6 个 Tab |
| `src/App.tsx` | 路由 `services/wiki/:projectId` |

> ✅ **后端已落地**：迁移 `019_wiki_module.sql`（wiki_projects / wiki_sources / wiki_pages 三表）与 `src-tauri/src/commands/wiki.rs`（10 个命令：项目 CRUD、来源增删、页面列表、摄入、问答）已实现；`wikiApi` 直连真实 Tauri 命令，`src/lib/wiki-mock.ts` 已删除。命令参数 camelCase→snake_case 由 Tauri v2 自动转换，返回值蛇形命名与 `src/types/index.ts` 的 Wiki* 一致。

---

## 10. 风险与缓解

| 风险 | 缓解 |
|---|---|
| 摄入成本高（每文档都调 LLM） | `content_hash` 增量跳过；摄入/查询可用不同模型；队列可取消 |
| 页面质量依赖 Prompt | `schema` 可编辑 + Prompt 模板持续迭代 |
| 大规模检索性能 | < 500 页用 index 导航；超出后再引入 embedding |
| 与知识库边界模糊 | 文档明确分工；UI 上并列展示但文案区分 |
| 迁移不可改 | 建表一次到位（`tags` 等字段提前加） |
| 前端工作量被低估 | 参考实现前端为空，需自研；建议先出 mockup 定范围 |

---

## 11. 决策记录（v1 已定）

| # | 议题 | 结论 | 影响 |
|---|---|---|---|
| 1 | MVP 是否含 PDF/Office 解析 | **不做**，v1 只支持本地目录/文件的文本类文件（git/url 暂未实现，命令会返回明确错误）；同时暴露既有文案 bug（知识库前端声称支持 pdf 但后端无实现），需另开小任务修文案 | 见 §3.3 |
| 2 | 是否需要数据面 HTTP 端点 | **第一版不做**，只做 Tauri 管理面（`commands/wiki.rs`）；对外暴露留到后续 MCP 阶段 | §4 中 `/api/wiki/*` 推迟 |
| 3 | Wiki 是否做向量处理 | **不做**，v1 靠 `index.md` 导航 + 关键词/FTS5 检索 | §3.2 的 embedding 复用推迟；`rag/embed.rs` 留作后期增强入口 |
| 4 | 摄入是否占用网关密钥配额 | **不占**，日志记账沿用 `api_key_name = "Wiki: {项目名}"`（与 RAG 的 `"RAG: {知识库名}"` 对齐），mode = `wiki` | 与既有 `request_logs` 记账方式一致 |
| 5 | Wiki 项目的来源 | **新建空白项目 + 独立源**，不从已有知识库派生 | 新建弹窗不含源选择；源在详情页「源」Tab 添加；Wiki 与知识库无数据耦合 |
| 6 | 项目状态是否需要「摄入中」 | **不需要**，项目只有 `就绪` / `禁用` 两态；摄入进度只体现在「源」粒度 | 一个项目可并行多个源，项目级状态无法表达"哪个在跑"；且会与启用开关语义冲突 |
| 7 | 启用/禁用控件 | 用 `Switch` 开关，与 RAG 列表的启用/MCP 开关统一 | 放弃图标按钮方案，全站一致 |
| 8 | 详情页头部信息 | 标题下**不再放**源数/页面数等统计，统一移入「概览」Tab | 头部只保留「返回 + 项目名 + 状态徽章 + 刷新」 |
| 9 | 问答 Tab 命名 / 设置配置项 | **问答 → 搜索**；设置「生成配置」→「模型配置」，新增**对话渠道 + 对话模型**两个下拉（绑定 `chat_channel_id` / `chat_model`，复用现有渠道与模型预设） | 与 RAG 检索交互一致；模型配置解耦「摄入模型」与「对话模型」 |

### 关于决策 3 的补充说明

Wiki 不依赖向量是**设计使然，不是偷工减料**，原因是它的检索对象与 RAG 根本不同：

| | RAG 为什么**必须**要向量 | Wiki 为什么**可以不要** |
|---|---|---|
| 检索对象 | 海量、扁平、未消化的原文片段 | 少量（几十~几百）、已结构化、LLM 消化过的页面 |
| 匹配需求 | 用户问法与原文措辞差异大，需语义匹配 | 页面标题/目录本身就是语义摘要，关键词即可命中 |
| 导航手段 | 无 | 有 `index.md` 目录 + `[[wikilink]]` 显式关联 |

同类开源产品的 Wiki 实现同样**零向量代码**（其检索为 `LIKE` 关键词匹配），可作为佐证。

**需要向量的时机**：页面数超过约 500，或需要"语义找相关页面"而非"按目录找"。届时 `rag/embed.rs` 可直接复用，接入成本低，不需要现在预付。
