# DongX 安全审计闸门 — 设计与实现

> 适配目标：本地单用户 LLM 网关（Tauri 2 + Rust/Axum）。参考同类桌面 LLM 网关（CC Switch 这类）的成熟安全闸门做法，按 DongX 的实际定位（单用户、本地、Chat + Responses 为主）裁剪。
>
> 本文档**描述已落地的真实实现**。末尾「§9 与初版方案偏差」列出了与早期 proposal 的关键差异，便于追溯决策。

---

## 1. 现状与问题（改造前）

| 项 | 现状 | 问题 |
|---|---|---|
| 内容扫描 | `security/scanner.rs` 有 `check_api_keys` / `check_credit_cards` / `check_ssn`(美式 `XXX-XX-XXXX`) / `check_internal_ips` | ① **全部未接入请求管道**（死代码，`#![allow(dead_code)]` 静音）；② `check_ssn` 是美式格式，**无中国身份证规则**；③ 没有手机号 / 邮箱 / 银行卡 / 私钥 / 提示注入等 |
| 脱敏 | `scanner::redact_secrets` 已实现但从未调用 | 日志里 `log_raw_body` 开启时会把**原始请求/响应体**落库（`request_logs.request_body/response_body`），凭证可能进本地 DB |
| 介入点 | 无统一"路由/转换/访问上游之前审计"的位置 | 鉴权后直接 `dispatcher`→`adapter`→上游，没有任何内容安全闸门 |
| 审计落库 | `audit_events` 表已接 `invalid_key`/`quota_exhaust`/`config_change` | `suspicious` 类型已预留但**未接线**（因为扫描没接） |

**一句话**：所谓"安全"只是几条文案级正则且没生效；真正的缺口是「统一闸门 + 脱敏转发 + 本地化 PII 规则 + 审计落库」。

---

## 2. 设计目标（落地版）

- **G1 统一介入点**：所有推理入口（Chat / Responses，靠 `mode` 区分，同一函数承载）在 `dispatcher`/`adapter`/上游调用**之前**先过安全闸门。
- **G2 原始协议全树扫描**：扫的是解析后的原始请求 JSON 全树，不是转换后的 Chat JSON（否则会漏掉 Responses 工具、图片 URL、未知字段）。
- **G3 脱敏转发**：`redact` 模式下对转发体做全树高风险脱敏，确保凭证/卡号/外传命令不出本机。（**注**：日志体脱敏尚未完成，见 §8 局限。）
- **G4 扫描预算保护**：单次请求累计扫描字节上限 1 MiB，超限**跳过剩余内容**（fail-open，不阻断请求）。
- **G5 风险分级 + 动作语义**（`permissive`/`warning`/`redact`/`strict`），**默认 `warning` 模式**——本地单用户不误伤。
- **G6 审计落库**：命中即写 `request_security_findings` 明细 + `request_logs` 风险汇总 6 字段 + `suspicious` 审计事件。

### 2.1 关键原则：fail-open（与初版 fail-closed 相反）

安全闸门任何**内部错误**（读设置失败 / 读规则失败 / 解析异常）一律**放行并记录告警**，绝不因审计子系统异常而阻断正常请求。理由：DongX 是本地单用户网关，审计是增强项而非基础设施——不应让审计子系统的故障影响用户正常对话。这与配额/健康统计的既有原则一致。

> 仅有两类情况会真正阻断或改变请求：① 闸门**正常决策**出 `Block`/`Redact` 动作（受 `security_mode` 与命中等级控制）；② 上游自身错误或鉴权失败（与审计无关）。

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
   ├─ 加载 7 个安全设置（enabled + mode + 6 开关）
   ├─ BuiltinRuleRepository::get_enabled + CustomRuleRepository::get_enabled
   ├─ scanner::scan(全树, settings, builtin, custom) → (findings, budget_exceeded)
   ├─ decide_action(findings, settings)              → (SecurityAction, SecurityOutcome)
   ├─ action == Redact → redact::redact(body) 得 forward_body；否则 forward_body = body
   │
   ▼
