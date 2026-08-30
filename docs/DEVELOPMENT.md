# DongX 开发文档

> 版本 v0.1 · 创建 2026-08-21 · 作者：WorkBuddy × 用户
> 状态：草案（随实现同步修订）

---

## 1. 项目结构

```
D:\DongX/
├── docs/                        # 文档
│   ├── REQUIREMENTS.md          #   需求文档
│   └── DEVELOPMENT.md           #   开发文档（本文件）
├── src/                         # 前端（React 19 + TS + Vite 7 + TailwindCSS 4）
│   ├── components/
│   │   ├── ui/                  #   shadcn/ui 组件（copy-source 模式）
│   │   └── layout/              #   布局组件（Layout/Sidebar/Topbar）
│   ├── pages/                   #   路由页面
│   ├── lib/                     #   工具库（api/format/theme/constants/utils）
│   ├── hooks/                   #   自定义 React Query Hooks
│   ├── store/                   #   Zustand 状态
│   ├── types/                   #   前后端共享契约类型
│   ├── assets/                  #   静态资源
│   ├── App.tsx                  #   根组件
│   ├── main.tsx                 #   入口
│   └── index.css                #   全局样式（Tailwind v4 令牌）
├── src-tauri/                   # 后端（Rust + Tauri 2 + Axum + SQLite + sqlx）
│   ├── src/
│   │   ├── main.rs              #   入口
│   │   ├── lib.rs               #   Tauri Builder + 插件注册 + Axum spawn
│   │   ├── server/              #   数据面（Axum HTTP 服务器）
│   │   ├── core/                #   核心引擎（调度/负载均衡/代理转发）
│   │   ├── adapter/             #   协议适配（OpenAI/Anthropic/Ollama）
│   │   ├── commands/            #   管理面（Tauri invoke 命令）
│   │   ├── db/                  #   持久化（SQLx 连接池 + migrations）
│   │   ├── models/              #   数据模型（Serde 结构体）
│   │   ├── crypto/              #   加密（AES-GCM + OS keystore）
│   │   ├── security/            #   安全审计（风控/脱敏/审计事件）
│   │   ├── config/              #   应用配置
│   │   └── error.rs             #   统一错误类型
│   ├── migrations/              #   SQL 迁移脚本
│   ├── capabilities/            #   Tauri 权限配置
│   ├── icons/                   #   应用图标
│   ├── Cargo.toml
│   ├── tauri.conf.json
│   └── build.rs
├── index.html
├── package.json
├── vite.config.ts
├── tsconfig.json
├── tsconfig.node.json
├── components.json              # shadcn/ui 配置
├── .gitignore
└── README.md
```

---

## 2. 技术栈

### 2.1 前端

| 技术 | 版本 | 说明 |
|------|------|------|
| React | 19.x | UI 框架 |
| TypeScript | 5.8.x | 类型安全 |
| Vite | 7.x | 构建工具（经 `@tailwindcss/vite` 集成 Tailwind） |
| Tailwind CSS | 4.x | 原子化 CSS |
| React Router | 7.x (library 模式) | 路由（`react-router-dom`，HashRouter 适配 Tauri WebView） |
| Zustand | 5.x | 客户端 UI 状态（主题等） |
| @tanstack/react-query | 5.x | 服务端/异步数据缓存（包装 invoke） |
| shadcn/ui | copy-source 模式 | UI 组件（取自 Radix 原语，可改源码） |
| lucide-react | 1.x | 图标 |
| clsx + tailwind-merge | latest | className 合并工具 |

### 2.2 后端（Rust / Tauri）

| 技术 | 版本 | 说明 |
|------|------|------|
| Tauri | 2.x | 桌面框架（Rust 后端 + WebView 前端） |
| Axum | 0.8.x | 数据面 HTTP 服务器（OpenAI 兼容 API） |
| SQLx | 0.8.x | 异步 SQL，编译时校验 |
| Reqwest | 0.12.x | 上游 HTTP 客户端（支持流式 SSE） |
| Serde | 1.0 | 序列化/反序列化 |
| Tokio | 1.x | async 运行时 |
| tower / tower-http | latest | 中间件（CORS、trace） |
| uuid | 1.x | UUID 生成 |
| chrono | 0.4 | 时间处理 |
| aes-gcm | 0.10 | 密钥加密 |
| keyring | 3.x | 系统密钥环（Windows DPAPI） |

### 2.3 Tauri 插件

| 插件 | 用途 | 风险 |
|------|------|------|
| opener | 打开外部链接 | 低 |
| store | KV 偏好存储（主题/窗口状态） | 低 |
| clipboard-manager | 复制密钥/curl | 低 |
| notification | 风控告警弹窗 | 低 |
| autostart | 开机自启 | 低 |
| dialog | 导入/导出配置文件 | 低 |

