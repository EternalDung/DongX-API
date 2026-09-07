use crate::db::repository::settings as settings_repo;
use crate::AppState;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Arc;

// ── 数据结构 ──

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ClientInfo {
    pub name: String,
    pub label: String,
    pub icon: String,
    pub description: String,
    pub config_path: String,
    pub config_format: String,
    /// 可配置：CLI 已装 或 配置文件已存在（可写入）
    pub available: bool,
    /// 已安装：探测到 CLI 可执行文件（强信号，不再仅凭配置目录存在判断）
    pub installed: bool,
    pub applied: bool,
    pub download_url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ApplyResult {
    pub success: bool,
    pub message: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct ConfigContent {
    pub exists: bool,
    pub content: String,
    pub error: Option<String>,
}

// ── 应用定义（方案 B：配置目录存在即视为已安装） ──

struct AppDef {
    name: &'static str,
    label: &'static str,
    icon: &'static str,
    description: &'static str,
    config_format: &'static str,
    download_url: &'static str,
    config_dir_fn: fn() -> PathBuf,
    config_file: &'static str,
    /// CLI 可执行文件名（在 PATH 中查找，覆盖 npm/nvm 全局安装）
    cli_names: &'static [&'static str],
    /// 额外安装点：相对 home 的路径（覆盖官方安装脚本等不在 PATH 的情况）
    extra_exe: &'static [&'static str],
}

fn home_dir() -> PathBuf {
    dirs::home_dir().unwrap_or_else(|| PathBuf::from("."))
}

// 各客户端配置目录（方案 B：配置目录存在即视为已安装）
fn claude_dir() -> PathBuf {
    home_dir().join(".claude")
}
fn codex_dir() -> PathBuf {
    home_dir().join(".codex")
}
fn opencode_dir() -> PathBuf {
    home_dir().join(".config").join("opencode")
}
fn openclaw_dir() -> PathBuf {
    home_dir().join(".qclaw")
}
fn hermes_dir() -> PathBuf {
    home_dir().join(".hermes")
}

// ── 安装检测：探测 CLI 可执行文件 ──
// 注意：不能仅凭「配置目录存在」判断已安装。
// ~/.claude 这类目录会由 Claude 桌面端创建（内含 sessions/，并生成 ~/.claude.json），
// 与 Claude Code CLI 是否安装无关；本程序写入配置时也会 create_dir_all 创建目录，
// 若以目录为准会形成「写入一次即永久显示已安装」的自证循环。

/// 在 PATH 中查找可执行文件（Windows 需遍历 PATHEXT，如 .cmd/.exe）
fn command_exists(prog: &str) -> bool {
    let Some(path_var) = std::env::var_os("PATH") else {
        return false;
    };
    let exts: Vec<String> = if cfg!(windows) {
        std::env::var("PATHEXT")
            .unwrap_or_else(|_| ".COM;.EXE;.BAT;.CMD".to_string())
            .split(';')
            .filter(|s| !s.is_empty())
            .map(|s| s.to_string())
            .collect()
    } else {
        Vec::new()
    };

    for dir in std::env::split_paths(&path_var) {
        if dir.join(prog).is_file() {
            return true;
        }
        for ext in &exts {
            if dir.join(format!("{prog}{ext}")).is_file() {
                return true;
            }
        }
    }
    false
}

/// 额外安装点（相对 home），Windows 下补测 .exe
fn extra_exe_exists(rel: &str) -> bool {
    let base = home_dir().join(rel);
    if base.is_file() {
        return true;
    }
    cfg!(windows) && base.with_extension("exe").is_file()
}

/// 是否已安装：PATH 中能找到 CLI，或命中额外安装点
fn detect_cli(app: &AppDef) -> bool {
    app.cli_names.iter().any(|c| command_exists(c))
        || app.extra_exe.iter().any(|r| extra_exe_exists(r))
}

