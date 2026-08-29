# DongX 安全审计闸门 — 设计与实现

> 适配目标：本地单用户 LLM 网关（Tauri 2 + Rust/Axum）。参考同类桌面 LLM 网关（CC Switch 这类）的成熟安全闸门做法，按 DongX 的实际定位（单用户、本地、Chat + Responses 为主）裁剪。
>
> 本文档**描述已落地的真实实现**。安全审计设置模型（4 模式 + 6 开关）已**对齐 waliapi**；末尾「§9 waliapi 对齐度对照」列出与参考实现的差异与保留的 DongX 本地化决策。

---

## 1. 现状与问题（改造前）

| 项 | 现状 | 问题 |
|---|---|---|
| 内容扫描 | `security/scanner.rs` 有 `check_api_keys` / `check_credit_cards` / `check_ssn`(美式 `XXX-XX-XXXX`) / `check_internal_ips` | ① **全部未接入请求管道**（死代码，`#![allow(dead_code)]` 静音）；② `check_ssn` 是美式格式，**无中国身份证规则**；③ 没有手机号 / 邮箱 / 银行卡 / 私钥 / 提示注入等 |
| 脱敏 | `scanner::redact_secrets` 已实现但从未调用 | 日志里 `log_raw_body` 开启时会把**原始请求/响应体**落库（`request_logs.request_body/response_body`），凭证可能进本地 DB |
| 介入点 | 无统一"路由/转换/访问上游之前审计"的位置 | 鉴权后直接 `dispatcher`→`adapter`→上游，没有任何内容安全闸门 |
| 审计落库 | `audit_events` 表已接 `invalid_key`/`quota_exhaust`/`config_change` | `suspicious` 类型已预留但**未接线**（因为扫描没接） |
| 设置开关 | 前端 6 个开关写 `security_detect_*` 等键，但后端 gate 读的是另一套 `scan_*` 死键 | **6 个开关全是装饰品**，对闸门零作用（本次对齐已修正） |

**一句话**：所谓"安全"只是几条文案级正则且没生效；真正的缺口是「统一闸门 + 脱敏转发 + 本地化 PII 规则 + 审计落库 + 开关真正生效」。

---

## 2. 设计目标（落地版）

- **G1 统一介入点**：所有推理入口（Chat / Responses，靠 `mode` 区分，同一函数承载）在 `dispatcher`/`adapter`/上游调用**之前**先过安全闸门。
- **G2 原始协议全树扫描**：扫的是解析后的原始请求 JSON 全树，不是转换后的 Chat JSON（否则会漏掉 Responses 工具、图片 URL、未知字段）。
- **G3 脱敏转发 + 日志脱敏**：`security_redact_secrets` 开启时对转发体做全树高风险脱敏；日志落库体**始终**走脱敏副本（高风险凭证不出本机、不入 DB）。
- **G4 扫描预算保护**：单次请求累计扫描字节上限 1 MiB，超限**跳过剩余内容**（fail-open，不阻断请求）。
- **G5 风险分级 + 动作语义**（`audit`/`warn`/`redact`/`block`，对齐 waliapi），**默认 `audit` 模式**——本地单用户默认仅记录、零误伤。
- **G6 审计落库**：命中即写 `request_security_findings` 明细 + `request_logs` 风险汇总 6 字段 + `suspicious` 审计事件。

### 2.1 关键原则：fail-open（与初版 fail-closed 相反）

安全闸门任何**内部错误**（读设置失败 / 读规则失败 / 解析异常）一律**放行并记录告警**，绝不因审计子系统异常而阻断正常请求。理由：DongX 是本地单用户网关，审计是增强项而非基础设施——不应让审计子系统的故障影响用户正常对话。这与配额/健康统计的既有原则一致。

> 仅有两类情况会真正阻断或改变请求：① 闸门**正常决策**出 `Block`（受 `security_mode`/`block_on_critical` 与命中等级控制）；② 上游自身错误或鉴权失败（与审计无关）。

---

## 3. 架构（真实模块划分）