> **注意**：不使用 plugin-sql（前端不直连 SQLite，统一走 SQLx 后端）。不使用 plugin-shell（无 shell 执行需求）。

---

## 3. 架构设计

### 3.1 双层架构

详见 `REQUIREMENTS.md §3`。

**关键原则**：
- 数据面（Axum）：对外暴露 OpenAI 兼容 HTTP API
- 管理面（Tauri invoke）：前端 IPC 调 Rust 命令
- 数据库单一所有者：SQLx 统一访问 SQLite，前端不碰 DB
- 同进程：Axum 和 Tauri 共享 Tokio runtime 和 DB 连接池

### 3.2 请求数据流

```
1. 客户端 POST /v1/chat/completions (Bearer sk-dong-xxx)
2. Axum auth 中间件 → 校验网关密钥 → 查 gateway_keys 表
3. 配额检查：quota_used vs quota_limit
4. dispatcher → 按 model + model_mapping 匹配渠道
5. balancer → 优先级排序 → 加权选择 → 熔断器检查
6. adapter → OpenAI 格式转上游格式（如 Anthropic messages）
7. proxy → Reqwest 转发 → SSE 流式透传
8. 响应回流客户端 + 异步落库 request_logs
9. 异常时记录错误信息 + notification
```

### 3.3 前端架构

```
React Frontend
├── Router (HashRouter)
│   ├── /dashboard         DashboardPage
│   ├── /channels          ChannelsPage
│   ├── /api-keys          ApiKeysPage
│   ├── /logs              LogsPage
│   ├── /audit             AuditPage
│   └── /settings          SettingsPage
├── State
│   ├── React Query        服务端数据（invoke 包装）
│   └── Zustand            UI 状态（主题）
├── API Layer
│   └── lib/api.ts         统一 invoke 封装 + Mock 回退
└── UI
    ├── components/ui/      shadcn/ui
    └── components/layout/  Layout/Sidebar/Topbar
```

**调用桥设计**：
- `lib/api.ts` 统一封装 `invoke`，类型安全
- 非 Tauri 环境（纯浏览器 `npm run dev`）自动回退 Mock 数据
- 前端页面统一从 `api.ts` 取数，不直接调 `invoke`

---

## 4. 数据库设计

### 4.1 表结构

详见 `src-tauri/migrations/001_init.sql`。

| 表 | 说明 |
|----|------|
| `channels` | 上游渠道 |
| `gateway_keys` | 网关密钥 |
| `request_logs` | 请求日志 |
| `security_findings` | 安全发现（关联 request_logs） |
| `settings` | 系统设置（KV） |

### 4.2 设计约定

- 时间戳统一 ISO8601 TEXT
- JSON 字段用 TEXT 存储，应用层 Serde 序列化
- 前后端 wire 格式统一 snake_case
- 枚举字段：status 用 INTEGER（0/1/2），类型/模式用 TEXT 字面量
- 主键用 UUID（TEXT 存储）

### 4.3 SQLx 编译时校验

- 使用 `sqlx::query!` 和 `query_as!` 宏，编译时校验 SQL 与 schema 一致性
- 需要 `cargo sqlx prepare` 生成离线数据（CI/无 DB 环境编译）
- 开发时需设置 `DATABASE_URL` 环境变量指向本地 SQLite

---

## 5. 开发环境配置

### 5.1 前置要求

| 工具 | 版本 | 说明 |
|------|------|------|
| Rust | 1.97+ | `rustc --version` |
| Node.js | 22+ | `node --version` |
| npm | 11+ | `npm --version` |
| Windows 11 | - | 主要目标平台 |

### 5.2 首次启动

```bash
# 1. 安装前端依赖
cd D:\DongX
npm install

# 2. 安装 shadcn/ui 组件（按需）
npx shadcn@latest init
npx shadcn@latest add button switch dialog card badge

# 3. 启动开发模式（同时启动 Vite + Tauri）
npm run tauri dev

# 4. 或仅前端开发（浏览器 + Mock 数据）
npm run dev
```

### 5.3 环境变量

| 变量 | 说明 |
|------|------|
| `DATABASE_URL` | SQLite 路径（SQLx 编译时校验用），如 `sqlite:D:\DongX\data\dongx.db` |

### 5.4 SQLx 离线模式

```bash
# 安装 sqlx-cli
cargo install sqlx-cli --no-default-features --features sqlite

# 创建数据库
sqlx database create --database-url "sqlite:data/dongx.db"

# 执行迁移
sqlx migrate run --database-url "sqlite:data/dongx.db" --source src-tauri/migrations

# 生成离线数据（CI/无 DB 环境编译用）
cargo sqlx prepare --workspace
```

---

## 6. 开发规范

### 6.1 标准化开发循环（vibe coding）