const APPS: &[AppDef] = &[
    AppDef {
        name: "claude-code",
        label: "Claude Code",
        icon: "terminal",
        description: "Anthropic 命令行 AI 编程助手，写入 ~/.claude/settings.json 的 env 段",
        config_format: "JSON (~/.claude/settings.json)",
        download_url: "https://docs.anthropic.com/en/docs/claude-code/overview",
        config_dir_fn: claude_dir,
        config_file: "settings.json",
        cli_names: &["claude"],
        extra_exe: &[
            ".local/bin/claude",
            ".claude/local/claude",
            "AppData/Local/Programs/claude-code/claude",
        ],
    },
    AppDef {
        name: "codex",
        label: "Codex CLI",
        icon: "code",
        description:
            "OpenAI Codex 命令行工具，写入 ~/.codex/config.toml 的 model_providers.dongx 段",
        config_format: "TOML (~/.codex/config.toml)",
        download_url: "https://github.com/openai/codex",
        config_dir_fn: codex_dir,
        config_file: "config.toml",
        cli_names: &["codex"],
        extra_exe: &[".local/bin/codex"],
    },
    AppDef {
        name: "opencode",
        label: "OpenCode",
        icon: "wrench",
        description: "开源 AI 编程工具，写入 ~/.config/opencode/opencode.json 的 provider.dongx 段",
        config_format: "JSON (~/.config/opencode/opencode.json)",
        download_url: "https://opencode.ai",
        config_dir_fn: opencode_dir,
        config_file: "opencode.json",
        cli_names: &["opencode"],
        extra_exe: &[".local/bin/opencode"],
    },
    AppDef {
        name: "openclaw",
        label: "OpenClaw",
        icon: "bot",
        description: "开源 Agent 框架，写入 ~/.qclaw/config.json 的 provider 段",
        config_format: "JSON (~/.qclaw/config.json)",
        download_url: "https://openclaw.ai",
        config_dir_fn: openclaw_dir,
        config_file: "config.json",
        cli_names: &["openclaw", "qclaw"],
        extra_exe: &[".local/bin/openclaw"],
    },
    AppDef {
        name: "hermes",
        label: "Hermes Agent",
        icon: "boxes",
        description: "Hermes Agent 框架，写入 ~/.hermes/config.json 的 custom_providers 段",
        config_format: "JSON (~/.hermes/config.json)",
        download_url: "https://github.com/openai/hermes",
        config_dir_fn: hermes_dir,
        config_file: "config.json",
        cli_names: &["hermes"],
        extra_exe: &[".local/bin/hermes"],
    },
];

// ── 原子写入（temp + rename，绝对不覆盖用户其它配置） ──

async fn atomic_write(path: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = path.parent() {
        tokio::fs::create_dir_all(parent)
            .await
            .map_err(|e| format!("创建目录失败: {e}"))?;
    }
    let tmp = path.with_extension(format!(
        "tmp.{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_nanos()
    ));
    tokio::fs::write(&tmp, data)
        .await
        .map_err(|e| format!("写入临时文件失败: {e}"))?;
    tokio::fs::rename(&tmp, path).await.map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        format!("替换文件失败: {e}")
    })?;
    Ok(())
}

async fn read_json_file<T: serde::de::DeserializeOwned>(path: &Path) -> Result<T, String> {
    let content = tokio::fs::read_to_string(path)
        .await
        .map_err(|e| format!("读取文件失败: {e}"))?;
    serde_json::from_str(&content).map_err(|e| format!("解析 JSON 失败: {e}"))
}

async fn write_json_file<T: Serialize>(path: &Path, data: &T) -> Result<(), String> {
    let json = to_pretty_json(data).map_err(|e| format!("序列化 JSON 失败: {e}"))?;
    atomic_write(path, json.as_bytes()).await
}

/// 自定义 JSON pretty printer，保留 non-ASCII 字符（中文等）原文不转义
fn to_pretty_json<T: Serialize>(data: &T) -> Result<String, String> {
    let value = serde_json::to_value(data).map_err(|e| format!("{e}"))?;
    let mut out = String::new();
    write_value(&mut out, &value, 0);
    Ok(out)
}

