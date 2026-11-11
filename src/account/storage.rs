use crate::upstream::headers::DeviceFingerprint;
use crate::upstream::quota::ModelQuota;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AntigravityAccount {
    pub email: String,
    #[serde(rename = "refreshToken")]
    pub refresh_token: String,
    #[serde(rename = "accessToken", default)]
    pub access_token: Option<String>,
    #[serde(rename = "expiresAt", default)]
    pub expires_at: Option<i64>,
    #[serde(rename = "projectId", default)]
    pub project_id: Option<String>,

    #[serde(rename = "healthScore", default = "default_health")]
    pub health_score: f64,
    #[serde(rename = "lastUsed", default)]
    pub last_used: i64,
    #[serde(rename = "tokenUsage", default)]
    pub token_usage: i64,
    #[serde(rename = "cooldowns", default)]
    pub cooldowns: HashMap<String, i64>,

    #[serde(default)]
    pub fingerprint: Option<DeviceFingerprint>,

    #[serde(default)]
    pub quota: Option<Vec<ModelQuota>>,
}

fn default_health() -> f64 {
    100.0
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(untagged)]
enum AccountsFileFormat {
    Object {
        accounts: Vec<AntigravityAccount>,
        #[serde(default)]
        strategy: Option<String>,
    },
    Array(Vec<AntigravityAccount>),
}

pub struct AccountStorage {
    file_path: PathBuf,
}

impl AccountStorage {
    pub fn new(path: impl AsRef<Path>) -> Self {
        Self {
            file_path: path.as_ref().to_path_buf(),
        }
    }

    pub fn load_accounts(&self) -> Vec<AntigravityAccount> {
        if !self.file_path.exists() {
            return Vec::new();
        }

        let content = match fs::read_to_string(&self.file_path) {
            Ok(c) => c,
            Err(_) => return Vec::new(),
        };

        match serde_json::from_str::<AccountsFileFormat>(&content) {
            Ok(AccountsFileFormat::Object { accounts, .. }) => accounts,
            Ok(AccountsFileFormat::Array(accounts)) => accounts,
            Err(e) => {
                eprintln!("[AccountStorage] Failed to parse accounts file: {}", e);
                Vec::new()
            }
        }
    }

    pub fn save_accounts(&self, accounts: &[AntigravityAccount]) -> Result<(), String> {
        let payload = serde_json::json!({
            "accounts": accounts,
            "strategy": "hybrid"
        });
        let content = serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())?;
        let temp_file = self.file_path.with_extension("tmp");

        fs::write(&temp_file, &content).map_err(|e| format!("Failed to write temp accounts file: {}", e))?;
        fs::rename(&temp_file, &self.file_path).map_err(|e| format!("Failed to atomic rename accounts file: {}", e))?;

        Ok(())
    }
}
