use serde::{Deserialize, Serialize};

/// Application configuration persisted in SQLite (settings table)
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub struct AppConfig {
    pub listen_host: String,
    pub listen_port: u16,
    pub gateway_api_key: Option<String>,
    pub log_retention_days: i32,
    pub max_log_body_length: i32,
    pub enable_rate_limit: bool,
    pub rate_limit_rpm: i32,
    pub enable_content_scan: bool,
    pub auto_start_server: bool,
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            listen_host: "127.0.0.1".into(),
            listen_port: 9090,
            gateway_api_key: None,
            log_retention_days: 30,
            max_log_body_length: 4096,
            enable_rate_limit: false,
            rate_limit_rpm: 60,
            enable_content_scan: true,
            auto_start_server: true,
        }
    }
}
