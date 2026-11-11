use super::headers::{get_impersonation_headers, DeviceFingerprint};
use reqwest::Client;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct ModelQuota {
    #[serde(rename = "groupName", default)]
    pub group_name: String,
    #[serde(rename = "modelId", default)]
    pub model_id: String,
    #[serde(rename = "remainingFraction", default)]
    pub remaining_fraction: Option<f64>,
    #[serde(default)]
    pub percentage: Option<f64>,
    #[serde(rename = "resetTime", default)]
    pub reset_time: Option<String>,
    #[serde(rename = "quotaLeft", default)]
    pub quota_left: Option<String>,
    #[serde(rename = "resetIn", default)]
    pub reset_in: Option<String>,
    #[serde(default)]
    pub limit: Option<String>,
    #[serde(default)]
    pub usage: Option<String>,
    #[serde(rename = "limitName", default)]
    pub limit_name: Option<String>,
}

impl ModelQuota {
    pub fn get_percentage(&self) -> f64 {
        if let Some(p) = self.percentage {
            p
        } else if let Some(rf) = self.remaining_fraction {
            rf * 100.0
        } else {
            100.0
        }
    }
}

pub async fn fetch_quota(
    client: &Client,
    access_token: &str,
    fingerprint: &DeviceFingerprint,
    project_id: &str,
) -> Result<Vec<ModelQuota>, String> {
    if project_id.is_empty() || access_token.is_empty() {
        return Err("Missing project_id or access_token".to_string());
    }

    // 优先尝试与 GitHub 各开源实现相同的 Sandbox 端点，兼顾日常动态配额与分级模型
    let candidate_urls = [
        "https://daily-cloudcode-pa.sandbox.googleapis.com/v1internal:fetchAvailableModels",
        "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
    ];

    let mut last_error = String::new();

    for url in candidate_urls {
        let mut headers = get_impersonation_headers(access_token, fingerprint, None);
        headers.insert(reqwest::header::USER_AGENT, reqwest::header::HeaderValue::from_static("antigravity"));
        let body = serde_json::json!({ "project": project_id });

        match client.post(url).headers(headers).json(&body).send().await {
            Ok(res) if res.status().is_success() => {
                if let Ok(json) = res.json::<serde_json::Value>().await {
                    let quotas = parse_quota_response(&json);
                    if !quotas.is_empty() {
                        return Ok(quotas);
                    }
                }
            }
            Ok(res) => {
                last_error = format!("Endpoint {} returned {}", url, res.status());
            }
            Err(e) => {
                last_error = format!("Endpoint {} request error: {}", url, e);
            }
        }
    }

    Err(last_error)
}

fn get_next_midnight_pt() -> String {
    let now = chrono::Utc::now();
    // America/Los_Angeles offset is approx -7h or -8h. Defaulting safely to +24h if absent
    (now + chrono::Duration::hours(24)).to_rfc3339()
}

fn parse_quota_response(data: &serde_json::Value) -> Vec<ModelQuota> {
    let mut entries: Vec<(String, serde_json::Value)> = Vec::new();

    if let Some(arr) = data.get("availableModels").and_then(|v| v.as_array()) {
        for m in arr {
            let label = m.get("model")
                .and_then(|v| v.get("name"))
                .or_else(|| m.get("displayName"))
                .or_else(|| m.get("displayMetadata").and_then(|dm| dm.get("label")))
                .and_then(|s| s.as_str())
                .unwrap_or("Unknown")
                .to_string();
            entries.push((label, m.clone()));
        }
    } else if let Some(obj) = data.get("models").and_then(|v| v.as_object()) {
        for (k, v) in obj {
            entries.push((k.clone(), v.clone()));
        }
    }

    let mut groups: std::collections::HashMap<String, ModelQuota> = std::collections::HashMap::new();

    for (key, m) in entries {
        let quota_info = match m.get("quotaInfo") {
            Some(q) => q,
            None => continue,
        };

        let label = m.get("displayMetadata")
            .and_then(|dm| dm.get("label"))
            .or_else(|| m.get("displayName"))
            .or_else(|| m.get("model").and_then(|v| v.get("name")))
            .and_then(|s| s.as_str())
            .unwrap_or(&key)
            .to_string();

        let lower_label = label.to_lowercase();
        if label == "Unknown" || lower_label == "unknown" {
            continue;
        }

        let allowed_patterns = ["Claude", "Anthropic", "GPT", "Gemini", "chat", "tab_flash", "MODEL_PLACEHOLDER"];
        let is_allowed = allowed_patterns.iter().any(|p| label.contains(p) || key.contains(&p.to_lowercase()));
        if !is_allowed {
            continue;
        }

        let remaining_fraction = quota_info.get("remainingFraction").and_then(|v| v.as_f64()).unwrap_or(0.0);
        let limit_name = quota_info.get("limitName").and_then(|v| v.as_str()).unwrap_or(&label).to_string();

        let model_id = key.replace("models/", "");
        let percentage = remaining_fraction * 100.0;

        let mut reset_time = quota_info.get("quotaResetTime")
            .or_else(|| quota_info.get("resetTime"))
            .or_else(|| quota_info.get("nextResetTime"))
            .or_else(|| quota_info.get("quota_reset_time"))
            .and_then(|v| v.as_str())
            .map(|s| s.to_string())
            .unwrap_or_default();

        if reset_time.is_empty() {
            reset_time = get_next_midnight_pt();
        }

        if let Some(existing) = groups.get_mut(&limit_name) {
            if !existing.group_name.contains(&label) {
                existing.group_name = format!("{} / {}", existing.group_name, label);
            }
        } else {
            let group_name = label.clone();
            let quota_left = format!("{:.0}%", percentage);

            groups.insert(limit_name.clone(), ModelQuota {
                group_name,
                model_id: model_id.clone(),
                remaining_fraction: Some(remaining_fraction),
                percentage: Some(percentage),
                reset_time: Some(reset_time),
                quota_left: Some(quota_left),
                reset_in: None,
                limit: Some("Unknown".to_string()),
                usage: Some("Unknown".to_string()),
                limit_name: Some(limit_name),
            });
        }
    }

    let mut result: Vec<ModelQuota> = groups.into_values().collect();
    result.sort_by(|a, b| a.group_name.cmp(&b.group_name));
    result
}