fn write_indent(out: &mut String, depth: usize) {
    for _ in 0..depth {
        out.push_str("  ");
    }
}

fn write_value(out: &mut String, v: &serde_json::Value, depth: usize) {
    match v {
        serde_json::Value::Null => out.push_str("null"),
        serde_json::Value::Bool(b) => out.push_str(if *b { "true" } else { "false" }),
        serde_json::Value::Number(n) => out.push_str(&n.to_string()),
        serde_json::Value::String(s) => write_json_string(out, s),
        serde_json::Value::Array(arr) => {
            if arr.is_empty() {
                out.push_str("[]");
            } else {
                out.push('[');
                for (i, item) in arr.iter().enumerate() {
                    out.push('\n');
                    write_indent(out, depth + 1);
                    write_value(out, item, depth + 1);
                    if i < arr.len() - 1 {
                        out.push(',');
                    }
                }
                out.push('\n');
                write_indent(out, depth);
                out.push(']');
            }
        }
        serde_json::Value::Object(obj) => {
            if obj.is_empty() {
                out.push_str("{}");
            } else {
                out.push('{');
                let len = obj.len();
                for (i, (k, val)) in obj.iter().enumerate() {
                    out.push('\n');
                    write_indent(out, depth + 1);
                    write_json_string(out, k);
                    out.push_str(": ");
                    write_value(out, val, depth + 1);
                    if i < len - 1 {
                        out.push(',');
                    }
                }
                out.push('\n');
                write_indent(out, depth);
                out.push('}');
            }
        }
    }
}

fn write_json_string(out: &mut String, s: &str) {
    out.push('"');
    for ch in s.chars() {
        match ch {
            '"' => out.push_str("\\\""),
            '\\' => out.push_str("\\\\"),
            '\n' => out.push_str("\\n"),
            '\r' => out.push_str("\\r"),
            '\t' => out.push_str("\\t"),
            '\u{08}' => out.push_str("\\b"),
            '\u{0c}' => out.push_str("\\f"),
            c if c.is_control() => {
                out.push_str(&format!("\\u{:04x}", c as u32));
            }
            c => out.push(c),
        }
    }
    out.push('"');
}

// ── 备份与恢复（恢复原始配置） ──

fn backup_path(config_path: &Path) -> PathBuf {
    let mut name = config_path
        .file_name()
        .unwrap_or_default()
        .to_string_lossy()
        .to_string();
    name.push_str(".dongx-backup");
    config_path.with_file_name(name)
}

async fn backup_config(config_path: &Path) -> Result<(), String> {
    if config_path.exists() {
        let content = tokio::fs::read(config_path)
            .await
            .map_err(|e| format!("读取配置失败: {e}"))?;
        atomic_write(&backup_path(config_path), &content).await?;
    }
    Ok(())
}

async fn restore_config(config_path: &Path) -> Result<(), String> {
    let backup = backup_path(config_path);
    if backup.exists() {
        let content = tokio::fs::read(&backup)
            .await
            .map_err(|e| format!("读取备份失败: {e}"))?;
        atomic_write(config_path, &content).await?;
        let _ = tokio::fs::remove_file(&backup).await;
        Ok(())
    } else {
        Err("没有找到备份文件，可能此前没有可恢复的原始配置".to_string())
    }
}

// ── 获取 DongX 网关地址（读 settings 中的 server_port） ──

async fn get_dongx_url(state: &AppState) -> String {
    let port_raw = settings_repo::get(&state.db, "server_port")
        .await
        .ok()
        .flatten();
    let port: u16 = port_raw
        .map(|s| s.trim().trim_matches('"').to_string())
        .and_then(|s| s.parse::<u16>().ok())
        .unwrap_or(9842);
    format!("http://127.0.0.1:{}", port)
}

// ── 各客户端配置写入逻辑（合并三段：base_url + api_key + model） ──