GateOutput { forward_body, outcome, findings, action }
   ├─ action == Block → 403 阻断，写日志(含 blocked_reason) + findings，绝不联系上游
   └─ 否则: forward_body 往下传; outcome 写 request_logs 6 字段;
            findings 写 request_security_findings; findings 非空写 suspicious 审计
```

---

## 4. 关键设计点

### 4.1 4 级模式 + 6 个检测开关

`security_mode`（`permissive`/`warning`/`redact`/`strict`）+ 6 个 `toggle_key` 开关（`scan_credentials`/`scan_pii`/`scan_payment`/`scan_network`/`scan_code_exec`/`scan_prompt_injection`）共同决定行为。

**`decide_action` 真值表**（按本次请求命中的**最高**风险等级 `max` 决策；`action` 之外的发现仍全部记录）：

| mode | max = Low/Info | max = Medium | max = High+ | 说明 |
|---|---|---|---|---|
| `permissive`（宽松） | Allow | Allow | Allow | 只记录，永不改动流量 |
| `warning`（警告，**默认**） | Allow | Warn | Warn | 中高风险标记告警，仍放行原文 |
| `redact`（脱敏） | Allow | Allow | Redact | 高风险脱敏后转发 |
| `strict`（严格） | Allow | Warn | Block | 高风险阻断；中高标记告警 |

- 风险评分 `risk_score` = 各发现 `RiskLevel::rank()` 之和，封顶 999，用于排序/展示。
- `blocked_reason` 仅在 `Block` 时生成（含模式、命中等级、最高风险标题）。
- 6 开关通过 `is_switch_on(settings, toggle_key)` 匹配；规则的 `toggle_key` 为 `NULL`/未知 → 常开（true）。开关关闭或规则 `enabled=0` 均不参与扫描（双控，与参考产品的"规则可按类别开关 + 单条可启停"一致）。

> 初版 proposal 的 `block_on_critical` 独立开关被**合并进 `strict` 模式**：`strict` 下 High+（含 critical 私钥/云密钥）一律 Block。不再需要独立开关。

### 4.2 库表（参考同类网关的「内置规则 + 自定义规则 + 发现明细」三表设计）

迁移文件：`src-tauri/migrations/003_security_audit.sql`（**新建**，sqlx 自动应用；遵循迁移铁律不改已应用的 `001`/`002`）。

**`security_builtin_rules`**（应用首次启动种子，用户可编辑 `enabled`）
| 列 | 含义 |
|---|---|
| `rule_id` | 稳定标识，scanner 按此匹配 `PATTERNS` 中的正则 |
| `category` | credential / personal / payment / network / tool / prompt |
| `severity` | info / low / medium / high / critical |
| `title` / `description` | 展示用 |
| `toggle_key` | 对应 6 个检测开关之一；NULL = 常开 |
| `enabled` | 0/1 |

**`security_custom_rules`**（用户自定义黑名单/白名单，P2 增强，库表已建，UI 待做）
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

**种子 21 条内置规则**（`b001`–`b021`），`toggle_key` 映射：

| toggle_key | 规则 | 示例命中 |
|---|---|---|
| `scan_credentials` | b001–b005 | `sk-…` / `ghp_` / `AKIA` / `AIza` / JWT / `Bearer`；PEM 私钥；`Authorization/Cookie/Password` 字段赋值；`mysql://` 等连接串；云密钥 |
| `scan_pii` | b006–b008 | 中国身份证 18 位；邮箱；手机号 `1[3-9]\d{9}` |
| `scan_payment` | b009–b010 | 信用卡号；银行卡号（银联 62 开头，带负向预查避身份证误报） |
| `scan_network` | b011–b014 | `ifconfig.me`/`ipify` 等 IP 探测；`webhook.site`/`ngrok` 等可疑域名；外部 URL；追踪像素 |
| `scan_code_exec` | b015–b019 | `curl`/`wget`/`bash -c`/`python -c`；数据外传组合；远程脚本管道；Git 信息；`id_rsa` 等 SSH 密钥 |
| `scan_prompt_injection` | b020–b021 | "忽略以上指令"/`disregard`/`jailbreak`/绕过审计；指纹/风控词 |

### 4.3 扫描器实现要点

