# DongX

> 本地 LLM API 网关 · 个人项目（Side Project）

DongX 是一个运行在本机的 LLM API 网关桌面应用。它把多个提供商的 API 收拢成统一的 OpenAI 兼容端点，在本地完成密钥管理、加权路由、故障转移与安全审计——所有数据留在你的机器上，不依赖任何外部服务。

> English documentation: see [README.en.md](./README.en.md).

## 功能特性

### 统一接入与协议适配
- 支持 **OpenAI / Anthropic / Gemini（原生 API）** 三类协议适配，对外暴露 OpenAI 兼容的 `/v1/chat/completions` 与 `/v1/responses`（Responses API 模式）。
- **流式与非流式双管道**均已打通：OpenAI 系透传；Claude / Gemini 原生 SSE 经适配器逐块转换为 OpenAI SSE；Responses 模式由 Chat SSE 逐帧转换。
- 渠道选择器按协议（OpenAI / Anthropic / Ollama）主动选择；提供商下拉按协议过滤，同一厂商可跨协议挂载（如 DeepSeek 同时挂 OpenAI 与 Anthropic 兼容口）。

### 加权路由与负载均衡
- 同一提供商下多个模型按**权重**分配流量。
- 每条密钥支持**独立 token 预算**，累加响应 `usage.total_tokens`，超额自动禁用并移出路由候选。
- 多 key **加权负载均衡**：`keys: Vec<{key,weight}>` 加密存储，按权重分发。

### 高可用：熔断与故障转移
- **渠道级熔断**：连续失败达阈值（默认 3 次）自动冷却（默认 60s），冷却中渠道不参与分发。
- **请求内故障转移（Failover 状态机）**：单次请求内自动换渠道重试；可重试失败（5xx / 429 / 408 / 409 / 连接超时）自动换下个候选渠道，4xx 不重试；重试上限 = `retry_times + 1`。
- 上游抖动对调用方透明。

### 安全审计
- **25 条内置检测规则**，覆盖凭证、个人信息（PII）、支付卡、命令注入、Unicode 隐写、网络外联、工具风险、提示注入等类目；按类目分组，可单独开关与调整严重等级。
- **自定义黑名单规则**：命中可告警（warn）或阻断（block）。
- **三阶段审计**：请求体 / 响应体 / 流式增量（response_delta）三阶段扫描，流式阶段只记录不阻断。
- **证据脱敏 + 取证哈希**：命中证据脱敏后落库，同时计算 SHA-256 `evidence_hash` 用于跨阶段去重与溯源，**全程不落明文**。
- **网关级限流**：按网关密钥 RPM 限流，超限返回 `429`。
- 安全闸门 4 模式：`audit`（仅审计）/ `warn`（告警）/ `redact`（脱敏转发）/ `block`（阻断）。
- 设置页内置规则管理 UI：分组 / 搜索 / 严重度调整 / 开关 / 恢复默认 / 一键折叠 / 与全局开关门控联动 / 已启用计数。

### 密钥与配置管理
- 密钥加密存储于本机 SQLite；支持明文复制、启停、按渠道多 key 管理。
- 设置项含服务器地址 / 端口、主题、语言、托盘行为、限流与重试策略、安全审计开关与模式。

### 日志与可观测
- `request_logs` 记录每次请求 / 响应、token 用量、是否重试。
- 安全 findings 详情可在日志详情中查看（含脱敏证据与取证哈希）。

### 本地优先
- 所有配置、日志、审计数据均不离开本机；密钥库位于 `%APPDATA%\com.wei.dongx\`（Windows）。
- 不依赖任何外部服务或云端。

## 架构

双层结构：

- **管理面（Tauri invoke）**：前端 React 通过 Tauri 命令读写配置、密钥、渠道、规则与日志（17 个命令）。
- **数据面（Axum）**：独立 HTTP 服务监听 `127.0.0.1:9842`，承担 OpenAI 兼容代理管道（鉴权 → 分发 → 协议转换 → 转发 → 落日志 / 审计）。

```
┌────────────┐     Tauri invoke      ┌──────────────────┐
│  React UI  │ ───────────────────▶ │  管理面 (Rust)    │
└────────────┘                      └──────────────────┘
       │  OpenAI-compatible HTTP
       ▼
┌──────────────────────────────────────────────┐
│  Axum 数据面  (127.0.0.1:9842)                 │
│  auth → dispatcher → adapter → upstream        │
│        └─ failover / circuit-breaker / audit   │
└──────────────────────────────────────────────┘
```

## 技术栈

- 前端：React 19 · TypeScript · Vite 7 · TailwindCSS 4 · shadcn/ui · lucide-react
- 后端：Rust · Tauri 2 · Axum · SQLite (sqlx) · reqwest

## 快速开始

```bash
# 安装前端依赖
npm install

# 开发模式（同时起前端 + Rust 后端）
cargo tauri dev

# 产出桌面安装包
cargo tauri build
```

> 运行数据位于 `%APPDATA%\com.wei.dongx\`（Windows），包含配置、SQLite 数据库与日志。

## 说明

- 本项目为个人学习 / 演示用途，按自有需求构建，非任何商业或委托项目。
- 安全审计规则与网关逻辑均为本地运行；请勿将网关监听地址暴露到公网。
- 当前构建产物未签名，Windows 安装时可能提示 SmartScreen「未知发布者」，点「仍要运行」即可；macOS 未签名 app 首次打不开时，终端执行 `xattr -cr /Applications/DongX.app` 后从 Finder 右键打开。

## License

[MIT](./LICENSE)
