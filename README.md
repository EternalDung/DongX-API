# DongX

> 本地 LLM API 网关 · 个人项目（Side Project）

DongX 是一个运行在本机的 LLM API 网关桌面应用。它把多个提供商的 API 收拢成统一的 OpenAI 兼容端点，在本地完成密钥管理、加权路由、故障转移与安全审计——所有数据留在你的机器上，不依赖任何外部服务。

## 功能特性

- **统一接入**：OpenAI / Anthropic / Gemini（原生 API）协议适配，对外暴露 OpenAI 兼容的 `/v1/chat/completions` 与 `/v1/responses`，流式与非流式均支持。
- **加权路由**：同一提供商下多个模型按权重分配流量；每条密钥支持独立 token 预算，超额自动禁用并移出路由候选。
- **高可用**：渠道级熔断（连续失败自动冷却）+ 单次请求内自动换渠道重试，上游抖动对调用方透明。
- **安全审计**
  - 25 条内置检测规则（凭证、PII、支付卡、命令注入、Unicode 隐写、网络外联等），按类目分组、可单独开关与调整等级。
  - 自定义黑名单规则，命中可告警或阻断。
  - 请求 / 响应 / 流式增量三阶段审计，证据脱敏落库并保留取证哈希（不落明文）。
  - 网关级限流（RPM），超限返回 429。
- **本地优先**：密钥加密存储于本机 SQLite；配置、日志、审计数据均不离开本机。

## 架构

双层结构：

- **管理面（Tauri invoke）**：前端 React 通过 Tauri 命令读写配置、密钥、渠道、规则与日志。
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

- 前端：React 19 · TypeScript · Vite · TailwindCSS · shadcn/ui
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

## License

[MIT](./LICENSE)