```
security/
├── mod.rs        # 类型: RiskLevel / SecurityAction / SecuritySettings / SecurityFinding /
│                 #      SecurityOutcome + is_switch_on() + decide_action()（模式→动作）
├── gate.rs       # run_gate(): 编排 加载设置→取启用规则→扫描→决策→(可选)脱敏→GateOutput
├── scanner.rs    # 全树 JSON walker + OnceLock 正则表(PATTERNS) + 1MiB 预算 + 脱敏证据掩码
├── rules.rs      # BuiltinRule / CustomRule 结构体 + 两表的 get_enabled 仓储
├── redact.rs     # redact(): 对全树字符串应用 high_risk_regexes() → 命中替换 [REDACTED]
└── rate_limit.rs # (已有, 独立 backlog) 限流, 本实现不展开

db/repository.rs
└── security_findings  # insert(): 写 request_security_findings 一行
```

**核心数据流（gate）**：

```
原始请求 JSON (body_json)
   │
   ▼
run_gate(pool, body)
   ├─ security_enabled == false → 直接放行（forward=原 body, outcome=allow）
   ├─ 加载安全设置（enabled + mode + 6 开关，键名对齐 waliapi）
   ├─ BuiltinRuleRepository::get_enabled + CustomRuleRepository::get_enabled
   ├─ scanner::scan(全树, settings, builtin, custom) → (findings, budget_exceeded)
   ├─ decide_action(findings, settings)              → (SecurityAction, SecurityOutcome)
   ├─ security_redact_secrets == true → redact::redact(body) 得 forward_body；否则 forward_body = body
   │
   ▼
GateOutput { forward_body, outcome, findings, action }
   ├─ action == Block → 403 阻断，写日志(含 blocked_reason) + findings，绝不联系上游
   └─ 否则: forward_body 往下传; outcome 写 request_logs 6 字段;
            findings 写 request_security_findings; findings 非空写 suspicious 审计
```

---

## 4. 关键设计点

### 4.1 4 级模式 + 6 个检测开关（对齐 waliapi）

`security_mode`（`audit`/`warn`/`redact`/`block`）+ 6 个开关（3 个可切换扫描类目 + 1 个响应侧 + 2 个行为开关）共同决定行为。键名与 waliapi 完全一致：

| 开关 | 类型 | 默认 | 说明 |
|---|---|---|---|
| `security_scan_unicode` | 扫描类目 | `true` | Unicode 隐写检测（b022–b025） |
| `security_scan_tools` | 扫描类目 | `true` | 工具/命令风险（exec.* 规则） |
| `security_scan_network` | 扫描类目 | `true` | 外联/追踪（net.* 规则） |
| `security_scan_response` | 响应侧 | `false` | 响应体扫描（非流式响应按规则扫描，发现 phase=response 落库） |
| `security_redact_secrets` | **行为** | `false` | 独立于模式：开启后转发体高风险脱敏 |
| `security_block_on_critical` | **行为** | `false` | 独立于模式：Critical 命中一律阻断 |

> 与 waliapi 的关键对齐点：**脱敏/阻断是独立于模式的开关**，而非塞进模式里。仅 `unicode/tools/network` 三类可被独立开关控制；凭证/PII/支付/命令/提示注入规则 `toggle_key=NULL`，**始终扫描**（不可单独关闭），与 waliapi 一致。

**`decide_action` 真值表**（按本次请求命中的**最高**风险等级 `max` 决策；`action` 之外的发现仍全部记录）：

| mode | max = Low/Info | max = Medium | max = High | max = Critical | 说明 |
|---|---|---|---|---|---|
| `audit`（只审计，**默认**） | Allow | Allow | Allow | Allow | 仅记录，永不改动流量 |
| `warn`（警告） | Allow | Warn | Warn | Warn | 中高风险标记告警，仍放行原文 |
| `redact`（脱敏） | Allow | Allow | Redact | Redact | 高风险动作标记为 Redact |
| `block`（阻断） | Allow | Allow | Block | Block | 高风险直接阻断 |

- **跨模式覆盖 `block_on_critical`**：无论 `mode` 取值，只要 `max == Critical` 且开关开启 → 强制 `Block`（对齐 waliapi，避免"warn 模式漏掉私钥/云密钥"）。
- 转发体脱敏由 **`security_redact_secrets` 独立控制**（与 `mode` 解耦）：开启即对所有请求体走 `redact::redact()`，上游只见 `[REDACTED]`；日志落库体则始终走脱敏副本（见 §4.4）。
- `sanitized` 标记 = `security_redact_secrets`（真实反映转发体是否被脱敏）。
- 风险评分 `risk_score` = 各发现 `RiskLevel::rank()` 之和，封顶 999，用于排序/展示。
- `blocked_reason` 仅在 `Block` 时生成（含模式、命中等级、最高风险标题）。
- 开关通过 `is_switch_on(settings, toggle_key)` 匹配；规则 `toggle_key=NULL`/未知 → 常开（true）。开关关闭或规则 `enabled=0` 均不参与扫描（双控）。

