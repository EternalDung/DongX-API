# DongX 需求文档

> 版本 v0.1 · 创建 2026-08-21 · 作者：WorkBuddy × 用户
> 状态：草案（待多轮讨论完善）

---

## 1. 项目概述

### 1.1 产品定位

DongX 是一个**本地运行的 LLM API 网关桌面软件**。核心职责是将各家 LLM 供应商的非统一 API **统一转换为 OpenAI 兼容协议**，让 Cursor、VSCode 插件、ChatBox、脚本等下游工具只需对接一套标准 OpenAI 接口即可调度任意上游模型。

### 1.2 与 dongapi 的关系

DongX 是 dongapi（D:\dongapi）的设计重构版。dongapi 作为前期探索积累了架构经验和前端骨架，DongX 在此基础上：
- 重新组织后端模块划分，更清晰的职责边界
- 安全审计作为一等公民纳入初始设计
- 文档先行，需求驱动开发

### 1.3 对标产品

| 产品 | 参考价值 |
|------|---------|
| CC Switch | 桌面端 Provider 配置管理、本地路由、密钥本地存储、Token 成本追踪 |
| One-API / New-API | 网关协议转换、多渠道管理、负载均衡（Web 端，DongX 取其内核思路做桌面版） |
| LiteLLM | 统一 SDK 协议转换思路 |

### 1.4 核心价值

- **协议统一**：下游工具只对接 OpenAI 协议，网关负责转写
- **渠道聚合**：一个入口管理多个供应商、多个密钥
- **负载均衡**：按权重分发、按优先级故障转移
- **本地安全**：密钥加密存储，数据不出本机
- **可观测**：请求日志、Token 统计、安全审计

---

## 2. 核心功能需求

### 2.1 功能模块总览

| # | 模块 | 优先级 | 说明 |
|---|------|--------|------|
| F1 | 多渠道管理 | P0 | 渠道 CRUD、协议适配、模型映射、优先级/权重 |
| F2 | 密钥管理 | P0 | 上游密钥加密存储 + 网关密钥分发管理 |
| F3 | 网关转发引擎 | P0 | OpenAI 兼容端点代理、流式 SSE、协议转写 |
| F4 | 负载均衡 | P0 | 加权轮询 + 优先级故障转移 |
| F5 | 请求日志 | P1 | 逐条请求明细、请求/响应体查看、筛选导出 |
| F6 | 仪表盘 | P1 | 请求统计、Token 消耗可视化 |
| F7 | 安全审计 | P1 | 风控规则、异常告警、审计事件记录 |
| F8 | 设置中心 | P1 | 主题/端口/自启/日志保留等系统配置 |
| F9 | 配置导入导出 | P2 | 配置备份恢复、跨设备迁移 |

### 2.2 F1 多渠道管理

**渠道（Channel）**= 一个上游供应商的接入点，包含协议类型、Base URL、密钥、支持的模型列表等。

#### 2.2.1 协议分层

顶层选「协议族」，再选「提供商预设」，最后填具体配置：

| 协议族 | 端点 | 提供商预设 |
|--------|------|-----------|
| OpenAI 兼容 | `/v1/chat/completions`, `/v1/completions`, `/v1/embeddings`, `/v1/models` | OpenAI, DeepSeek, 智谱, 豆包, 自定义 |
| Anthropic | `/v1/messages` | Claude |
| Ollama | `/api/chat`, `/api/generate` | Ollama (本地) |

#### 2.2.2 渠道字段

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | UUID |
| name | TEXT | 渠道名称 |
| protocol | TEXT | openai \| anthropic \| ollama |
| type | TEXT | 提供商类型：openai, deepseek, claude, gemini, zhipu, ollama, custom |
| base_url | TEXT | 上游基础 URL |
| cred_encrypted | TEXT | 加密后的上游 API Key（不存明文） |
| models | JSON | 支持的模型列表 |
| status | INT | 0=停用 1=启用 2=异常 |
| priority | INT | 故障转移顺序（数字越小越优先） |
| weight | INT | 负载均衡权重（同优先级内按权重分发） |
| config | JSON | 扩展配置（超时、自定义头、代理等） |
| model_mapping | JSON | 外部模型名 → 上游实际模型名 |
| endpoints | JSON | 启用的端点路径列表 |
| created_at | TEXT | ISO8601 |
| updated_at | TEXT | ISO8601 |
| last_test_at | TEXT | 最近连通性测试时间 |
| last_test_ok | INT | 0/1/null |

