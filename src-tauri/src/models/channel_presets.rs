//! 渠道提供商模板 registry（唯一可信源）
//!
//! 本模块是前端、迁移、路由与草稿测试共用的 provider preset 单一数据来源
//! （设计 4.4）。它只定义纯类型与只读数据，不访问网络、不写入数据库。
//!
//! 设计文档：docs/channel-protocol-provider-refactor-design.md §2、§4.2、§4.4、§5.2
//! 任务规格：docs/channel-refactor-tasks/01-presets-and-domain-model.md
//!
//! 序列化稳定性：枚举字符串与设计 5.2 的 TS DTO 完全一致，不得随意改名。

use serde::{Deserialize, Serialize};

/// 协议：决定上游请求/响应格式、鉴权、测试方式与端点集合。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelProtocol {
    #[serde(rename = "openai")]
    OpenAI,
    Anthropic,
    Ollama,
}

impl ChannelProtocol {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChannelProtocol::OpenAI => "openai",
            ChannelProtocol::Anthropic => "anthropic",
            ChannelProtocol::Ollama => "ollama",
        }
    }
}

/// 渠道提供商：决定默认 Base URL、模型建议、地区分组与厂商提示。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ChannelProvider {
    #[serde(rename = "openai")]
    OpenAI,
    Google,
    #[serde(rename = "deepseek")]
    DeepSeek,
    Qwen,
    Zhipu,
    Doubao,
    #[serde(rename = "doubao_coding_plan")]
    DoubaoCodingPlan,
    Moonshot,
    Anthropic,
    Ollama,
    Custom,
}

impl ChannelProvider {
    pub fn as_str(&self) -> &'static str {
        match self {
            ChannelProvider::OpenAI => "openai",
            ChannelProvider::Google => "google",
            ChannelProvider::DeepSeek => "deepseek",
            ChannelProvider::Qwen => "qwen",
            ChannelProvider::Zhipu => "zhipu",
            ChannelProvider::Doubao => "doubao",
            ChannelProvider::DoubaoCodingPlan => "doubao_coding_plan",
            ChannelProvider::Moonshot => "moonshot",
            ChannelProvider::Anthropic => "anthropic",
            ChannelProvider::Ollama => "ollama",
            ChannelProvider::Custom => "custom",
        }
    }
}

/// 原生端点：描述该渠道上游真实提供的端点（T00 决策 9）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeEndpoint {
    ChatCompletions,
    Responses,
    Messages,
    CountTokens,
    Embeddings,
    ApiChat,
}

/// 鉴权方案：各厂商接受不同的凭据放置方式。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum AuthScheme {
    /// `Authorization: Bearer <key>`
    Bearer,
    /// `x-api-key: <key>`
    XApiKey,
    /// URL query 参数携带 key（仅旧 Google 原生配置）
    QueryKey,
    /// Bearer 可选（Ollama 本地默认无 Key）
    OptionalBearer,
}

/// 地区分组：产品分组，非服务器部署地域判断（设计 4.1）。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RegionGroup {
    /// 自定义配置，置顶、默认选中，不归入国际/国内/本地
    Custom,
    International,
    Domestic,
    Local,
}

/// 单个静态模型建议：必须可追溯到 `verified_at` + 官方 `source_url`。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModelSuggestion {
    pub id: String,
    /// 复核日期，格式 `YYYY-MM-DD`（2026-08-04 基线）
    pub verified_at: String,
    /// 官方模型目录/文档地址
    pub source_url: String,
}

/// 端点测试策略：草稿测试如何验证该预设的真实端点。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum EndpointTestStrategy {
    /// 用最小推理请求（stream:false + 最小输出上限）验证端点可用性
    ProbeFirstModel,
    /// 查询模型列表接口（OpenAI 兼容 `/models` / Ollama `/api/tags`）
    ListModels,
}

/// 模型枚举策略：新建引导时如何获得模型候选。
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModelEnumStrategy {
    /// 仅用静态建议（Anthropic 不假设存在兼容模型列表）
    StaticOnly,
    /// 静态建议 + 允许上游同步（OpenAI 兼容 `/models`）
    StaticPlusSync,
    /// 仅从上游枚举（Ollama `/api/tags`）
    SyncOnly,
}