### 4.2 库表（对齐同类网关的「内置规则 + 自定义规则 + 发现明细」三表设计）

迁移文件：`src-tauri/migrations/003_security_audit.sql`（**新建**，sqlx 自动应用）+ `004_security_alignment.sql`（本次对齐，`003` 已应用不可改，故全部变更放 `004`）。

**`security_builtin_rules`**（应用首次启动种子，用户可编辑 `enabled`）
| 列 | 含义 |
|---|---|
| `rule_id` | 稳定标识，scanner 按此匹配 `PATTERNS` 中的正则 |
| `category` | credential / personal / payment / network / tool / prompt / unicode |
| `severity` | info / low / medium / high / critical |
| `title` / `description` | 展示用 |
| `toggle_key` | 对应 3 个可切换开关之一；NULL = 常开 |
| `enabled` | 0/1 |

**`security_custom_rules`**（用户自定义黑名单/白名单，库表已建，UI 待做）
| 列 | 含义 |
|---|---|
| `rule_type` | blacklist / whitelist |
| `category` | domain / tool / path / keyword |
| `pattern` | 子串/关键字 |
| `severity` / `action` / `enabled` | 风险等级 / 动作 / 启停 |

**`request_security_findings`**（每次请求命中的风险明细）
| 列 | 含义 |
|---|---|
| `log_id` | → `request_logs.id` |
| `phase` | 扫描阶段（当前固定 `request`） |
| `category` / `rule_id` / `severity` / `title` | 来自规则 |
| `location` | JSON 指针，定位命中字段 |
| `evidence_masked` | 脱敏证据片段（首尾各 2 字符 + `****`），**不存明文** |
| `evidence_hash` | 明文哈希（可选，当前恒 NULL） |
| `action` | 对该发现采取的动作 |

**种子 25 条内置规则**（`b001`–`b025`，`003` 种 21 条 + `004` 补 4 条 Unicode），`toggle_key` 映射（经迁移 `004` 重映射）：

| toggle_key | 规则 | 示例命中 |
|---|---|---|
| `NULL`（常开） | b001–b005 | `sk-…` / `ghp_` / `AKIA` / `AIza` / JWT / `Bearer`；PEM 私钥；`Authorization/Cookie/Password` 字段赋值；`mysql://` 等连接串；云密钥 |
| `NULL`（常开） | b006–b008 | 中国身份证 18 位；邮箱；手机号 `1[3-9]\d{9}` |
| `NULL`（常开） | b009–b010 | 信用卡号；银行卡号（银联 62 开头，带负向预查避身份证误报） |
| `security_scan_network` | b011–b014 | `ifconfig.me`/`ipify` 等 IP 探测；`webhook.site`/`ngrok` 等可疑域名；外部 URL；追踪像素 |
| `security_scan_tools` | b015–b019 | `curl`/`wget`/`bash -c`/`python -c`；数据外传组合；远程脚本管道；Git 信息；`id_rsa` 等 SSH 密钥 |
| `NULL`（常开） | b020–b021 | "忽略以上指令"/`disregard`/`jailbreak`/绕过审计；指纹/风控词 |
| `security_scan_unicode` | b022–b025 | 零宽字符 U+200B 等；Bidi 控制 U+202A–E；变体选择符；同形异义字符 |

### 4.3 扫描器实现要点

- **全树 walk**：递归遍历 JSON（Object/Array/String/…），仅对字符串叶子做正则匹配；记录 JSON 指针定位。
- **正则集中在代码**：`PATTERNS: &[(&str, &str)]` 为 `(rule_id, regex)` 静态表，由 `OnceLock<HashMap>` **编译一次**（避免每次请求重编译，且无 ReDoS 风险）。元数据（severity/title）来自 DB，可编辑；正则固定可测。
- **脱敏证据**：`mask_evidence()` 取首尾各 2 字符 + `****`，落 `evidence_masked`（不存明文）。
- **预算保护**：`MAX_SCAN_BYTES = 1 MiB`（代码常量，非设置项）。累计扫描字节超上限 → 跳过剩余字符串，**不阻断请求**（fail-open 预算）。