#### 2.2.3 功能点

- CRUD 渠道（创建、编辑、删除、列表查看）
- 协议三选一 → 提供商预设带出默认 Base URL
- 端点勾选（至少选一个）
- 模型映射配置（外部模型名 → 上游模型名）
- 启用/禁用切换
- 连通性测试（打真实端点探测，回填 last_test_ok）
- 优先级 + 权重配置

### 2.3 F2 密钥管理

#### 2.3.1 上游密钥

- 录入时 AES-GCM 加密存储，密钥材料优先用系统密钥环（Windows DPAPI）
- 界面只显示掩码（`sk-****a1b2`）
- 复制功能（clipboard 插件）
- 每个渠道可绑定一个上游密钥

#### 2.3.2 网关密钥（Gateway Key / ApiKey）

网关自身分发给下游客户端的访问凭证。

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | UUID |
| name | TEXT | 密钥名称 |
| key | TEXT | 脱敏展示串 `sk-dong-****a1b2` |
| key_hash | TEXT | 哈希值（bcrypt/argon2），永不存明文 |
| status | INT | 0=停用 1=正常 2=已过期 |
| allowed_models | JSON | 允许调用的模型列表（空=不限制） |
| allowed_channels | JSON | 允许调用的渠道列表（空=不限制） |
| quota_limit | INT | Token 配额上限（0=不限） |
| quota_used | INT | 已用 Token 数 |
| expires_at | TEXT | 过期时间（可空） |
| created_at | TEXT | |
| updated_at | TEXT | |

#### 2.3.3 功能点

- 生成 `sk-dong-<random>` 格式的网关密钥
- 创建时返回明文（仅此一次），后续只展示掩码
- 启用/禁用/删除
- 配额管理（累计 usage.total_tokens，超限自动禁用）
- 过期管理
- 模型/渠道绑定

### 2.4 F3 网关转发引擎

#### 2.4.1 OpenAI 兼容端点

| 端点 | 方法 | 说明 |
|------|------|------|
| `/v1/chat/completions` | POST | 对话补全（支持 SSE 流式） |
| `/v1/completions` | POST | 文本补全 |
| `/v1/embeddings` | POST | 向量嵌入 |
| `/v1/models` | GET | 模型列表 |
| `/health` | GET | 健康检查 |

#### 2.4.2 转发流程

```
客户端 POST /v1/chat/completions (带网关密钥)
  │
  ├─ 1. 鉴权：校验网关密钥 → 查 gateway_keys 表
  ├─ 2. 配额检查：quota_used vs quota_limit
  ├─ 3. 渠道选择：按 model + model_mapping 匹配渠道（考虑优先级/权重/故障转移）
  ├─ 4. 协议转写：OpenAI 格式 → 上游格式（如 Anthropic messages 结构）
  ├─ 5. 转发：Reqwest 发请求到 base_url + endpoint，注入上游 api_key
  ├─ 6. 响应回流：SSE 流式回传客户端
  ├─ 7. 落库：异步写 request_logs（token、延迟、状态）
  └─ 8. 风控：异常时写 audit_events + notification
```

#### 2.4.3 流式 SSE 支持

- 透传上游 SSE 流（`text/event-stream`）
- 边流边累计 token 用量（解析 SSE chunk 中的 usage）
- 流结束后异步落库

### 2.5 F4 负载均衡

#### 2.5.1 调度策略

| 策略 | 说明 |
|------|------|
| 优先级故障转移 | 按 priority 排序，优先用高优先级渠道；异常时自动降级到下一优先级 |
| 加权轮询 | 同优先级内，按 weight 比例分发请求 |
| 熔断器 | 单渠道连续失败 N 次 → 标记异常（status=2）→ 移出候选 → 冷却后自动恢复探测 |

