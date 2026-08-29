use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::error::{AppError, AppResult};
use crate::security::rules::{CustomRule, CustomRuleRepository};
use crate::AppState;

/// 自定义规则创建/更新载荷（对齐前端 CustomRuleInput）。
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct CustomRuleInput {
    pub rule_type: String, // 'blacklist' | 'whitelist'
    pub category: String,  // 'domain' | 'tool' | 'path' | 'keyword'
    pub pattern: String,
    pub severity: String,  // 'low' | 'medium' | 'high' | 'critical'
    pub action: String,    // 'warn' | 'block'
    pub enabled: bool,
    pub description: Option<String>,
}

/// 将仓储行序列化为前端线形（enabled 由 i64 转 bool）。
fn row_to_value(r: CustomRule) -> serde_json::Value {
    serde_json::json!({
        "id": r.id,
        "rule_type": r.rule_type,
        "category": r.category,
        "pattern": r.pattern,
        "severity": r.severity,
        "action": r.action,
        "enabled": r.enabled != 0,
        "description": r.description,
        "created_at": r.created_at,
    })
}

/// 列出全部自定义规则（含已禁用），供管理页展示。
#[tauri::command]
pub async fn list_custom_rules(
    state: State<'_, Arc<AppState>>,
) -> AppResult<Vec<serde_json::Value>> {
    let rows = CustomRuleRepository::list_all(&state.db).await?;
    Ok(rows.into_iter().map(row_to_value).collect())
}

/// 新建自定义规则，返回新建 id。
#[tauri::command]
pub async fn create_custom_rule(
    input: CustomRuleInput,
    state: State<'_, Arc<AppState>>,
) -> AppResult<serde_json::Value> {
    validate(&input)?;
    let id = Uuid::new_v4().to_string();
    CustomRuleRepository::insert(
        &state.db,
        &id,
        &input.rule_type,
        &input.category,
        &input.pattern,
        &input.severity,
        &input.action,
        if input.enabled { 1 } else { 0 },
        input.description.as_deref(),
    )
    .await?;
    Ok(serde_json::json!({ "id": id, "status": "created" }))
}

/// 更新已有自定义规则（按 id 全量覆盖）。
#[tauri::command]
pub async fn update_custom_rule(
    id: String,
    input: CustomRuleInput,
    state: State<'_, Arc<AppState>>,
) -> AppResult<serde_json::Value> {
    validate(&input)?;
    let n = CustomRuleRepository::update(
        &state.db,
        &id,
        &input.rule_type,
        &input.category,
        &input.pattern,
        &input.severity,
        &input.action,
        if input.enabled { 1 } else { 0 },
        input.description.as_deref(),
    )
    .await?;
    if n == 0 {
        return Err(AppError::NotFound(format!("自定义规则 {} 不存在", id)));
    }
    Ok(serde_json::json!({ "status": "updated" }))
}

/// 删除自定义规则。
#[tauri::command]
pub async fn delete_custom_rule(
    id: String,
    state: State<'_, Arc<AppState>>,
) -> AppResult<()> {
    let n = CustomRuleRepository::delete(&state.db, &id).await?;
    if n == 0 {
        return Err(AppError::NotFound(format!("自定义规则 {} 不存在", id)));
    }
    Ok(())
}

/// 参数校验：规则类型/类别/等级/动作取值合法，匹配模式非空。
fn validate(input: &CustomRuleInput) -> AppResult<()> {
    if input.rule_type != "blacklist" && input.rule_type != "whitelist" {
        return Err(AppError::Validation(
            "规则类型必须为 blacklist 或 whitelist".into(),
        ));
    }
    if !["domain", "tool", "path", "keyword"].contains(&input.category.as_str()) {
        return Err(AppError::Validation("匹配类别必须为 domain/tool/path/keyword".into()));
    }
    if input.pattern.trim().is_empty() {
        return Err(AppError::Validation("匹配模式不能为空".into()));
    }
    if !["low", "medium", "high", "critical"].contains(&input.severity.as_str()) {
        return Err(AppError::Validation("风险等级必须为 low/medium/high/critical".into()));
    }
    if !["warn", "block"].contains(&input.action.as_str()) {
        return Err(AppError::Validation("命中动作必须为 warn/block".into()));
    }
    Ok(())
}
