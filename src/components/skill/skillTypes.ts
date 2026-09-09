// 技能包契约的共享类型：RAG / Wiki 两个技能包共用同一套结构。
//
// 设计取舍（与 waliapi 一致）：工具参数稳定、不随运行态变化，故写死在前端
// spec 文件里，作为「页面展示 + zip 导出」的单一事实源。后端只负责在运行态
// 提供会变的那一件事——MCP 端点 URL（见 SkillTab 的 `status.endpoint`）。

export interface JsonProp {
  type?: string;
  description?: string;
  default?: unknown;
  minimum?: number;
  maximum?: number;
}

export interface SkillToolSpec {
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