#### 2.5.2 渠道选择算法

```
1. 过滤：status=1（启用）且支持请求的 model
2. 排序：按 priority 升序
3. 取最高优先级组
4. 组内按 weight 加权随机选择
5. 请求失败 → 标记渠道失败计数 +1
6. 失败计数 ≥ 阈值 → 熔断，尝试同组其他渠道
7. 同组全部熔断 → 降级到下一优先级组
```

### 2.6 F5 请求日志

#### 2.6.1 日志字段

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | UUID |
| seq | INT | 自增序号 |
| api_key_name | TEXT | 调用方密钥名称（脱敏） |
| channel_name | TEXT | 实际服务的上游渠道名称 |
| model | TEXT | 网关侧模型名 |
| upstream_model | TEXT | 转发到上游时的实际模型名 |
| mode | TEXT | chat \| completion \| embedding \| other |
| status_code | INT | HTTP 状态码 |
| prompt_tokens | INT | 输入 Token |
| completion_tokens | INT | 输出 Token |
| total_tokens | INT | 总 Token |
| duration_ms | INT | 端到端耗时 |
| error_message | TEXT | 错误信息（可空） |
| is_stream | INT | 是否流式 |
| is_retry | INT | 是否重试 |
| created_at | TEXT | ISO8601 |
| request_body | TEXT | 请求体（可配置是否记录） |
| response_body | TEXT | 响应体（可配置是否记录） |
| risk_level | TEXT | none \| low \| medium \| high \| critical |
| risk_score | INT | 0~100 |
| risk_summary | TEXT | 风险摘要 |
| security_action | TEXT | allow \| sanitize \| flag \| block |
| sanitized | INT | 是否已脱敏 |
| blocked_reason | TEXT | 拦截原因（仅 block 时） |

#### 2.6.2 功能点

- 列表分页 + 筛选（时间/渠道/状态/模型/密钥）
- 详情查看：请求体/响应体 JSON 高亮
- 日志保留天数可配置（过期自动清理）
- 可配置是否记录 raw 请求/响应体（性能与隐私权衡）

### 2.7 F6 仪表盘

- 今日请求数、成功率、平均延迟、今日 Token 消耗
- 总请求数、总 Token、活跃渠道数
- 按渠道/模型/时间维度的统计
- 简单图表（CSS 条形或轻量图表库，暂不强制引入）

### 2.8 F7 安全审计

#### 2.8.1 审计事件

| 字段 | 类型 | 说明 |
|------|------|------|
| id | TEXT PK | UUID |
| timestamp | TEXT | ISO8601 |
| type | TEXT | rate_limit \| invalid_key \| quota_exhaust \| suspicious \| config_change |
| severity | TEXT | info \| warning \| critical |
| actor | TEXT | 涉及的网关密钥/操作者 |
| message | TEXT | 事件描述 |
| meta | JSON | 扩展元数据 |

#### 2.8.2 风控规则

- 单密钥限流（每分钟最大请求数）
- 总配额耗尽告警
- 异常流量检测（短时高频/错误率突增）
- 触发时写 audit_events + notification 弹窗
- 审计时间线按严重级别筛选

#### 2.8.3 安全模式

| 模式 | 说明 |
|------|------|
| strict | 严格扫描：Unicode 注入、工具调用、网络请求、响应内容均扫描，命中即拦截 |
| balanced | 平衡模式：扫描但不拦截，仅标记 |
| permissive | 宽松模式：仅记录，不扫描 |

### 2.9 F8 设置中心