### 4.4 脱敏（由 `security_redact_secrets` 独立控制）

`redact::redact(value)` 对全树每个字符串应用 `high_risk_regexes()`（severity ≥ High 的 15 个规则正则：密钥/私钥/连接串/云密钥/卡号/可疑域名/IP 探测/外传/远程脚本/SSH 密钥/提示注入/Bidi 控制），命中替换为 `[REDACTED]`。

- **转发体脱敏**：当 `security_redact_secrets=true`（独立于 `mode`）时，`run_gate` 对所有请求体调用 `redact::redact()`，上游收到的即为脱敏体。`sanitized` 标记 = `security_redact_secrets`。
- **日志体脱敏（G3 请求体已完成）**：`handler.rs` 在解析 `body_json` 后、落库前对其调用 `redact::redact()` 生成脱敏副本，存入 `request_logs.request_body`（仅 `log_raw_body` 开启时落）。即本地 DB **永不落明文高风险凭证**；低/中风险（邮箱/手机/身份证）仍保留以便调试。无论 `mode` 与 `redact_secrets` 取值，日志侧一律存脱敏副本。
- **日志体脱敏（响应体已完成）**：`log_raw_body` 开启时，`request_logs.response_body` 在 `security_redact_secrets` 开启时统一脱敏（非流式 JSON 走 `redact::redact`，流式 SSE 文本走 `redact::redact_text`），闭合 G3 响应半边。

### 4.5 与现有代码接线（接入点 = `handler.rs::run_chat_pipeline`）

`run_chat_pipeline` 同时承载 chat 与 responses（靠 `mode` 区分），闸门**只插一处**覆盖两条路径：

```rust
// handler.rs::run_chat_pipeline 内, auth 成功后、retry 设置加载前
let gate = match security::gate::run_gate(&state.db, body_json.clone()).await {
    Ok(g) => g,
    Err(e) => {
        tracing::warn!("安全闸门异常，fail-open 放行: {}", e);
        security::GateOutput { forward_body: body_json.clone(),
            outcome: SecurityOutcome::allow(), findings: vec![], action: SecurityAction::Allow }
    }
};
let sec_outcome = gate.outcome.clone();
let sec_findings = gate.findings.clone();

if gate.action == SecurityAction::Block {
    spawn_log(... 403, blocked_reason, sec_outcome, sec_findings);
    return error_response(FORBIDDEN, "security_blocked", ...);
}
let forward_body = gate.forward_body.clone();
// 后续 dispatcher/adapter 使用 forward_body（脱敏模式下为脱敏体）
```

- `DispatchContext.request_body` 与 `ProxyRequest.body` 改用 `forward_body`。
- `spawn_log` 签名尾部加 `sec: SecurityOutcome, findings: Vec<SecurityFinding>`：内部 `request_logs::insert` 传入 6 安全字段；落 `request_security_findings`（via `repository::security_findings::insert`）；findings 非空写 `suspicious` 审计（critical/high→critical，medium→warning，其余→info）。
- 流路径 `serve_stream` / `build_stream_response` / `build_responses_stream_response` 同步透传 `sec, findings` 到 `spawn_log`。
- `Responses` 路径因复用同一 `run_chat_pipeline` 自动覆盖，无需另插。

### 4.6 设置项（实际落地，对齐 waliapi）

`settings` KV 表（键名与 waliapi 完全一致）：

| key | 类型 | 默认 | 说明 |
|---|---|---|---|
| `security_enabled` | bool | `true` | 总开关；关闭即整体放行 |
| `security_mode` | `"audit"`\|`"warn"`\|`"redact"`\|`"block"` | `"audit"` | 4 级模式 |
| `security_scan_unicode` | bool | `true` | Unicode 隐写检测 |
| `security_scan_tools` | bool | `true` | 工具/命令风险检测 |
| `security_scan_network` | bool | `true` | 外联/追踪风险检测 |
| `security_scan_response` | bool | `false` | 响应侧安全扫描（非流式响应已接，发现 phase=response 落库） |
| `security_redact_secrets` | bool | `false` | 请求脱敏转发（独立于模式） |
| `security_block_on_critical` | bool | `false` | 严重风险强制阻断（跨模式覆盖） |