async fn write_claude_code(
    config_dir: &Path,
    dongx_url: &str,
    dongx_key: &str,
    model: &str,
) -> Result<(), String> {
    let settings_path = config_dir.join("settings.json");
    let mut settings: serde_json::Value = if settings_path.exists() {
        read_json_file(&settings_path)
            .await
            .unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = settings.as_object_mut() {
        obj.insert(
            "env".to_string(),
            serde_json::json!({
                "ANTHROPIC_BASE_URL": dongx_url,
                "ANTHROPIC_API_KEY": dongx_key,
                "ANTHROPIC_MODEL": model
            }),
        );
        obj.insert("_dongx".to_string(), serde_json::json!(true));
    }

    write_json_file(&settings_path, &settings).await
}

async fn write_codex(
    config_dir: &Path,
    dongx_url: &str,
    dongx_key: &str,
    model: &str,
) -> Result<(), String> {
    use toml_edit::DocumentMut;

    // Codex 鉴权方式：experimental_bearer_token 作为 Bearer token 发给上游
    // 不写 auth.json 的 OPENAI_API_KEY，避免 Codex 拿它去 OpenAI 验证。
    let config_path = config_dir.join("config.toml");
    let existing_text = if config_path.exists() {
        tokio::fs::read_to_string(&config_path)
            .await
            .map_err(|e| format!("Failed to read config.toml: {e}"))?
    } else {
        String::new()
    };

    let mut doc = existing_text
        .parse::<DocumentMut>()
        .map_err(|e| format!("Failed to parse config.toml: {e}"))?;

    doc["model_provider"] = toml_edit::value("dongx");
    doc["model"] = toml_edit::value(model);

    if doc.get("model_providers").is_none() {
        let mut table = toml_edit::Table::new();
        table.set_implicit(true);
        doc["model_providers"] = toml_edit::Item::Table(table);
    }

    if let Some(providers) = doc["model_providers"].as_table_mut() {
        let dongx_entry = providers.entry("dongx");
        let provider_table = dongx_entry.or_insert(toml_edit::Item::Table(toml_edit::Table::new()));
        if let Some(table) = provider_table.as_table_mut() {
            table["name"] = toml_edit::value("DongX Gateway");
            table["base_url"] = toml_edit::value(format!("{}/v1", dongx_url.trim_end_matches('/')));
            table["wire_api"] = toml_edit::value("responses");
            table["experimental_bearer_token"] = toml_edit::value(dongx_key);
            table.remove("requires_openai_auth");
        }

        // 若 legacy 'custom' provider 带 requires_openai_auth，移除以避免鉴权冲突
        if let Some(custom_table) = providers.get_mut("custom") {
            if let Some(t) = custom_table.as_table_mut() {
                if t.contains_key("requires_openai_auth") {
                    t.remove("requires_openai_auth");
                }
            }
        }
    }

    atomic_write(&config_path, doc.to_string().as_bytes()).await?;
    Ok(())
}

async fn write_opencode(
    config_dir: &Path,
    dongx_url: &str,
    dongx_key: &str,
    model: &str,
) -> Result<(), String> {
    let config_path = config_dir.join("opencode.json");
    let mut config: serde_json::Value = if config_path.exists() {
        read_json_file(&config_path)
            .await
            .unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({"$schema": "https://opencode.ai/config.json"})
    };

    if let Some(obj) = config.as_object_mut() {
        let provider = serde_json::json!({
            "npm": "@ai-sdk/openai-compatible",
            "name": "DongX Gateway",
            "options": {
                "baseURL": format!("{}/v1", dongx_url),
                "apiKey": dongx_key
            },
            "models": {
                "dongx-default": { "name": model }
            }
        });
        if let Some(providers) = obj.get_mut("provider").and_then(|v| v.as_object_mut()) {
            providers.insert("dongx".to_string(), provider);
        } else {
            obj.insert(
                "provider".to_string(),
                serde_json::json!({"dongx": provider}),
            );
        }
    }

    write_json_file(&config_path, &config).await
}

async fn write_openclaw(
    config_dir: &Path,
    dongx_url: &str,
    dongx_key: &str,
    model: &str,
) -> Result<(), String> {
    let config_path = config_dir.join("config.json");
    let mut config: serde_json::Value = if config_path.exists() {
        read_json_file(&config_path)
            .await
            .unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = config.as_object_mut() {
        obj.insert(
            "baseUrl".to_string(),
            serde_json::json!(format!("{}/v1", dongx_url)),
        );
        obj.insert("apiKey".to_string(), serde_json::json!(dongx_key));
        obj.insert("model".to_string(), serde_json::json!(model));
        obj.insert("_dongx".to_string(), serde_json::json!(true));
    }

    write_json_file(&config_path, &config).await
}

async fn write_hermes(
    config_dir: &Path,
    dongx_url: &str,
    dongx_key: &str,
    model: &str,
) -> Result<(), String> {
    let config_path = config_dir.join("config.json");
    let mut config: serde_json::Value = if config_path.exists() {
        read_json_file(&config_path)
            .await
            .unwrap_or_else(|_| serde_json::json!({}))
    } else {
        serde_json::json!({})
    };

    if let Some(obj) = config.as_object_mut() {
        if let Some(providers) = obj
            .get_mut("custom_providers")
            .and_then(|v| v.as_array_mut())
        {
            providers.retain(|p| p.get("id").and_then(|v| v.as_str()) != Some("dongx"));
            let mut entry = serde_json::Map::new();
            entry.insert("id".to_string(), serde_json::json!("dongx"));
            entry.insert("name".to_string(), serde_json::json!("DongX Gateway"));
            entry.insert(
                "base_url".to_string(),
                serde_json::json!(format!("{}/v1", dongx_url)),
            );
            entry.insert("api_key".to_string(), serde_json::json!(dongx_key));
            entry.insert("default_model".to_string(), serde_json::json!(model));
            providers.push(serde_json::Value::Object(entry));
        } else {
            let mut entry = serde_json::Map::new();
            entry.insert("id".to_string(), serde_json::json!("dongx"));
            entry.insert("name".to_string(), serde_json::json!("DongX Gateway"));
            entry.insert(
                "base_url".to_string(),
                serde_json::json!(format!("{}/v1", dongx_url)),
            );
            entry.insert("api_key".to_string(), serde_json::json!(dongx_key));
            entry.insert("default_model".to_string(), serde_json::json!(model));
            obj.insert(
                "custom_providers".to_string(),
                serde_json::Value::Array(vec![serde_json::Value::Object(entry)]),
            );
        }
    }

    write_json_file(&config_path, &config).await
}

// ── 检测是否已由 DongX 配置（applied 状态，独立于 available） ──

async fn detect_applied(config_path: &Path, app_name: &str) -> bool {
    if !config_path.exists() {
        return false;
    }
    let content = match tokio::fs::read_to_string(config_path).await {
        Ok(c) => c,
        Err(_) => return false,
    };

    match app_name {
        "claude-code" | "openclaw" => {
            let v: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => return false,
            };
            v.get("_dongx").and_then(|v| v.as_bool()).unwrap_or(false)
        }
        "codex" => content.contains("DongX"),
        "opencode" => {
            let v: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => return false,
            };
            v.pointer("/provider/dongx").is_some()
        }
        "hermes" => {
            let v: serde_json::Value = match serde_json::from_str(&content) {
                Ok(v) => v,
                Err(_) => return false,
            };
            v.get("custom_providers")
                .and_then(|v| v.as_array())
                .and_then(|arr| {
                    arr.iter()
                        .find(|p| p.get("id").and_then(|v| v.as_str()) == Some("dongx"))
                })
                .is_some()
        }
        _ => false,
    }
}