| 设置项 | 类型 | 说明 |
|--------|------|------|
| server_port | INT | Axum 监听端口（默认 9842） |
| server_host | TEXT | 监听地址（默认 127.0.0.1） |
| ui_theme | TEXT | light \| dark \| system |
| ui_language | TEXT | 语言（初期仅中文） |
| minimize_to_tray | BOOL | 最小化到托盘 |
| close_to_tray | BOOL | 关闭到托盘 |
| auto_start | BOOL | 开机自启 |
| retry_enabled | BOOL | 启用自动重试 |
| retry_times | INT | 重试次数 |
| log_retention_days | INT | 日志保留天数（默认 30） |
| log_raw_body | BOOL | 是否记录请求/响应原始体 |
| security_enabled | BOOL | 启用安全扫描 |
| security_mode | TEXT | strict \| balanced \| permissive |

### 2.10 F9 配置导入导出（P2）

- 导出全量配置为 JSON（渠道、密钥脱敏、设置）
- 导入配置并合并
- 密钥导出时可选是否包含（默认不含）

---

## 3. 技术架构

### 3.1 双层架构

```
┌─────────────────────────────────────────────────────────────┐
│  外部客户端 (Cursor / VSCode / ChatBox / OpenAI SDK / 脚本)  │
│           │  HTTP (OpenAI 兼容 API)                           │
│           ▼                                                 │
│  ┌──────────────────────────────────────────┐               │
│  │  Axum HTTP Server (localhost:port)        │  ← 数据面     │
│  │  /v1/chat/completions 等                  │               │
│  │  鉴权 → 路由 → 负载均衡 → Reqwest 转发上游  │               │
│  │  写请求日志 / 统计 / 审计事件               │               │
│  └────────────────┬─────────────────────────┘               │
│                   │ (同进程内调用)                             │
│  ┌────────────────┴─────────────────────────┐               │
│  │  Tauri invoke commands (IPC)              │  ← 管理面     │
│  │  渠道CRUD / 密钥CRUD / 日志查询 / 设置读写   │               │
│  │  (均经 SQLx 访问 SQLite)                   │               │
│  └────────────────┬─────────────────────────┘               │
│                   │ Tauri IPC                               │
│  ┌────────────────┴─────────────────────────┐               │
│  │  React 19 + TS 前端 (WebView)             │               │
│  │  仪表盘 / 渠道管理 / 密钥 / 日志 / 审计 / 设置│              │
│  └──────────────────────────────────────────┘               │
└─────────────────────────────────────────────────────────────┘
         ↓ 转发到各供应商
┌─────────────────────────────────────────────────────────────┐
│  OpenAI │ DeepSeek │ Claude │ Gemini │ 智谱 │ Ollama ...     │
└─────────────────────────────────────────────────────────────┘
```

### 3.2 架构原则

- **数据面**（Axum）：对外暴露 OpenAI 兼容 HTTP API，供外部工具调用
- **管理面**（Tauri invoke）：前端通过 IPC 调 Rust 命令做管理操作，不走 HTTP
- **数据库单一所有者**：后端 SQLx 统一拥有所有 DB 访问，前端不直接碰 SQLite
- **同进程**：Axum 和 Tauri 在同一 Rust 进程内，共享数据库连接池

### 3.3 后端模块划分

```
src-tauri/src/
├── main.rs                      # 入口
├── lib.rs                       # Tauri Builder + 插件注册 + Axum spawn
├── server/                      # 【数据面】Axum HTTP 服务器
│   ├── mod.rs                   #   模块入口
│   ├── router.rs                #   路由定义
│   ├── handler.rs               #   请求处理（chat/completions 等）
│   └── auth.rs                  #   网关密钥校验
├── core/                        # 【核心引擎】调度与转发
│   ├── mod.rs
│   ├── dispatcher.rs            #   请求分发（渠道选择）
│   ├── balancer.rs              #   负载均衡（加权轮询 + 熔断器）
│   └── proxy.rs                 #   Reqwest 转发 + 流式 SSE
├── adapter/                     # 【协议适配】各供应商格式转写
│   ├── mod.rs
│   ├── openai.rs                #   OpenAI 兼容（直通）
│   ├── anthropic.rs             #   Anthropic 格式转写
│   └── ollama.rs                #   Ollama 格式转写
├── commands/                    # 【管理面】Tauri invoke 命令
│   ├── mod.rs
│   ├── channel.rs               #   渠道 CRUD
│   ├── key.rs                   #   密钥 CRUD
│   ├── log.rs                   #   日志查询
│   ├── audit.rs                 #   审计事件查询
│   └── settings.rs              #   设置读写
├── db/                          # 【持久化】SQLx 连接池 + migrations
│   ├── mod.rs
│   └── schema.rs
├── models/                      # 【数据模型】Serde 结构体
│   └── mod.rs
├── crypto/                      # 【加密】密钥加密存储
│   └── mod.rs
├── security/                    # 【安全审计】风控与脱敏
│   ├── mod.rs
│   ├── scanner.rs               #   规则扫描
│   ├── redact.rs                #   敏感信息脱敏
│   └── audit.rs                 #   审计事件写入
├── config/                      # 【配置】应用配置
│   └── mod.rs
└── error.rs                     # 【错误】统一错误类型
```