> **默认值说明**：waliapi 将所有扫描开关默认 `false`（opt-in）。DongX 作为**本地单用户网关**保留了「扫描默认开、强动作默认关」的本地化决策——`scan_*` 三类目默认 `true`（装上即有保护），`redact_secrets`/`block_on_critical`/`scan_response` 默认 `false`（需用户主动开启强动作）。模式默认 `audit` 与 waliapi 一致。

前端：设置 → **安全审计** Tab（4 级模式下拉 + 启用开关 + 6 个检测开关卡片，键名与后端/DB 一致）。

**迁移 `004_security_alignment.sql`**（本次对齐新增，遵循迁移铁律不动 `003`）：
- 重映射 `security_builtin_rules.toggle_key`：凭证/PII/支付/提示注入 → `NULL`（常开）；net → `security_scan_network`；exec → `security_scan_tools`；新增 4 条 Unicode 规则 b022–b025（`security_scan_unicode`）。
- 旧模式值映射：`permissive→audit` / `warning→warn` / `strict→block`（redact 不变）。
- 清理 `003` 种下的孤儿类目键 `scan_credentials/pii/payment/network/code_exec/prompt_injection`。

---

## 5. 实施状态

- **已完成（本次落地）**：
  - 迁移 `003_security_audit.sql`：3 张表 + 索引 + 21 条种子规则 + 6 开关默认 `true` + 旧 `balanced` → `warning` 迁移。
  - 迁移 `004_security_alignment.sql`：规则 `toggle_key` 重映射 + 补 4 条 Unicode 规则 + 旧模式值映射 + 清理孤儿键（**对齐 waliapi**）。
  - `security/mod.rs`：类型 + `is_switch_on` + `decide_action`（4 级模式真值表 + `block_on_critical` 跨模式覆盖）。
  - `security/rules.rs`：内置/自定义规则结构体 + `get_enabled` 仓储。
  - `security/scanner.rs`：全树 walk + `PATTERNS`（含 4 条 Unicode 正则）+ `OnceLock` 正则 + 1 MiB 预算 + 脱敏证据掩码 + `high_risk_regexes()`（15 条）。
  - `security/redact.rs`：`redact()` 全树高风险替换。
  - `security/gate.rs`：`run_gate()` 编排（读 waliapi 键名 + `redact_secrets` 解耦脱敏）+ fail-open。
  - `db/repository.rs::security_findings`：`insert()` 落明细。
  - `server/handler.rs`：闸门接线（chat+responses 共用）+ `spawn_log` 落 6 字段 + findings + suspicious 审计 + 日志体脱敏副本（G3 请求体）。
  - `commands/settings.rs` + 前端 `types/index.ts` / `api.ts` / `SettingsPage.tsx`：6 开关键名与 4 级模式**对齐 waliapi**，开关真正生效（修正此前死键 bug）。
  - **编译验证**：`cargo check --lib` 通过 + `cargo test --lib security` 单测通过。

- **未做（后续）**：见 §8。

---

## 6. 与 waliapi 对齐度对照

本实现的安全审计设置模型（4 模式 + 6 开关）**已对齐 waliapi**：库表三件套（`security_builtin_rules`/`security_custom_rules`/`request_security_findings`）+ `request_logs` 6 安全字段 + 4 级模式 + 6 开关键名均一致。下表列出差异与保留的 DongX 本地化决策：

| 维度 | waliapi | DongX | 是否对齐 |
|---|---|---|---|
| 模式枚举 | `audit/warn/redact/block` | `audit/warn/redact/block` | ✅ 完全一致 |
| 6 开关键名 | `security_scan_unicode/tools/network/response/redact_secrets/block_on_critical` | 同名 | ✅ 完全一致 |
| 行为开关解耦 | 脱敏/阻断独立开关，与模式解耦 | 同 | ✅ 一致 |
| 仅 3 类可单独关 | unicode/tools/network；凭证/PII/支付/命令/注入常开 | 同 | ✅ 一致 |
| 扫描开关默认值 | 全部 `false`（opt-in） | `scan_*` 三类目 `true`、强动作 `false` | ⚠️ 本地化：单用户装上即有保护 |
| 响应侧扫描 | 有开关（预留后端） | 同（预留后端未接） | ✅ 一致（均未接后端） |
| 模式默认值 | `audit` | `audit` | ✅ 一致 |
| 预算/异常 | fail-closed 拒绝 | **fail-open 放行** | ⚠️ 本地化：单用户不误伤 |
| Unicode 规则 | b012–b015 | b022–b025（语义对齐） | ✅ 一致 |

