use axum::http::HeaderMap;

/// Extract and validate the gateway key from Authorization header
/// Expected format: "Bearer sk-dong-xxxx"
pub fn extract_gateway_key(headers: &HeaderMap) -> Option<String> {
    let auth_header = headers.get(axum::http::header::AUTHORIZATION)?;
    let auth_str = auth_header.to_str().ok()?;
    let key = auth_str.strip_prefix("Bearer ")?;
    if key.starts_with("sk-dong-") {
        Some(key.to_string())
    } else {
        None
    }
}

/// Validate a gateway key against the database
/// TODO: Implement hash comparison against gateway_keys table
pub async fn validate_gateway_key(key: &str) -> bool {
    // Placeholder: always valid in dev mode
    !key.is_empty()
}
