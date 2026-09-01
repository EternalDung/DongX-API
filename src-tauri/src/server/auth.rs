use axum::http::HeaderMap;
use sqlx::SqlitePool;

use crate::db::repository::gateway_keys;
use crate::models::GatewayKeyRow;

/// Extract the gateway key from the request headers.
///
/// Two client styles are accepted so the same `sk-dongapi-*` gateway key works
/// across every protocol the data plane exposes:
/// - OpenAI-style: `Authorization: Bearer sk-dongapi-xxxx`
/// - Anthropic-style: `x-api-key: sk-dongapi-xxxx` (Claude SDKs send this)
///
/// The key is DongX's *gateway* key (not the upstream provider key) — it is
/// validated against the local DB the same way regardless of which header
/// carried it.
pub fn extract_gateway_key(headers: &HeaderMap) -> Option<String> {
    // OpenAI-style: `Authorization: Bearer sk-dongapi-xxxx`
    if let Some(auth_header) = headers.get(axum::http::header::AUTHORIZATION) {
        if let Ok(auth_str) = auth_header.to_str() {
            if let Some(key) = auth_str.strip_prefix("Bearer ") {
                if key.starts_with("sk-dongapi-") {
                    return Some(key.to_string());
                }
            }
        }
    }
    // Anthropic-style: `x-api-key: sk-dongapi-xxxx`
    if let Some(key_header) = headers.get("x-api-key") {
        if let Ok(key) = key_header.to_str() {
            if key.starts_with("sk-dongapi-") {
                return Some(key.to_string());
            }
        }
    }
    None
}

/// Validate a gateway key against the database.
///
/// Returns the matching `GatewayKeyRow` only when:
/// - the key exists (plaintext match)
/// - status == 1 (active)
/// - not expired (expires_at in the future, if set)
/// - quota not exhausted (quota_limit == 0 means unlimited)
///
/// Java comparison: like a Spring UserDetailsService that loads + validates
/// credentials and returns the principal (or None) in one shot.
pub async fn validate_gateway_key(pool: &SqlitePool, key: &str) -> Option<GatewayKeyRow> {
    let row = gateway_keys::get_by_key(pool, key).await.ok()??;

    // Status: 0=disabled 1=active 2=expired
    if row.status != 1 {
        return None;
    }

    // Expiry check (RFC3339). Empty or unparseable => treat as no expiry.
    if let Some(exp) = &row.expires_at {
        if !exp.is_empty() {
            if let Ok(t) = chrono::DateTime::parse_from_rfc3339(exp) {
                if chrono::Utc::now() > t.with_timezone(&chrono::Utc) {
                    return None;
                }
            }
        }
    }

    // Quota: quota_limit == 0 means unlimited; otherwise used must be < limit.
    if row.quota_limit > 0 && row.quota_used >= row.quota_limit {
        return None;
    }

    Some(row)
}
