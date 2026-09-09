// 静态技能包契约：Wiki 工具清单（name / description / 入参 schema / 出参描述）。
//
// **必须与后端 `src-tauri/src/mcp/wiki_tools.rs` 的 `specs()` 保持一致**——
// 后端工具增减或改 schema 时，同步改这里（页面展示与导出都吃这份数据）。
// 共享类型见 ./skillTypes。
//
// 与 RAG 的两个差异，导出给外部 agent 时容易踩：
// 1. 页面按 `slug` 寻址（不是 waliapi 那种 `path`），这是 DongX `wiki_pages` 表的语义；
// 2. `ask_wiki` **不需要传 model**，回答模型取 Wiki 项目自身配置的 chat_model
//    （RAG 的 ask_knowledge_base 则要求显式传 model）。

import type { SkillToolSpec } from "./skillTypes";

export const wikiSkills: SkillToolSpec[] = [
  {
    name: "list_wiki_projects",
    description: "列出所有已开启 MCP 暴露的 Wiki 项目（ID / 名称 / 页面数 / 源数 / 链接数）",
    returns:
      "Markdown 表格：| ID | 名称 | 页面 | 源 | 链接 | 描述 |，每行为一个已开启「MCP 暴露」且启用的 Wiki 项目。无项目时返回提示文本。",
    inputSchema: {
      type: "object",
      properties: {},
      required: [],
      additionalProperties: false,
    },
  },
  {
    name: "get_wiki_project",
    description: "获取 Wiki 项目详情：描述、页面/源/链接统计、最近摄入时间。",
    returns:
      "Markdown：## Wiki 项目「名称」，含 ID / 描述 / 页面数 / 源资料数 / wikilink 数 / 预估 token / 摄入模型 / 问答模型 / 最近摄入（从未摄入显示「从未」）。",
    inputSchema: {
      type: "object",
      properties: {
        project_id: { type: "string", description: "Wiki 项目 ID" },
      },
      required: ["project_id"],
      additionalProperties: false,
    },
  },
  {
    name: "list_wiki_pages",
    description: "列出 Wiki 项目的所有页面（标题 / slug / 分类 / wikilink）。",
    returns:
      "Markdown 表格：| 标题 | slug | 分类 | wikilink |，每行为该项目的一个页面（首页排最前）。无页面时返回提示文本。",
    inputSchema: {
      type: "object",
      properties: {
        project_id: { type: "string", description: "Wiki 项目 ID" },
      },
      required: ["project_id"],
      additionalProperties: false,
    },
  },
  {
    name: "get_wiki_page",
    description: "读取指定 Wiki 页面的完整 Markdown 正文。",
    returns:
      "Markdown：页面标题 + 元信息（slug / 分类 / token / 更新时间 / wikilink）+ 分隔线 + 完整正文。slug 不存在时返回错误文本并提示用 list_wiki_pages 查看。",
    inputSchema: {
      type: "object",
      properties: {
        project_id: { type: "string", description: "Wiki 项目 ID" },
        slug: {
          type: "string",
          description: "页面 slug（如 'index' 或 'guides/setup'），可由 list_wiki_pages 获取",
        },
      },
      required: ["project_id", "slug"],
      additionalProperties: false,
    },
  },
  {
    name: "search_wiki",
    description: "在 Wiki 页面内做关键词检索，返回命中页面与上下文片段（标题/slug/正文加权打分）。",
    returns:
      "文本：Wiki 项目「名称」中匹配「query」的页面 Top-N，每条含序号、标题、slug、得分、上下文片段（截断 320 字）。无命中返回提示文本。注意这是关键词打分，不是向量语义检索。",
    inputSchema: {
      type: "object",
      properties: {
        project_id: { type: "string", description: "Wiki 项目 ID" },
        query: { type: "string", description: "检索关键词（空格分隔多词）" },
        top_k: {
          type: "integer",
          description: "返回条数（默认 10，范围 1-30）",
          default: 10,
          minimum: 1,
          maximum: 30,
        },
      },
      required: ["project_id", "query"],
      additionalProperties: false,
    },
  },
  {
    name: "ask_wiki",
    description:
      "向 Wiki 提问：检索相关页面 → LLM 生成回答 → 返回回答 + 来源引用。回答模型取项目自身配置的 chat_model，无需传入。",
    returns:
      "Markdown：## 回答（基于检索到的 Wiki 页面生成）+ ## 来源（每条含序号、页面标题、slug、摘录）。无来源时仅返回回答。",
    inputSchema: {
      type: "object",
      properties: {
        project_id: { type: "string", description: "Wiki 项目 ID" },
        question: { type: "string", description: "问题" },
      },
      required: ["project_id", "question"],
      additionalProperties: false,
    },
  },
  {
    name: "list_wiki_sources",
    description: "列出 Wiki 项目的源资料及其摄入状态。",
    returns:
      "Markdown 表格：| ID | 类型 | 位置 | 状态 | 进度 | 最近摄入 |，每行为该项目的一个源资料。无源资料时返回提示文本。",
    inputSchema: {
      type: "object",
      properties: {
        project_id: { type: "string", description: "Wiki 项目 ID" },
      },
      required: ["project_id"],
      additionalProperties: false,
    },
  },
];
