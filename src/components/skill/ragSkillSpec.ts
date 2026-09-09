// 静态技能包契约：RAG 工具清单（name / description / 入参 schema / 出参描述）。
//
// **必须与后端 `src-tauri/src/mcp/tools.rs` 的 `rag_specs()` 保持一致**——
// 后端工具增减或改 schema 时，同步改这里（页面展示与导出都吃这份数据）。
// 共享类型见 ./skillTypes（RAG / Wiki 两个技能包共用同一套结构）。

import type { SkillToolSpec } from "./skillTypes";

export const ragSkills: SkillToolSpec[] = [
  {
    name: "search_knowledge_base",
    description: "语义检索 RAG，返回匹配文本片段和相似度评分",
    returns:
      "文本。格式：知识库「名称」Top-N 命中，每条含序号、文档标题、相似度分数(0~1)、doc_id（供 read_document 读全文）、片段正文（截断 320 字）。未命中返回提示文本。",
    inputSchema: {
      type: "object",
      properties: {
        kb_id: { type: "string", description: "知识库 id（必须已开启「MCP 暴露」）" },
        query: { type: "string", description: "查询文本" },
        top_k: {
          type: "integer",
          description: "返回条数（默认 5，范围 1-20）",
          default: 5,
          minimum: 1,
          maximum: 20,
        },
      },
      required: ["kb_id", "query"],
      additionalProperties: false,
    },
  },
  {
    name: "list_knowledge_bases",
    description: "列出所有已暴露的 RAG（ID/名称/文档数）",
    returns:
      "Markdown 表格：| ID | 名称 | 嵌入模型 | 文档数 | 分片数 |，每行为一个已暴露且启用的知识库。",
    inputSchema: {
      type: "object",
      properties: {},
      required: [],
      additionalProperties: false,
    },
  },
  {
    name: "ask_knowledge_base",
    description: "RAG 问答，基于检索内容生成回答并返回来源引用",
    returns:
      "Markdown：## 回答（基于检索上下文生成）+ ## 来源（每条含标题、相似度、片段）。无来源时仅返回回答。",
    inputSchema: {
      type: "object",
      properties: {
        kb_id: { type: "string", description: "知识库 id（必须已开启「MCP 暴露」）" },
        question: { type: "string", description: "用户问题" },
        model: {
          type: "string",
          description: "用于生成回答的 chat 模型（必填，例如 deepseek-v4-flash；网关要求显式指定）",
        },
      },
      required: ["kb_id", "question", "model"],
      additionalProperties: false,
    },
  },
  {
    name: "read_document",
    description: "读取指定文档的完整内容（含分片正文）",
    returns:
      "Markdown：文档元信息（标题 / 来源类型 / 分片数 / 字符数 / 状态）+ 各分片正文（### 分片 n）。",
    inputSchema: {
      type: "object",
      properties: {
        kb_id: { type: "string", description: "知识库 id" },
        doc_id: { type: "string", description: "文档 id" },
      },
      required: ["kb_id", "doc_id"],
      additionalProperties: false,
    },
  },
  {
    name: "get_knowledge_base_stats",
    description: "获取 RAG 统计信息（文档数 / 切片数 / token 数）",
    returns:
      "文本：ID / 描述 / 嵌入模型 / 渠道 / 状态 / 已就绪文档数 / 分片总数 / 总字符数 / 总 token 数。",
    inputSchema: {
      type: "object",
      properties: {
        kb_id: { type: "string", description: "知识库 id" },
      },
      required: ["kb_id"],
      additionalProperties: false,
    },
  },
  {
    name: "list_documents",
    description:
      "列出知识库下的文档（ID/标题/来源/分片数/状态），用于取得 read_document 需要的 doc_id",
    returns:
      "Markdown 表格：| ID | 标题 | 来源 | 字符 | 分片 | 状态 |，按创建时间倒序；末尾提示用 read_document 传 doc_id 读全文。库内无文档时返回提示文本。",
    inputSchema: {
      type: "object",
      properties: {
        kb_id: { type: "string", description: "知识库 id" },
      },
      required: ["kb_id"],
      additionalProperties: false,
    },
  },
  {
    name: "rebuild_index",
    description:
      "重建索引：按知识库当前嵌入模型重新向量化全部分片（切换嵌入模型后必做）",
    returns:
      "文本：索引重建完成 + 嵌入模型 / 文档数 / 分片数 / 已向量化数 / 过期(stale) 数 / token 总数 / 索引是否完整。",
    inputSchema: {
      type: "object",
      properties: {
        kb_id: { type: "string", description: "知识库 id" },
      },
      required: ["kb_id"],
      additionalProperties: false,
    },
  },
];