- **全树 walk**：递归遍历 JSON（Object/Array/String/…），仅对字符串叶子做正则匹配；记录 JSON 指针定位。
- **正则集中在代码**：`PATTERNS: &[(&str, &str)]` 为 `(rule_id, regex)` 静态表，由 `OnceLock<HashMap>` **编译一次**（避免每次请求重编译，且无 ReDoS 风险）。元数据（severity/title）来自 DB，可编辑；正则固定可测。
- **脱敏证据**：`mask_evidence()` 取首尾各 2 字符 + `****`，落 `evidence_masked`（不存明文）。
- **预算保护**：`MAX_SCAN_BYTES = 1 MiB`（代码常量，非设置项）。累计扫描字节超上限 → 跳过剩余字符串，**不阻断请求**（fail-open 预算，与初版"超额拒绝"相反）。

### 4.4 脱敏（redact 模式）

`redact::redact(value)` 对全树每个字符串应用 `high_risk_regexes()`（severity ≥ High 的 14 个规则正则，见 §4.2 中 critical/high 项），命中替换为 `[REDACTED]`。仅在 `decide_action` 决策出 `Redact` 时对 `forward_body` 调用，上游收到的即为脱敏体。

> **G3 日志体脱敏尚未完成**（见 §8）：当前 `request_logs.request_body` 落的是**原始** `raw_request`（仅 `log_raw_body` 开启时捕获），即使在 `redact` 模式下日志也不脱敏。本地单用户可接受，但是明确的后续项。

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

### 4.6 设置项（实际落地）

`settings` KV 表：

| key | 类型 | 默认 | 说明 |
|---|---|---|---|
| `security_enabled` | bool | `true` | 总开关；关闭即整体放行 |
| `security_mode` | `"permissive"`\|`"warning"`\|`"redact"`\|`"strict"` | `"warning"` | 4 级模式 |
| `scan_credentials` | bool | `true` | 凭证类检测 |
| `scan_pii` | bool | `true` | 个人身份信息（身份证/手机/邮箱） |
| `scan_payment` | bool | `true` | 卡类检测 |
| `scan_network` | bool | `true` | 外联/域名/追踪 |
| `scan_code_exec` | bool | `true` | 命令执行/外传 |
| `scan_prompt_injection` | bool | `true` | 提示注入/越权 |

> 初版 proposal 的 `security.redact_secrets` / `security.block_on_critical` / `security.max_scan_bytes` 三个独立设置项**未采用**：脱敏由 `redact` 模式内置、关键风险拦截由 `strict` 模式内置、`max_scan_bytes` 作为代码常量 1 MiB。

前端：设置 → **安全审计** Tab（4 级模式下拉 + 启用开关 + 6 个检测开关卡片）。审计页右上角「设置」按钮跳转该 Tab。

---

## 5. 实施状态

- **已完成（本次落地）**：
  - 迁移 `003_security_audit.sql`：3 张表 + 索引 + 21 条种子规则 + 6 开关默认 `true` + 旧 `balanced` → `warning` 迁移。
  - `security/mod.rs`：类型 + `is_switch_on` + `decide_action`（4 级模式真值表）。
  - `security/rules.rs`：内置/自定义规则结构体 + `get_enabled` 仓储。
  - `security/scanner.rs`：全树 walk + `PATTERNS` + `OnceLock` 正则 + 1 MiB 预算 + 脱敏证据掩码 + `high_risk_regexes()`。
  - `security/redact.rs`：`redact()` 全树高风险替换。
  - `security/gate.rs`：`run_gate()` 编排 + fail-open。
  - `db/repository.rs::security_findings`：`insert()` 落明细。
  - `server/handler.rs`：闸门接线（chat+responses 共用）+ `spawn_log` 落 6 字段 + findings + suspicious 审计。
  - 前端：4 级模式 + 6 检测开关 UI（前序会话已落）。
  - **编译验证**：`cargo check --lib` 通过（仅 12 个既有 dead_code warning，与本次无关）。

- **未做（后续）**：见 §8。

---

## 6. 与初版方案偏差（决策追溯）