---

## 4. 数据库设计

### 4.1 ER 关系

```
channels 1───* gateway_keys (allowed_channels)
channels 1───* request_logs (channel_name)
gateway_keys 1───* request_logs (api_key_name)
gateway_keys 1───* audit_events (actor)
settings (独立 KV 表)
```

### 4.2 表结构

详细 SQL 见 `src-tauri/migrations/001_init.sql`，包含：
- `channels` — 上游渠道
- `gateway_keys` — 网关密钥
- `request_logs` — 请求日志
- `audit_events` — 安全审计
- `settings` — 系统设置（KV）

### 4.3 设计原则

- 上游密钥与网关密钥分离存储
- 日志与审计独立，便于风控分析
- 时间戳统一 ISO8601 文本（SQLite 无原生 datetime）
- JSON 字段用 TEXT 存储，应用层 Serde 序列化
- 前后端 wire 格式统一 snake_case

---

## 5. API 设计

### 5.1 数据面 API（Axum HTTP）

| 路径 | 方法 | 鉴权 | 说明 |
|------|------|------|------|
| `/v1/chat/completions` | POST | Bearer sk-dong-* | 对话补全（支持 SSE） |
| `/v1/completions` | POST | Bearer sk-dong-* | 文本补全 |
| `/v1/embeddings` | POST | Bearer sk-dong-* | 向量嵌入 |
| `/v1/models` | GET | Bearer sk-dong-* | 模型列表 |
| `/health` | GET | 无 | 健康检查 |

### 5.2 管理面 API（Tauri invoke 命令）

| 命令 | 说明 |
|------|------|
| `list_channels` | 列出所有渠道 |
| `create_channel` | 创建渠道 |
| `update_channel` | 更新渠道 |
| `delete_channel` | 删除渠道 |
| `test_channel` | 测试渠道连通性 |
| `list_provider_presets` | 列出提供商预设 |
| `list_api_keys` | 列出网关密钥 |
| `create_api_key` | 创建网关密钥 |
| `update_api_key` | 更新网关密钥 |
| `delete_api_key` | 删除网关密钥 |
| `list_logs` | 查询请求日志 |
| `get_log_detail` | 日志详情 |
| `clear_logs` | 清理日志 |
| `list_audit_events` | 查询审计事件 |
| `get_settings` | 读取设置 |
| `update_settings` | 更新设置 |
| `get_dashboard_stats` | 仪表盘统计 |
| `get_usage_summary` | 用量统计 |

---

## 6. 安全需求

### 6.1 密钥安全

- 上游密钥：AES-GCM 加密落库，密钥材料优先用系统密钥环（Windows DPAPI）
- 网关密钥：只存哈希（bcrypt/argon2），永不存明文
- 网关密钥创建时返回明文一次，后续只展示掩码
- 传输仅限 localhost

### 6.2 网络安全

- Axum 只监听 127.0.0.1（不暴露到外网）
- Tauri WebView CSP 收紧（禁止 csp: null）
- SQL 一律参数化（SQLx 编译时校验天然防注入）

### 6.3 审计安全

- 审计事件独立存储，不可篡改（append-only 语义）
- 敏感信息脱敏后再落日志
- 所有管理操作（创建/删除/修改渠道密钥）记录审计事件