---

## 7. 验收标准（按真实行为）

1. `security_enabled=false` → 请求完全不受审计影响（gate 直接放行）。
2. `warn` 模式 + 请求含 `sk-…` → 日志 `request_logs.risk_level=high`、`security_action=warn`；上游收到**原文**（不改写）；`request_security_findings` 有 1 行。
3. `redact` 模式 + `security_redact_secrets=true` + 请求含 PEM 私钥 → 上游收到的 body 中私钥被 `[REDACTED]` 替换；`sanitized=true`。
4. `block` 模式 + 请求含 `AKIA…` 云密钥（high）→ 返回 403 `security_blocked`，**绝不联系上游**；`blocked_reason` 非空。
5. `security_block_on_critical=true` + `warn` 模式 + 请求含私钥（critical）→ 仍强制 `Block`（跨模式覆盖生效）。
6. 关闭某可切换开关（如 `security_scan_network=false`）→ 该类规则不参与扫描（webhook.site 等漏检）；凭证/PII 类 `toggle_key=NULL` 始终扫描，不可单独关。
7. 畸形/超大（>1 MiB）请求 → 扫描跳过超额部分（warn 日志），请求照常转发（不拒）。
8. 中国身份证 18 位、手机号、邮箱、银行卡、Unicode 隐写（零宽/Bidi）能被对应规则命中。
9. 闸门依赖的 DB 查询抛错时 → 请求照常放行（fail-open）+ 告警日志。
10. 设置页拨动 6 个开关 → 经 `update_settings` 写入 `settings` 表 → `run_gate` 实际读取并影响扫描（**此前为死键，现已修正**）。

---

## 8. 已知局限 / 后续项

- **响应体扫描 / 响应体日志脱敏（已完成）**：`security_scan_response` 后端已接——非流式响应按启用规则扫描，发现 `phase="response"` 写入 `request_security_findings` 并并入本次审计风险；`security_redact_secrets` 开启时，落库的 `request_logs.response_body`（非流式 JSON + 流式 SSE 文本，后者经 `redact::redact_text`）统一脱敏，闭合 G3 响应半边。`cargo test --lib security` 已含 `scan_response_*` 集成用例。
  - **流式响应扫描边界**：流式/SSE 响应为逐帧文本、非完整 JSON，`scan_response` 仅覆盖非流式（JSON）响应；流式响应目前只做日志体脱敏、不做逐帧发现扫描（避免中途打断流、且 SSE 解析复杂）。代码注释与设计文档均已标注此边界。
- **自定义规则 UI**：`security_custom_rules` 表已建、仓储已接，但前端编辑 UI 未做（P2）。
- **单测（核心逻辑已补）**：`security/mod.rs`、`security/scanner.rs`、`security/redact.rs` 均加 `#[cfg(test)]` 用例；`security/gate.rs` 补 `#[cfg(test)]` 集成测试（内存库跑迁移 003/004，覆盖 4 模式、redact_secrets 脱敏转发体、block_on_critical 跨模式、network/unicode 开关关闭跳过、新键接线、`scan_response` 响应扫描 + phase 标记），`cargo test --lib` 全绿（37 passed）。
- **误报**：身份证/手机号正则可能误命中数字串；因默认 `audit` 不拦不改，影响可控；`block` 用户需关注告警。

---

## 9. 迁移铁律提醒

`003_security_audit.sql` 一旦被 sqlx 应用（应用首次启动跑过迁移即生效），**禁止再编辑**（sqlx 的 `_sqlx_migrations.checksum` = SHA-384(文件原始字节)，改了启动必 panic）。任何后续规则调整：
- 改 `enabled` / 加规则 → 走新迁移文件（`004_*.sql`）或运行时编辑 DB，而非改 `003`。
- 改正则 → 正则在 `scanner.rs` 的 `PATTERNS` 常量（代码层），与 DB 规则元数据解耦，改代码即可、无需动迁移。
- **本次已踩的坑**：`003` 种下的 `scan_*` 类目键是"死键"（gate 从不读），前端拨的开关写的是另一套 `security_detect_*` 键——导致 6 个开关对闸门零作用。对齐 waliapi 后，所有键统一为 `security_scan_*` 等，并通过 `004` 清理了孤儿键。
