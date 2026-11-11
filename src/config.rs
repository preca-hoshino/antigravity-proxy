use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AppConfig {
    #[serde(default)]
    pub server: ServerConfig,
    #[serde(default)]
    pub rotation: RotationConfig,
    #[serde(default)]
    pub retry: RetryConfig,
    #[serde(default)]
    pub quota: QuotaConfig,
    #[serde(default)]
    pub endpoints: EndpointsConfig,
    #[serde(default)]
    pub features: FeaturesConfig,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    #[serde(default = "default_host")]
    pub host: String,
    #[serde(default = "default_port")]
    pub port: u16,
    #[serde(default = "default_true")]
    pub enable_web: bool,
    #[serde(default = "default_accounts_path")]
    pub accounts_file: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RotationConfig {
    #[serde(default = "default_strategy")]
    pub strategy: String,
    #[serde(default = "default_cooldown_ms")]
    pub cooldown_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RetryConfig {
    #[serde(default = "default_max_attempts")]
    pub max_attempts: usize,
    #[serde(default = "default_timeout_ms")]
    pub default_timeout_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QuotaConfig {
    #[serde(default = "default_refresh_interval_ms")]
    pub refresh_interval_ms: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct EndpointsConfig {
    #[serde(default = "default_sandbox_endpoints")]
    pub sandbox: Vec<String>,
    #[serde(default = "default_cli_endpoints")]
    pub cli: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FeaturesConfig {
    #[serde(default = "default_true")]
    pub jitter_enabled: bool,
    #[serde(default = "default_jitter_min")]
    pub jitter_min_ms: u64,
    #[serde(default = "default_jitter_max")]
    pub jitter_max_ms: u64,
}

fn default_host() -> String {
    "127.0.0.1".to_string()
}
fn default_port() -> u16 {
    8317
}
fn default_true() -> bool {
    true
}
fn default_accounts_path() -> String {
    "antigravity-accounts.json".to_string()
}
fn default_strategy() -> String {
    "hybrid".to_string()
}
fn default_cooldown_ms() -> u64 {
    60_000
}
fn default_max_attempts() -> usize {
    5
}
fn default_timeout_ms() -> u64 {
    300_000
}
fn default_refresh_interval_ms() -> u64 {
    300_000
}
fn default_jitter_min() -> u64 {
    100
}
fn default_jitter_max() -> u64 {
    500
}

fn default_sandbox_endpoints() -> Vec<String> {
    vec![
        "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:streamGenerateContent?alt=sse".to_string(),
        "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse".to_string(),
        "https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse".to_string(),
    ]
}

fn default_cli_endpoints() -> Vec<String> {
    vec![
        "https://cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse".to_string(),
        "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:streamGenerateContent?alt=sse".to_string(),
        "https://daily-cloudcode-pa.googleapis.com/v1internal:streamGenerateContent?alt=sse".to_string(),
    ]
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: default_host(),
            port: default_port(),
            enable_web: true,
            accounts_file: default_accounts_path(),
        }
    }
}

impl Default for RotationConfig {
    fn default() -> Self {
        Self {
            strategy: default_strategy(),
            cooldown_ms: default_cooldown_ms(),
        }
    }
}

impl Default for RetryConfig {
    fn default() -> Self {
        Self {
            max_attempts: default_max_attempts(),
            default_timeout_ms: default_timeout_ms(),
        }
    }
}

impl Default for QuotaConfig {
    fn default() -> Self {
        Self {
            refresh_interval_ms: default_refresh_interval_ms(),
        }
    }
}

impl Default for EndpointsConfig {
    fn default() -> Self {
        Self {
            sandbox: default_sandbox_endpoints(),
            cli: default_cli_endpoints(),
        }
    }
}

impl Default for FeaturesConfig {
    fn default() -> Self {
        Self {
            jitter_enabled: true,
            jitter_min_ms: default_jitter_min(),
            jitter_max_ms: default_jitter_max(),
        }
    }
}

impl Default for AppConfig {
    fn default() -> Self {
        Self {
            server: ServerConfig::default(),
            rotation: RotationConfig::default(),
            retry: RetryConfig::default(),
            quota: QuotaConfig::default(),
            endpoints: EndpointsConfig::default(),
            features: FeaturesConfig::default(),
        }
    }
}

impl AppConfig {
    pub fn load_or_create(path: &Path) -> Self {
        if path.exists() {
            if let Ok(content) = std::fs::read_to_string(path) {
                if let Ok(cfg) = toml::from_str::<AppConfig>(&content) {
                    return cfg;
                }
            }
        }

        let default_cfg = AppConfig::default();
        if let Ok(toml_str) = toml::to_string_pretty(&default_cfg) {
            let _ = std::fs::write(path, toml_str);
        }
        default_cfg
    }
}