// ── Tauri Commands ──

#[tauri::command]
pub async fn get_client_configs() -> Result<Vec<ClientInfo>, String> {
    let mut apps: Vec<ClientInfo> = Vec::with_capacity(APPS.len());
    for app in APPS.iter() {
        let config_dir = (app.config_dir_fn)();
        let config_path = config_dir.join(app.config_file);
        // installed = 探测到 CLI；available = 已装 CLI 或已有配置文件（可写入）
        let installed = detect_cli(app);
        let available = installed || config_path.exists();
        let applied = detect_applied(&config_path, app.name).await;

        apps.push(ClientInfo {
            name: app.name.to_string(),
            label: app.label.to_string(),
            icon: app.icon.to_string(),
            description: app.description.to_string(),
            config_path: config_path.to_string_lossy().to_string(),
            config_format: app.config_format.to_string(),
            available,
            installed,
            applied,
            download_url: app.download_url.to_string(),
        });
    }

    Ok(apps)
}

#[tauri::command]
pub async fn apply_client_config(
    app_name: String,
    api_key: String,
    model: String,
    state: tauri::State<'_, Arc<AppState>>,
) -> Result<ApplyResult, String> {
    let st: &AppState = state.inner();
    let dongx_url = get_dongx_url(st).await;

    let app_def = APPS
        .iter()
        .find(|a| a.name == app_name)
        .ok_or_else(|| format!("不支持的客户端: {app_name}"))?;

    let config_dir = (app_def.config_dir_fn)();
    let config_path = config_dir.join(app_def.config_file);

    // 写入前先备份原始配置（供「恢复原始配置」使用）
    let _ = backup_config(&config_path).await;

    let result = match app_name.as_str() {
        "claude-code" => write_claude_code(&config_dir, &dongx_url, &api_key, &model).await,
        "codex" => write_codex(&config_dir, &dongx_url, &api_key, &model).await,
        "opencode" => write_opencode(&config_dir, &dongx_url, &api_key, &model).await,
        "openclaw" => write_openclaw(&config_dir, &dongx_url, &api_key, &model).await,
        "hermes" => write_hermes(&config_dir, &dongx_url, &api_key, &model).await,
        _ => return Err(format!("不支持的客户端: {app_name}")),
    };

    match result {
        Ok(()) => Ok(ApplyResult {
            success: true,
            message: format!("配置已写入 {}", config_path.display()),
        }),
        Err(e) => {
            // 写入失败回滚到备份
            let _ = restore_config(&config_path).await;
            Ok(ApplyResult {
                success: false,
                message: e,
            })
        }
    }
}