### 6.4 风控安全

- 单密钥限流
- 配额耗尽自动禁用
- 异常流量检测与告警
- 安全模式可切换（strict/balanced/permissive）

---

## 7. 非功能需求

| 维度 | 要求 |
|------|------|
| 性能 | 单请求转发延迟 < 50ms（不含上游响应时间）；SSE 流式无缓冲透传 |
| 可靠性 | 单渠道异常不影响其他渠道；熔断器自动隔离故障渠道 |
| 可维护性 | 模块化设计，职责清晰；文档与代码同步 |
| 可移植性 | Windows 优先，macOS/Linux 可编译 |
| 用户体验 | 暗色/浅色主题；响应式布局；操作反馈即时 |
| 数据安全 | 密钥加密存储；日志可配置脱敏；仅 localhost 监听 |
| 离线可用 | 无网络依赖（除上游 API 调用）；本地 SQLite 持久化 |

---

## 8. 待讨论问题

> 以下问题在文档中给出默认方案，欢迎多轮讨论调整。

### Q1 Axum 服务器生命周期

**默认方案**：Axum 随 Tauri 应用启动而启动；窗口关闭时最小化到系统托盘，保持代理运行；端口冲突时自动 +1 重试（最多 10 次），仍失败则报错让用户改端口。

**待确认**：是否采用「托盘常驻」模式？还是窗口关闭即停止代理？

### Q2 初始支持的供应商范围

**默认方案**：P0 阶段支持 OpenAI 兼容（含 DeepSeek/智谱/豆包等 OpenAI 兼容服务）、Anthropic、Ollama 三大协议族。

**待确认**：是否需要额外支持 Gemini 原生协议？还是通过 OpenAI 兼容模式接入？

### Q3 网关自身鉴权

**默认方案**：网关要求客户端携带 `sk-dong-*` 密钥鉴权；可配置关闭鉴权（仅本地信任环境）。

**待确认**：是否需要支持「无鉴权模式」？还是强制鉴权？

### Q4 熔断器参数

**默认方案**：连续失败 3 次 → 熔断 30 秒 → 半开探测 → 成功则恢复，失败则继续熔断。

**待确认**：阈值和冷却时间是否合理？是否需要可配置？

### Q5 密钥加密方案

**默认方案**：AES-GCM + Windows DPAPI（系统密钥环）；跨平台时用 keyring crate。

**待确认**：是否接受依赖系统密钥环？还是用本地派生密钥（密码派生）？

### Q6 模型映射复杂度

**默认方案**：简单的 1:1 映射（外部模型名 → 上游模型名）。

**待确认**：是否需要支持多对一、正则匹配等复杂映射？

### Q7 速率限制粒度

**默认方案**：按网关密钥限流（每分钟最大请求数）。

**待确认**：是否需要按渠道限流？按 IP 限流？

### Q8 成本追踪

**默认方案**：记录 Token 用量，不做货币换算（各供应商定价变动频繁）。

**待确认**：是否需要内置定价表做成本估算？还是仅记录 Token？

### Q9 i18n

**默认方案**：初期仅中文，预留不做。

**待确认**：是否需要多语言支持？

### Q10 图表库

**默认方案**：初期用 CSS 条形/数字卡片，不引入图表库；后续按需引入 Recharts。

**待确认**：是否初期就需要引入图表库？

---

## 9. 术语表

| 术语 | 含义 |
|------|------|
| 上游渠道 (Channel) | 实际提供模型的供应商接入点 |
| 网关密钥 (Gateway Key / ApiKey) | 本软件下发给下游客户端的访问凭证 |
| 数据面 | 对外代理流量的 Axum HTTP 服务 |
| 管理面 | 前端经 invoke 配置系统的内部通道 |
| 模型映射 | 外部模型名到上游实际模型名的转换 |
| 熔断器 | 连续失败后自动隔离故障渠道的机制 |
| 协议族 | 供应商 API 的协议分类（OpenAI/Anthropic/Ollama） |