1. **需求澄清**：给 AI 下达指令时给出「目标 + 约束 + 不欲 + 示例」
2. **小步实现**：一次只改一个聚焦切片；大改前先 `git commit` 做 checkpoint
3. **就地校验**：每个切片完成后立即跑 `npm run build` + `npm test`
4. **代码审查**：AI 产出的每一处要能读懂、能改、能讲
5. **落袋提交**：验证通过的切片按 conventional commits 提交

### 6.2 Bug 报告规范

使用「期望行为 vs 实际行为 + 报错原文/截图」三要素格式，避免单纯现象词（"错位""不对"）。

### 6.3 Git 提交约定

- 分支：solo 项目直接 `master` 频繁提交
- 身份：`user.name "Wei"` / `user.email "191123654@qq.com"`
- 格式：Conventional Commits

| 前缀 | 用途 |
|------|------|
| `feat:` | 新功能/新组件 |
| `fix:` | 修复 |
| `refactor:` | 重构 |
| `docs:` | 文档 |
| `test:` | 测试 |
| `chore:` | 杂项/配置 |

### 6.4 前端测试约定

- 框架：`vitest`（jsdom）+ `@testing-library/react`
- Tauri 不可测：`invoke` 在 jsdom 跑不了；测试走 Mock 分支或 `vi.mock`
- 分层：
  - ① 纯逻辑单测：`src/lib/__tests__/`（优先写）
  - ② 组件测试：`src/components/ui/__tests__/`
  - ③ E2E：早期跳过
- 命令：`npm test` / `npm run test:watch` / `npm run coverage`

### 6.5 Rust 编码约定

- 模块化：按 `REQUIREMENTS.md §3.3` 的模块划分组织
- 错误处理：统一 `error.rs` 定义 `AppError`，实现 `IntoResponse`
- 异步：全链路 async/await，Tokio runtime
- Serde：所有 DB 模型派生 `Serialize/Deserialize`，字段名 snake_case

---

## 7. 构建与部署

### 7.1 开发模式

```bash
npm run tauri dev    # Vite + Tauri 同时启动
npm run dev          # 仅前端（浏览器 + Mock）
```

### 7.2 生产构建

```bash
npm run tauri build  # 编译 Rust + 打包前端 → 可安装 exe
```

产物位于 `src-tauri/target/release/bundle/`。

### 7.3 Tauri 配置要点

- `productName`: DongX
- `identifier`: com.wei.dongx
- `devUrl`: http://localhost:1420
- `frontendDist`: ../dist
- 窗口：1200x800，可调整大小
- CSP：开发时 null，生产时收紧

---

## 8. 开发路线图

| 阶段 | 目标 | 关键交付 | 状态 |
|------|------|---------|------|
| **M0 地基** | 项目初始化 + 文档 | 目录结构 + 需求/开发文档 + 配置文件 + Git | ✅ 本次 |
| **M1 前端骨架** | 路由 + 状态 + 主题 + Mock | Router + QueryClient + Zustand + 深色/浅色 + 6 页面骨架 | 待实施 |
| **M2 后端骨架** | Rust 依赖 + 插件 + DB | Cargo.toml + capabilities + SQLx + migrations + lib.rs | 待实施 |
| **M3 网关内核** | Axum 跑通转发 | chat/completions 代理 + SSE + 单渠道转发 | 待实施 |
| **M4 负载均衡** | 多渠道调度 | 加权轮询 + 故障转移 + 熔断器 | 待实施 |
| **M5 管理面** | CRUD 落地 | 渠道/密钥/日志/审计/设置 invoke 命令 | 待实施 |
| **M6 安全** | 加密 + 风控 | AES-GCM 密钥加密 + 风控规则 + 审计 | 待实施 |
| **M7 打磨** | 托盘 + 自启 + 打包 | tray + autostart + tauri build exe | 待实施 |

---

## 9. 安全设计要点

- 上游密钥 AES-GCM 加密落库，密钥材料用系统密钥环
- 网关密钥只存哈希，永不存明文
- Axum 只监听 127.0.0.1
- Tauri CSP 收紧
- SQL 参数化（SQLx 编译时校验）
- 审计事件 append-only
- 敏感信息脱敏后落日志

---

## 10. 术语表

| 术语 | 含义 |
|------|------|
| 上游渠道 (Channel) | 实际提供模型的供应商接入点 |
| 网关密钥 (Gateway Key / ApiKey) | 本软件下发给下游客户端的访问凭证 |
| 数据面 | 对外代理流量的 Axum HTTP 服务 |
| 管理面 | 前端经 invoke 配置系统的内部通道 |
| 模型映射 | 外部模型名到上游实际模型名的转换 |
| 熔断器 | 连续失败后自动隔离故障渠道的机制 |
| 协议族 | 供应商 API 的协议分类（OpenAI/Anthropic/Ollama） |
| Mock 回退 | 非 Tauri 环境下前端用 Mock 数据代替 invoke |