| # | 初版 proposal | 真实落地 | 理由 |
|---|---|---|---|
| 1 | 模式 `audit/warn/redact/block` + 独立 `block_on_critical` | `permissive/warning/redact/strict`；关键拦截并入 `strict` | 4 级更贴合"宽松→严格"语义；独立开关冗余 |
| 2 | 默认 `audit`（只记录） | 默认 `warning`（中高标记告警） | 默认即能让用户**看到**风险，又不阻断/改写，零误伤 |
| 3 | 双输出：`forward_json` + `sanitized_log_json`（日志永远脱敏） | 仅 `forward_body` 脱敏；**日志体仍存原文**（G3 未完全达标） | 本地单用户优先做"上游不出明文"，日志侧留给后续 |
| 4 | 预算超额 → fail-closed 拒绝（429） | 预算超额 → **跳过剩余内容，不阻断** | 本地请求通常很小；扫描器过载不应影响用户正常对话 |
| 5 | 闸门异常 → fail-closed 拒绝 | 闸门异常 → **fail-open 放行 + 告警** | 审计是增强项，不应因自身故障阻断用户请求（与配额/健康同原则） |
| 6 | 规则种子表列为 **P2 可选** | 直接落地 3 张表 + 21 条种子 + 自定义表结构 | 用户明确要求"参考其库表实现完成功能"，DB 规则管理一并落地 |
| 7 | 独立 `redact_secrets`/`block_on_critical`/`max_scan_bytes` 设置项 | 三设置项取消（内置到模式 / 代码常量） | 减少设置面，行为可由 mode 完全决定 |

---

## 7. 验收标准（按真实行为）

1. `security_enabled=false` → 请求完全不受审计影响（gate 直接放行）。
2. `warning` 模式 + 请求含 `sk-…` → 日志 `request_logs.risk_level=high`、`security_action=warn`；上游收到**原文**（不改写）；`request_security_findings` 有 1 行。
3. `redact` 模式 + 请求含 PEM 私钥 → 上游收到的 body 中私钥被 `[REDACTED]` 替换；`sanitized=true`。
4. `strict` 模式 + 请求含 `AKIA…` 云密钥（high）→ 返回 403 `security_blocked`，**绝不联系上游**；`blocked_reason` 非空。
5. 关闭某 `toggle_key`（如 `scan_pii=false`）→ 该类规则不参与扫描（身份证/手机/邮箱漏检）。
6. 畸形/超大（>1 MiB）请求 → 扫描跳过超额部分（warn 日志），请求照常转发（不拒）。
7. 中国身份证 18 位、手机号、邮箱、银行卡能被对应规则命中。
8. 闸门依赖的 DB 查询抛错时 → 请求照常放行（fail-open）+ 告警日志。

---

## 8. 已知局限 / 后续项

- **G3 日志体脱敏未完成**：`request_logs.request_body`（仅 `log_raw_body` 开启时）仍存原文。如需"DB 永不落明文"，可在 `spawn_log` 改用 `forward_body`（redact 模式下已是脱敏体；其余模式也可统一走 `redact::redact` 仅对 high+ 类别脱敏的日志副本）。
- **响应体扫描**：当前不扫响应体（参考产品有响应侧安全扫描开关，前端已预留 `scan_response` 类 UI 位，后端未接）。
- **自定义规则 UI**：`security_custom_rules` 表已建、仓储已接，但前端编辑 UI 未做（P2）。
- **单测**：gate / scanner / redact 尚无自动化测试（验收目前靠人工）。建议补"脱敏后上游只见掩码""超预算 fail-open""fail-open 槽路"等用例。
- **误报**：身份证/手机号正则可能误命中数字串；因默认 `warning` 不拦不改，影响可控；`strict` 用户需关注告警。

---

## 9. 迁移铁律提醒

`003_security_audit.sql` 一旦被 sqlx 应用（应用首次启动跑过迁移即生效），**禁止再编辑**（sqlx 的 `_sqlx_migrations.checksum` = SHA-384(文件原始字节)，改了启动必 panic）。任何后续规则调整：
- 改 `enabled` / 加规则 → 走新迁移文件（`004_*.sql`）或运行时编辑 DB，而非改 `003`。
- 改正则 → 正则在 `scanner.rs` 的 `PATTERNS` 常量（代码层），与 DB 规则元数据解耦，改代码即可、无需动迁移。