/// 渠道提供商模板。所有字段序列化稳定；URL 为完整规范值，不做运行时猜厂商。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChannelPreset {
    /// 稳定 ID，格式 `{protocol}:{provider}`；custom 为 `{protocol}:custom`
    pub id: String,
    pub protocol: ChannelProtocol,
    pub provider: ChannelProvider,
    /// 显示名称（例如“字节豆包（Coding Plan）”“Ollama（本地）”）
    pub display_name: String,
    pub region: RegionGroup,
    pub description: String,
    /// 前端图标 key（`"openai" | "google" | ...`）
    pub icon_key: String,
    /// 新协议规范根 URL（UI 显示/编辑）
    pub native_base_url: String,
    /// 旧代码兼容根 URL（迁移期写回 `channels.base_url`）
    pub legacy_base_url: String,
    /// 旧适配器 `type`（写回 `channels.type`）
    pub legacy_type: String,
    /// 上游原生端点能力
    pub native_endpoints: Vec<NativeEndpoint>,
    /// 新建时默认勾选的端点
    pub default_checked_endpoints: Vec<NativeEndpoint>,
    pub auth_scheme: AuthScheme,
    pub model_suggestions: Vec<ModelSuggestion>,
    pub model_enum_strategy: ModelEnumStrategy,
    pub endpoint_test_strategy: EndpointTestStrategy,
    /// preset revision；保存渠道时记录供追溯，模板更新不覆盖已保存渠道
    pub preset_revision: String,
}

/// 每个协议返回一组：`presets[0]` 恒为 custom option（置顶、默认选中）。
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProtocolPresetGroup {
    pub protocol: ChannelProtocol,
    pub presets: Vec<ChannelPreset>,
}

/// 当前 registry revision（YYYY-MM-DD，2026-08-06 基线）。
pub const PRESET_REVISION: &str = "2026-08-06";

const SRC_OPENAI: &str = "https://platform.openai.com/docs/api-reference/chat";
const SRC_GEMINI_MODELS: &str = "https://ai.google.dev/gemini-api/docs/models";
const SRC_DEEPSEEK_FUNCTION_CALLING: &str =
    "https://api-docs.deepseek.com/guides/function_calling/";
const SRC_DEEPSEEK_ANTHROPIC: &str = "https://api-docs.deepseek.com/guides/anthropic_api";
const SRC_QWEN_ANTHROPIC: &str = "https://help.aliyun.com/en/model-studio/more-tools";
const SRC_QWEN_RESPONSES: &str =
    "https://help.aliyun.com/en/model-studio/qwen-api-via-openai-responses";
const SRC_ZHIPU: &str = "https://open.bigmodel.cn/dev/api";
const SRC_DOUBAO: &str = "https://www.volcengine.com/docs/82379/";
const SRC_MOONSHOT: &str = "https://platform.moonshot.ai/docs/api/chat";
const SRC_ANTHROPIC: &str = "https://docs.anthropic.com/en/api/messages";

/// 构建一个 preset 的便捷函数。
#[allow(clippy::too_many_arguments)]
fn preset(
    protocol: ChannelProtocol,
    provider: ChannelProvider,
    display_name: &str,
    region: RegionGroup,
    description: &str,
    icon_key: &str,
    native_base_url: &str,
    legacy_base_url: &str,
    legacy_type: &str,
    native_endpoints: Vec<NativeEndpoint>,
    default_checked_endpoints: Vec<NativeEndpoint>,
    auth_scheme: AuthScheme,
    model_suggestions: Vec<ModelSuggestion>,
    model_enum_strategy: ModelEnumStrategy,
    endpoint_test_strategy: EndpointTestStrategy,
) -> ChannelPreset {
    let id = format!("{}:{}", protocol.as_str(), provider.as_str());
    ChannelPreset {
        id,
        protocol,
        provider,
        display_name: display_name.to_string(),
        region,
        description: description.to_string(),
        icon_key: icon_key.to_string(),
        native_base_url: native_base_url.to_string(),
        legacy_base_url: legacy_base_url.to_string(),
        legacy_type: legacy_type.to_string(),
        native_endpoints,
        default_checked_endpoints,
        auth_scheme,
        model_suggestions,
        model_enum_strategy,
        endpoint_test_strategy,
        preset_revision: PRESET_REVISION.to_string(),
    }
}

