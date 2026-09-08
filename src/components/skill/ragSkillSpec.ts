// 静态技能包契约：RAG 工具清单（name / description / 入参 schema / 出参描述）。
// 参数稳定、不随运行态变化，故写死于此，作为「展示 + 导出」单一事实源，
// 与 waliapi 的纯前端 + 技能包模式一致。工具增减时手动同步此处即可。
// 端点 URL 仍由后端 get_mcp_status 实时提供（见 SkillTab）。

export interface JsonProp {
  type?: string;
  description?: string;
  default?: unknown;
  minimum?: number;
  maximum?: number;
}

export interface RagToolSpec {
  name: string;
  description: string;
  /** 出参结构描述（文本），供 Skill 页与导出 SKILL.md 展示 */
  returns: string;
  inputSchema: {
    type: string;
    properties: Record<string, JsonProp>;
    required: string[];
    additionalProperties?: boolean;
  };
}

export const ragSkills: RagToolSpec[] = [
  {
    name: "search_knowledge_base",
    description: "语义检索 RAG，返回匹配文本片段和相似度评分",
    returns:
      "文本。格式：知识库「名称」Top-N 命中，每条含序号、文档标题、相似度分数(0~1)、片段正文（截断 320 字）。未命中返回提示文本。",
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
          description: "用于生成回答的 chat 模型（可省略，使用任意可用模型由网关分发）",
        },
      },
      required: ["kb_id", "question"],
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
];