#[tauri::command]
pub async fn restore_client_config(app_name: String) -> Result<ApplyResult, String> {
    let app_def = APPS
        .iter()
        .find(|a| a.name == app_name)
        .ok_or_else(|| format!("不支持的客户端: {app_name}"))?;

    let config_dir = (app_def.config_dir_fn)();
    let config_path = config_dir.join(app_def.config_file);

    match restore_config(&config_path).await {
        Ok(()) => Ok(ApplyResult {
            success: true,
            message: format!("已恢复 {} 的原始配置", app_def.label),
        }),
        Err(e) => Ok(ApplyResult {
            success: false,
            message: format!("恢复失败: {e}"),
        }),
    }
}

#[tauri::command]
pub async fn get_client_config_content(app_name: String) -> Result<ConfigContent, String> {
    let app_def = APPS
        .iter()
        .find(|a| a.name == app_name)
        .ok_or_else(|| format!("不支持的客户端: {app_name}"))?;

    let config_dir = (app_def.config_dir_fn)();
    let config_path = config_dir.join(app_def.config_file);

    if !config_path.exists() {
        return Ok(ConfigContent {
            exists: false,
            content: String::new(),
            error: None,
        });
    }

    match tokio::fs::read_to_string(&config_path).await {
        Ok(content) => Ok(ConfigContent {
            exists: true,
            content,
            error: None,
        }),
        Err(e) => Ok(ConfigContent {
            exists: true,
            content: String::new(),
            error: Some(format!("读取失败: {e}")),
        }),
    }
}