fn model(id: &str, verified_at: &str, source_url: &str) -> ModelSuggestion {
    ModelSuggestion {
        id: id.to_string(),
        verified_at: verified_at.to_string(),
        source_url: source_url.to_string(),
    }
}

/// custom option：不提供默认 URL、密钥或模型；协议决定其允许端点。
fn custom_preset(protocol: ChannelProtocol) -> ChannelPreset {
    let (native_endpoints, default_checked, auth, strategy) = match protocol {
        ChannelProtocol::OpenAI => (
            vec![
                NativeEndpoint::ChatCompletions,
                NativeEndpoint::Responses,
                NativeEndpoint::Embeddings,
            ],
            vec![NativeEndpoint::ChatCompletions, NativeEndpoint::Embeddings],
            AuthScheme::Bearer,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        ChannelProtocol::Anthropic => (
            vec![NativeEndpoint::Messages],
            vec![NativeEndpoint::Messages],
            AuthScheme::XApiKey,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        ChannelProtocol::Ollama => (
            vec![NativeEndpoint::ApiChat],
            vec![NativeEndpoint::ApiChat],
            AuthScheme::OptionalBearer,
            EndpointTestStrategy::ProbeFirstModel,
        ),
    };
    preset(
        protocol,
        ChannelProvider::Custom,
        "自定义配置",
        RegionGroup::Custom,
        "手动配置协议与 Base URL，适用于私有网关或未内置厂商。",
        "custom",
        "",
        "",
        match protocol {
            ChannelProtocol::OpenAI | ChannelProtocol::Ollama => "openai",
            ChannelProtocol::Anthropic => "claude",
        },
        native_endpoints,
        default_checked,
        auth,
        vec![],
        ModelEnumStrategy::StaticOnly,
        strategy,
    )
}

fn openai_presets() -> Vec<ChannelPreset> {
    vec![
        preset(
            ChannelProtocol::OpenAI,
            ChannelProvider::OpenAI,
            "OpenAI",
            RegionGroup::International,
            "OpenAI 官方 API（Chat Completions 与 Responses）。",
            "openai",
            "https://api.openai.com/v1",
            "https://api.openai.com/v1",
            "openai",
            vec![
                NativeEndpoint::ChatCompletions,
                NativeEndpoint::Responses,
                NativeEndpoint::Embeddings,
            ],
            vec![
                NativeEndpoint::ChatCompletions,
                NativeEndpoint::Responses,
                NativeEndpoint::Embeddings,
            ],
            AuthScheme::Bearer,
            vec![
                model("gpt-5.2", PRESET_REVISION, SRC_OPENAI),
                model("gpt-5-mini", PRESET_REVISION, SRC_OPENAI),
                model("gpt-5-nano", PRESET_REVISION, SRC_OPENAI),
                model("gpt-4.1", PRESET_REVISION, SRC_OPENAI),
                model("gpt-4.1-mini", PRESET_REVISION, SRC_OPENAI),
            ],
            ModelEnumStrategy::StaticPlusSync,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::OpenAI,
            ChannelProvider::Google,
            "Google",
            RegionGroup::International,
            "Google Gemini 官方原生 API（generateContent），由 gemini 适配器做协议转换。",
            "google",
            "https://generativelanguage.googleapis.com",
            "https://generativelanguage.googleapis.com",
            "gemini",
            vec![NativeEndpoint::ChatCompletions],
            vec![NativeEndpoint::ChatCompletions],
            AuthScheme::QueryKey,
            vec![
                model("gemini-3.6-flash", PRESET_REVISION, SRC_GEMINI_MODELS),
                model("gemini-3.5-flash", PRESET_REVISION, SRC_GEMINI_MODELS),
                model("gemini-3.5-flash-lite", PRESET_REVISION, SRC_GEMINI_MODELS),
            ],
            ModelEnumStrategy::StaticPlusSync,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::OpenAI,
            ChannelProvider::DeepSeek,
            "DeepSeek",
            RegionGroup::Domestic,
            "DeepSeek 官方 OpenAI 接口。",
            "deepseek",
            "https://api.deepseek.com",
            "https://api.deepseek.com",
            "deepseek",
            vec![NativeEndpoint::ChatCompletions],
            vec![NativeEndpoint::ChatCompletions],
            AuthScheme::Bearer,
            vec![
                model(
                    "deepseek-v4-pro",
                    PRESET_REVISION,
                    SRC_DEEPSEEK_FUNCTION_CALLING,
                ),
                model(
                    "deepseek-v4-flash",
                    PRESET_REVISION,
                    SRC_DEEPSEEK_FUNCTION_CALLING,
                ),
            ],
            ModelEnumStrategy::StaticPlusSync,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::OpenAI,
            ChannelProvider::Qwen,
            "通义千问",
            RegionGroup::Domestic,
            "阿里云百炼 OpenAI 接口。",
            "qwen",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "https://dashscope.aliyuncs.com/compatible-mode/v1",
            "qwen",
            vec![NativeEndpoint::ChatCompletions, NativeEndpoint::Responses],
            vec![NativeEndpoint::ChatCompletions, NativeEndpoint::Responses],
            AuthScheme::Bearer,
            vec![
                model("qwen3.7-plus", PRESET_REVISION, SRC_QWEN_RESPONSES),
                model("qwen3.7-max", PRESET_REVISION, SRC_QWEN_RESPONSES),
                model("qwen3-coder-next", PRESET_REVISION, SRC_QWEN_RESPONSES),
            ],
            ModelEnumStrategy::StaticPlusSync,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::OpenAI,
            ChannelProvider::Zhipu,
            "智谱 GLM",
            RegionGroup::Domestic,
            "智谱 GLM OpenAI 接口（PAAS v4）。",
            "zhipu",
            "https://open.bigmodel.cn/api/paas/v4",
            "https://open.bigmodel.cn/api/paas/v4",
            "zhipu",
            vec![NativeEndpoint::ChatCompletions],
            vec![NativeEndpoint::ChatCompletions],
            AuthScheme::Bearer,
            vec![
                model("glm-4.7", PRESET_REVISION, SRC_ZHIPU),
                model("glm-4.7-flash", PRESET_REVISION, SRC_ZHIPU),
                model("glm-4.6v", PRESET_REVISION, SRC_ZHIPU),
            ],
            ModelEnumStrategy::StaticPlusSync,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::OpenAI,
            ChannelProvider::Doubao,
            "字节豆包 (Coding Plan)",
            RegionGroup::Domestic,
            "字节豆包官方 OpenAI 接口。",
            "doubao",
            "https://ark.cn-beijing.volces.com/api/v3",
            "https://ark.cn-beijing.volces.com/api/v3",
            "doubao",
            vec![NativeEndpoint::ChatCompletions],
            vec![NativeEndpoint::ChatCompletions],
            AuthScheme::Bearer,
            vec![
                model("doubao-seed-2-0-pro-260215", PRESET_REVISION, SRC_DOUBAO),
                model("doubao-seed-2-0-lite-260215", PRESET_REVISION, SRC_DOUBAO),
                model("doubao-seed-1-6", PRESET_REVISION, SRC_DOUBAO),
            ],
            ModelEnumStrategy::StaticPlusSync,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::OpenAI,
            ChannelProvider::Moonshot,
            "Moonshot(Kimi)",
            RegionGroup::Domestic,
            "Moonshot Kimi OpenAI 接口。",
            "moonshot",
            "https://api.moonshot.ai/v1",
            "https://api.moonshot.ai/v1",
            "moonshot",
            vec![NativeEndpoint::ChatCompletions],
            vec![NativeEndpoint::ChatCompletions],
            AuthScheme::Bearer,
            vec![
                model("kimi-k2.5", PRESET_REVISION, SRC_MOONSHOT),
                model("kimi-k2-thinking", PRESET_REVISION, SRC_MOONSHOT),
                model("kimi-k2-turbo-preview", PRESET_REVISION, SRC_MOONSHOT),
            ],
            ModelEnumStrategy::StaticPlusSync,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::OpenAI,
            ChannelProvider::Ollama,
            "Ollama（本地）",
            RegionGroup::Local,
            "本机或远程 Ollama 的 OpenAI 接口。",
            "ollama",
            "http://localhost:11434/v1",
            "http://localhost:11434/v1",
            "openai",
            vec![NativeEndpoint::ChatCompletions, NativeEndpoint::Responses],
            vec![NativeEndpoint::ChatCompletions, NativeEndpoint::Responses],
            AuthScheme::OptionalBearer,
            vec![],
            ModelEnumStrategy::SyncOnly,
            EndpointTestStrategy::ProbeFirstModel,
        ),
    ]
}

fn anthropic_presets() -> Vec<ChannelPreset> {
    vec![
        preset(
            ChannelProtocol::Anthropic,
            ChannelProvider::Anthropic,
            "Anthropic",
            RegionGroup::International,
            "Anthropic Claude Code 官方 Messages API。",
            "claudecode",
            "https://api.anthropic.com/v1",
            "https://api.anthropic.com/v1",
            "claude",
            vec![NativeEndpoint::Messages],
            vec![NativeEndpoint::Messages],
            AuthScheme::XApiKey,
            vec![
                model("claude-opus-4-6", PRESET_REVISION, SRC_ANTHROPIC),
                model("claude-sonnet-4-6", PRESET_REVISION, SRC_ANTHROPIC),
                model("claude-haiku-4-5-20251001", PRESET_REVISION, SRC_ANTHROPIC),
            ],
            ModelEnumStrategy::StaticOnly,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::Anthropic,
            ChannelProvider::DeepSeek,
            "DeepSeek",
            RegionGroup::Domestic,
            "DeepSeek 官方 Anthropic 接口。",
            "deepseek",
            "https://api.deepseek.com/anthropic/v1",
            "https://api.deepseek.com/anthropic/v1",
            "claude",
            vec![NativeEndpoint::Messages],
            vec![NativeEndpoint::Messages],
            AuthScheme::XApiKey,
            vec![
                model("deepseek-v4-pro", PRESET_REVISION, SRC_DEEPSEEK_ANTHROPIC),
                model("deepseek-v4-flash", PRESET_REVISION, SRC_DEEPSEEK_ANTHROPIC),
            ],
            ModelEnumStrategy::StaticOnly,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::Anthropic,
            ChannelProvider::Qwen,
            "通义千问",
            RegionGroup::Domestic,
            "阿里云百炼 Anthropic 接口。",
            "qwen",
            "https://dashscope.aliyuncs.com/apps/anthropic/v1",
            "https://dashscope.aliyuncs.com/apps/anthropic/v1",
            "claude",
            vec![NativeEndpoint::Messages],
            vec![NativeEndpoint::Messages],
            AuthScheme::XApiKey,
            vec![
                model("qwen3.7-plus", PRESET_REVISION, SRC_QWEN_ANTHROPIC),
                model("qwen3-coder-next", PRESET_REVISION, SRC_QWEN_ANTHROPIC),
            ],
            ModelEnumStrategy::StaticOnly,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::Anthropic,
            ChannelProvider::Zhipu,
            "智谱 GLM",
            RegionGroup::Domestic,
            "智谱 GLM 官方 Anthropic 接口。",
            "zhipu",
            "https://open.bigmodel.cn/api/anthropic/v1",
            "https://open.bigmodel.cn/api/anthropic/v1",
            "claude",
            vec![NativeEndpoint::Messages],
            vec![NativeEndpoint::Messages],
            AuthScheme::XApiKey,
            vec![
                model("glm-4.7", PRESET_REVISION, SRC_ZHIPU),
                model("glm-4.7-flash", PRESET_REVISION, SRC_ZHIPU),
            ],
            ModelEnumStrategy::StaticOnly,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::Anthropic,
            ChannelProvider::DoubaoCodingPlan,
            "字节豆包 (Coding Plan)",
            RegionGroup::Domestic,
            "字节豆包官方 Anthropic 接口。",
            "doubao_coding_plan",
            "https://ark.cn-beijing.volces.com/api/coding/v1",
            "https://ark.cn-beijing.volces.com/api/coding/v1",
            "claude",
            vec![NativeEndpoint::Messages],
            vec![NativeEndpoint::Messages],
            AuthScheme::XApiKey,
            // Coding Plan 当前开通模型：官方目录随接入点/区域变化（设计 4.2 备注）。
            // 2026-08-04 复核时未获得可追溯的官方型号清单，故不预置未经确认的 ID，
            // 由用户在保存前按官方 Coding Plan 控制台选择；避免「latest/preview」式猜测。
            vec![],
            ModelEnumStrategy::StaticOnly,
            EndpointTestStrategy::ProbeFirstModel,
        ),
        preset(
            ChannelProtocol::Anthropic,
            ChannelProvider::Ollama,
            "Ollama（本地）",
            RegionGroup::Local,
            "本机或远程 Ollama 的 Anthropic Messages 接口。",
            "ollama",
            "http://localhost:11434/v1",
            "http://localhost:11434/v1",
            "claude",
            vec![NativeEndpoint::Messages],
            vec![NativeEndpoint::Messages],
            AuthScheme::OptionalBearer,
            vec![],
            ModelEnumStrategy::SyncOnly,
            EndpointTestStrategy::ProbeFirstModel,
        ),
    ]
}

fn ollama_presets() -> Vec<ChannelPreset> {
    vec![preset(
        ChannelProtocol::Ollama,
        ChannelProvider::Ollama,
        "Ollama（本地）",
        RegionGroup::Local,
        "Ollama 原生 /api/chat 协议。",
        "ollama",
        "http://localhost:11434",
        "http://localhost:11434/v1",
        "openai",
        vec![NativeEndpoint::ApiChat],
        vec![NativeEndpoint::ApiChat],
        AuthScheme::OptionalBearer,
        vec![],
        ModelEnumStrategy::SyncOnly,
        EndpointTestStrategy::ProbeFirstModel,
    )]
}

/// 指定协议的全部 preset：custom 置顶，其后 international → domestic → local。
pub fn presets_for_protocol(protocol: ChannelProtocol) -> Vec<ChannelPreset> {
    let mut presets = vec![custom_preset(protocol)];
    let mut vendor: Vec<ChannelPreset> = match protocol {
        ChannelProtocol::OpenAI => openai_presets(),
        ChannelProtocol::Anthropic => anthropic_presets(),
        ChannelProtocol::Ollama => ollama_presets(),
    };
    vendor.sort_by_key(|p| region_order(p.region));
    presets.extend(vendor);
    presets
}

fn region_order(region: RegionGroup) -> u8 {
    match region {
        RegionGroup::Custom => 0,
        RegionGroup::International => 1,
        RegionGroup::Domestic => 2,
        RegionGroup::Local => 3,
    }
}

/// 按协议分组返回，供 `get_channel_presets()` 使用。
pub fn groups_for_protocols() -> Vec<ProtocolPresetGroup> {
    [
        ChannelProtocol::OpenAI,
        ChannelProtocol::Anthropic,
        ChannelProtocol::Ollama,
    ]
    .into_iter()
    .map(|protocol| ProtocolPresetGroup {
        protocol,
        presets: presets_for_protocol(protocol),
    })
    .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashSet;

    fn all_presets() -> Vec<ChannelPreset> {
        groups_for_protocols()
            .into_iter()
            .flat_map(|g| g.presets)
            .collect()
    }

    #[test]
    fn groups_cover_all_three_protocols() {
        let groups = groups_for_protocols();
        let protocols: Vec<ChannelProtocol> = groups.iter().map(|g| g.protocol).collect();
        assert_eq!(protocols.len(), 3);
        assert!(protocols.contains(&ChannelProtocol::OpenAI));
        assert!(protocols.contains(&ChannelProtocol::Anthropic));
        assert!(protocols.contains(&ChannelProtocol::Ollama));
        // 每个协议至少要有 custom + 一个厂商项
        for g in &groups {
            assert!(
                g.presets.len() >= 2,
                "协议 {:?} 的预设数量异常: {}",
                g.protocol,
                g.presets.len()
            );
        }
    }

    #[test]
    fn custom_preset_is_always_first() {
        for group in groups_for_protocols() {
            let first = group.presets.first().expect("每组至少一个 preset");
            assert_eq!(
                first.provider,
                ChannelProvider::Custom,
                "协议 {:?} 的自定义项未置顶",
                group.protocol
            );
            assert_eq!(first.region, RegionGroup::Custom);
            assert_eq!(first.id, format!("{}:custom", group.protocol.as_str()));
        }
    }

    #[test]
    fn preset_ids_are_globally_unique() {
        let mut seen = HashSet::new();
        for p in all_presets() {
            assert!(seen.insert(p.id.clone()), "重复 preset id: {}", p.id);
        }
    }

    #[test]
    fn vendor_presets_have_valid_urls_and_required_fields() {
        for p in all_presets() {
            if p.provider == ChannelProvider::Custom {
                continue; // 自定义项的 URL 由用户填写，允许为空
            }
            assert!(!p.display_name.is_empty(), "{} 缺 display_name", p.id);
            assert!(!p.icon_key.is_empty(), "{} 缺 icon_key", p.id);
            assert!(!p.legacy_type.is_empty(), "{} 缺 legacy_type", p.id);
            for url in [&p.native_base_url, &p.legacy_base_url] {
                assert!(
                    url.starts_with("https://") || url.starts_with("http://"),
                    "{} 的 URL 非法: {:?}",
                    p.id,
                    url
                );
            }
        }
    }

    #[test]
    fn default_checked_endpoints_are_subset_of_native_endpoints() {
        for p in all_presets() {
            assert!(
                !p.native_endpoints.is_empty(),
                "{} 未声明任何原生端点",
                p.id
            );
            for e in &p.default_checked_endpoints {
                assert!(
                    p.native_endpoints.contains(e),
                    "{} 默认勾选了未声明的端点 {:?}",
                    p.id,
                    e
                );
            }
        }
    }

    #[test]
    fn model_suggestions_are_documented_and_dated() {
        for p in all_presets() {
            for m in &p.model_suggestions {
                assert!(!m.id.is_empty(), "{} 存在空模型 id", p.id);
                assert!(
                    m.source_url.starts_with("https://"),
                    "{} 的模型 {} 缺少 https 官方来源: {}",
                    p.id,
                    m.id,
                    m.source_url
                );
                // verified_at 必须是 YYYY-MM-DD
                let d = &m.verified_at;
                assert_eq!(
                    d.len(),
                    10,
                    "{} 的 {} verified_at 应为 YYYY-MM-DD: {}",
                    p.id,
                    m.id,
                    d
                );
                assert_eq!(
                    &d[4..5],
                    "-",
                    "{} 的 {} verified_at 格式错误: {}",
                    p.id,
                    m.id,
                    d
                );
                assert_eq!(
                    &d[7..8],
                    "-",
                    "{} 的 {} verified_at 格式错误: {}",
                    p.id,
                    m.id,
                    d
                );
            }
        }
    }

    #[test]
    fn all_presets_carry_current_revision() {
        for p in all_presets() {
            assert_eq!(
                p.preset_revision, PRESET_REVISION,
                "{} 的 revision 未同步",
                p.id
            );
        }
    }

    #[test]
    fn presets_are_sorted_by_region() {
        for group in groups_for_protocols() {
            let seq: Vec<u8> = group
                .presets
                .iter()
                .map(|p| region_order(p.region))
                .collect();
            let mut sorted = seq.clone();
            sorted.sort_unstable();
            assert_eq!(
                seq, sorted,
                "协议 {:?} 的 preset 未按区域排序: {:?}",
                group.protocol, seq
            );
        }
    }
}
