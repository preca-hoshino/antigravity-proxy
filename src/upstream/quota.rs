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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UpstreamModelInfo {
    pub id: String,
    pub display_name: Option<String>,
    pub internal_model: Option<String>,
    pub max_input_tokens: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub remaining_fraction: Option<f64>,
    pub reset_time: Option<String>,
    pub tag_title: Option<String>,
    pub recommended: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuotaBucket {
    pub bucket_id: String,
    pub display_name: String,
    pub window: String,
    pub reset_time: Option<String>,
    pub description: Option<String>,
    pub remaining_fraction: Option<f64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct QuotaGroupSummary {
    pub display_name: String,
    pub description: Option<String>,
    pub five_hour: Option<QuotaBucket>,
    pub weekly: Option<QuotaBucket>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MergedModelInfo {
    pub base_id: String,
    pub display_name: String,
    pub tiers: Vec<String>,
    pub max_input_tokens: Option<i64>,
    pub max_output_tokens: Option<i64>,
    pub five_hour_fraction: Option<f64>,
    pub weekly_fraction: Option<f64>,
    pub fallback_fraction: Option<f64>,
    pub tag_title: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FetchAvailableModelsResult {
    pub default_agent_model_id: Option<String>,
    pub models: Vec<UpstreamModelInfo>,
    pub quotas: Vec<ModelQuota>,
    #[serde(default)]
    pub quota_groups: Vec<QuotaGroupSummary>,
}

pub async fn fetch_quota_summary(
    client: &Client,
    access_token: &str,
    fingerprint: &DeviceFingerprint,
    project_id: &str,
) -> Result<Vec<QuotaGroupSummary>, String> {
    if project_id.is_empty() || access_token.is_empty() {
        return Err("Missing project_id or access_token".to_string());
    }

    let candidate_urls = [
        "https://daily-cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
        "https://cloudcode-pa.googleapis.com/v1internal:retrieveUserQuotaSummary",
    ];

    let mut last_error = String::new();
    for url in candidate_urls {
        let headers = get_impersonation_headers(access_token, fingerprint, None);
        let body = serde_json::json!({ "project": project_id });

        match client.post(url).headers(headers).json(&body).send().await {
            Ok(res) if res.status().is_success() => {
                if let Ok(json) = res.json::<serde_json::Value>().await {
                    let mut groups = Vec::new();
                    if let Some(arr) = json.get("groups").and_then(|v| v.as_array()) {
                        for g in arr {
                            let display_name = g
                                .get("displayName")
                                .and_then(|v| v.as_str())
                                .unwrap_or("Unknown")
                                .to_string();
                            let description = g
                                .get("description")
                                .and_then(|v| v.as_str())
                                .map(|s| s.to_string());

                            let mut five_hour = None;
                            let mut weekly = None;

                            if let Some(buckets) = g.get("buckets").and_then(|v| v.as_array()) {
                                for b in buckets {
                                    let window = b
                                        .get("window")
                                        .and_then(|v| v.as_str())
                                        .unwrap_or("")
                                        .to_string();
                                    let bucket = QuotaBucket {
                                        bucket_id: b
                                            .get("bucketId")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string(),
                                        display_name: b
                                            .get("displayName")
                                            .and_then(|v| v.as_str())
                                            .unwrap_or("")
                                            .to_string(),
                                        window: window.clone(),
                                        reset_time: b
                                            .get("resetTime")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string()),
                                        description: b
                                            .get("description")
                                            .and_then(|v| v.as_str())
                                            .map(|s| s.to_string()),
                                        remaining_fraction: b
                                            .get("remainingFraction")
                                            .and_then(|v| v.as_f64()),
                                    };
                                    if window == "5h" {
                                        five_hour = Some(bucket);
                                    } else if window == "weekly" {
                                        weekly = Some(bucket);
                                    }
                                }
                            }

                            groups.push(QuotaGroupSummary {
                                display_name,
                                description,
                                five_hour,
                                weekly,
                            });
                        }
                    }
                    return Ok(groups);
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

fn split_model_tier(id: &str) -> (&str, Option<&str>) {
    for suffix in &["-xhigh", "-high", "-medium", "-low", "-tiered"] {
        if let Some(base) = id.strip_suffix(suffix) {
            return (base, Some(&suffix[1..]));
        }
    }
    (id, None)
}

fn clean_display_name(name: &str) -> String {
    let mut s = name.to_string();
    for suffix in &[" (XHigh)", " (High)", " (Medium)", " (Low)"] {
        if let Some(stripped) = s.strip_suffix(suffix) {
            s = stripped.to_string();
            break;
        }
    }
    s
}

fn tier_sort_rank(tier: &str) -> u8 {
    match tier {
        "low" => 1,
        "medium" => 2,
        "high" => 3,
        "xhigh" => 4,
        "tiered" => 5,
        _ => 9,
    }
}

fn match_quota_group<'a>(
    base_id: &str,
    display_name: &str,
    tag_title: Option<&str>,
    groups: &'a [QuotaGroupSummary],
) -> Option<&'a QuotaGroupSummary> {
    if tag_title == Some("Unavailable") {
        return None;
    }

    let id_lower = base_id.to_lowercase();
    let name_lower = display_name.to_lowercase();

    if id_lower.contains("argon") || name_lower.contains("argon") {
        if let Some(g) = groups
            .iter()
            .find(|g| g.display_name.to_lowercase().contains("argon"))
        {
            return Some(g);
        }
        return groups.iter().find(|g| {
            let dn = g.display_name.to_lowercase();
            dn.contains("gemini") && !dn.contains("argon")
        });
    }

    if id_lower.contains("claude") || id_lower.contains("gpt") {
        return groups.iter().find(|g| {
            let dn = g.display_name.to_lowercase();
            dn.contains("claude") || dn.contains("gpt")
        });
    }

    if id_lower.contains("gemini") {
        return groups.iter().find(|g| {
            let dn = g.display_name.to_lowercase();
            dn.contains("gemini") && !dn.contains("argon")
        });
    }

    None
}

pub fn merge_models_with_quotas(
    models: &[UpstreamModelInfo],
    quota_groups: &[QuotaGroupSummary],
) -> Vec<MergedModelInfo> {
    use std::collections::BTreeMap;

    struct Acc<'a> {
        display_name: Option<String>,
        tiers: Vec<String>,
        max_input_tokens: Option<i64>,
        max_output_tokens: Option<i64>,
        fallback_fraction: Option<f64>,
        tag_title: Option<String>,
        _marker: std::marker::PhantomData<&'a ()>,
    }

    let mut map: BTreeMap<String, Acc<'_>> = BTreeMap::new();

    for m in models {
        let (base_id, tier_opt) = split_model_tier(&m.id);
        let entry = map.entry(base_id.to_string()).or_insert_with(|| Acc {
            display_name: None,
            tiers: Vec::new(),
            max_input_tokens: m.max_input_tokens,
            max_output_tokens: m.max_output_tokens,
            fallback_fraction: m.remaining_fraction,
            tag_title: m.tag_title.clone(),
            _marker: std::marker::PhantomData,
        });

        if let Some(ref dn) = m.display_name {
            let cleaned = clean_display_name(dn);
            if entry.display_name.is_none() || entry.display_name.as_deref() == Some("-") {
                entry.display_name = Some(cleaned);
            }
        }

        if let Some(tier) = tier_opt {
            if !entry.tiers.iter().any(|t| t == tier) {
                entry.tiers.push(tier.to_string());
            }
        }

        if let Some(in_tok) = m.max_input_tokens {
            entry.max_input_tokens = Some(entry.max_input_tokens.unwrap_or(0).max(in_tok));
        }
        if let Some(out_tok) = m.max_output_tokens {
            entry.max_output_tokens = Some(entry.max_output_tokens.unwrap_or(0).max(out_tok));
        }
        if entry.fallback_fraction.is_none() {
            entry.fallback_fraction = m.remaining_fraction;
        }
        if entry.tag_title.is_none() {
            entry.tag_title = m.tag_title.clone();
        }
    }

    let mut merged = Vec::new();
    for (base_id, mut acc) in map {
        acc.tiers.sort_by_key(|t| tier_sort_rank(t));
        let display_name = acc.display_name.unwrap_or_else(|| "-".to_string());
        let matched_group = match_quota_group(&base_id, &display_name, acc.tag_title.as_deref(), quota_groups);

        let five_hour_fraction = matched_group
            .and_then(|g| g.five_hour.as_ref())
            .and_then(|b| b.remaining_fraction);
        let weekly_fraction = matched_group
            .and_then(|g| g.weekly.as_ref())
            .and_then(|b| b.remaining_fraction);

        merged.push(MergedModelInfo {
            base_id,
            display_name,
            tiers: acc.tiers,
            max_input_tokens: acc.max_input_tokens,
            max_output_tokens: acc.max_output_tokens,
            five_hour_fraction,
            weekly_fraction,
            fallback_fraction: acc.fallback_fraction,
            tag_title: acc.tag_title,
        });
    }

    merged
}

pub fn format_reset_remaining(iso: Option<&str>) -> String {
    let Some(iso_str) = iso else {
        return String::new();
    };
    let Ok(dt) = chrono::DateTime::parse_from_rfc3339(iso_str) else {
        return String::new();
    };
    let secs = (dt.with_timezone(&chrono::Utc) - chrono::Utc::now()).num_seconds();
    if secs <= 0 {
        return "ready".to_string();
    }
    let days = secs / 86400;
    let hours = (secs % 86400) / 3600;
    let mins = (secs % 3600) / 60;
    if days > 0 {
        format!("{}d{}h", days, hours)
    } else if hours > 0 {
        format!("{}h{}m", hours, mins)
    } else {
        format!("{}m", mins)
    }
}

pub fn print_models_summary_table(email: &str, res: &FetchAvailableModelsResult) {
    use colored::Colorize;
    use comfy_table::{presets::UTF8_FULL, Cell, Color, Row, Table};

    let merged = merge_models_with_quotas(&res.models, &res.quota_groups);

    println!();
    println!(
        "  {} {} (Merged: {} models from {} variants, Default: {})",
        "📡 Upstream Available Models for".bright_cyan().bold(),
        email.bright_yellow(),
        merged.len().to_string().bright_green(),
        res.models.len().to_string().cyan(),
        res.default_agent_model_id.as_deref().unwrap_or("-").cyan()
    );

    if !res.quota_groups.is_empty() {
        let mut group_summaries = Vec::new();
        for g in &res.quota_groups {
            let mut parts = Vec::new();
            if let Some(ref b) = g.five_hour {
                if let Some(f) = b.remaining_fraction {
                    let reset = format_reset_remaining(b.reset_time.as_deref());
                    let r_str = if reset.is_empty() { String::new() } else { format!(" ({})", reset) };
                    parts.push(format!("5h: {:.1}%{}", f * 100.0, r_str));
                }
            }
            if let Some(ref b) = g.weekly {
                if let Some(f) = b.remaining_fraction {
                    let reset = format_reset_remaining(b.reset_time.as_deref());
                    let r_str = if reset.is_empty() { String::new() } else { format!(" ({})", reset) };
                    parts.push(format!("Weekly: {:.1}%{}", f * 100.0, r_str));
                }
            }
            if !parts.is_empty() {
                group_summaries.push(format!("{}: [{}]", g.display_name.bright_white(), parts.join(", ")));
            }
        }
        if !group_summaries.is_empty() {
            println!("  {} {}", "⏱ Quota Windows:".bright_yellow(), group_summaries.join("  |  "));
        }
    }

    let mut table = Table::new();
    table.load_preset(UTF8_FULL);
    table.set_header(vec![
        Cell::new("Model ID").fg(Color::Cyan),
        Cell::new("Display Name").fg(Color::White),
        Cell::new("Tiers").fg(Color::Blue),
        Cell::new("Max In / Out").fg(Color::Yellow),
        Cell::new("5h Limit").fg(Color::Green),
        Cell::new("Weekly Limit").fg(Color::Green),
        Cell::new("Tag").fg(Color::Magenta),
    ]);

    let fmt_pct = |frac: Option<f64>| -> (String, Color) {
        match frac {
            Some(f) => {
                let pct = f * 100.0;
                let color = if pct >= 60.0 {
                    Color::Green
                } else if pct >= 20.0 {
                    Color::Yellow
                } else {
                    Color::Red
                };
                (format!("{:.1}%", pct), color)
            }
            None => ("-".to_string(), Color::DarkGrey),
        }
    };

    for m in &merged {
        let tiers_str = if m.tiers.is_empty() {
            "-".to_string()
        } else {
            m.tiers.join(", ")
        };
        let max_in = m.max_input_tokens.map(|v| format!("{}K", v / 1024)).unwrap_or_else(|| "-".to_string());
        let max_out = m.max_output_tokens.map(|v| format!("{}K", v / 1024)).unwrap_or_else(|| "-".to_string());

        let (h5_str, h5_color) = fmt_pct(m.five_hour_fraction);
        let (wk_str, wk_color) = if m.weekly_fraction.is_some() {
            fmt_pct(m.weekly_fraction)
        } else if m.five_hour_fraction.is_none() && m.tag_title.as_deref() != Some("Unavailable") {
            fmt_pct(m.fallback_fraction)
        } else {
            ("-".to_string(), Color::DarkGrey)
        };

        let tag_str = m.tag_title.as_deref().unwrap_or("-");

        table.add_row(Row::from(vec![
            Cell::new(&m.base_id),
            Cell::new(&m.display_name),
            Cell::new(tiers_str),
            Cell::new(format!("{} / {}", max_in, max_out)),
            Cell::new(h5_str).fg(h5_color),
            Cell::new(wk_str).fg(wk_color),
            Cell::new(tag_str),
        ]));
    }

    println!("{table}");
}

pub async fn fetch_available_models_raw(
    client: &Client,
    access_token: &str,
    fingerprint: &DeviceFingerprint,
    project_id: &str,
) -> Result<FetchAvailableModelsResult, String> {
    if project_id.is_empty() || access_token.is_empty() {
        return Err("Missing project_id or access_token".to_string());
    }

    let candidate_urls = [
        "https://daily-cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
        "https://cloudcode-pa.googleapis.com/v1internal:fetchAvailableModels",
    ];

    let mut last_error = String::new();

    for url in candidate_urls {
        let headers = get_impersonation_headers(access_token, fingerprint, None);
        let body = serde_json::json!({ "project": project_id });

        match client.post(url).headers(headers).json(&body).send().await {
            Ok(res) if res.status().is_success() => {
                if let Ok(json) = res.json::<serde_json::Value>().await {
                    let default_agent_model_id = json
                        .get("defaultAgentModelId")
                        .and_then(|v| v.as_str())
                        .map(|s| s.to_string());

                    let mut models = Vec::new();
                    if let Some(obj) = json.get("models").and_then(|v| v.as_object()) {
                        for (k, v) in obj {
                            let quota_info = v.get("quotaInfo");
                            let remaining_fraction = quota_info
                                .and_then(|q| q.get("remainingFraction"))
                                .and_then(|f| f.as_f64());
                            let reset_time = quota_info
                                .and_then(|q| {
                                    q.get("resetTime")
                                        .or_else(|| q.get("quotaResetTime"))
                                        .or_else(|| q.get("nextResetTime"))
                                })
                                .and_then(|s| s.as_str())
                                .map(|s| s.to_string());

                            models.push(UpstreamModelInfo {
                                id: k.clone(),
                                display_name: v
                                    .get("displayName")
                                    .and_then(|s| s.as_str())
                                    .map(|s| s.to_string()),
                                internal_model: v
                                    .get("model")
                                    .and_then(|s| s.as_str())
                                    .map(|s| s.to_string()),
                                max_input_tokens: v.get("maxTokens").and_then(|n| n.as_i64()),
                                max_output_tokens: v.get("maxOutputTokens").and_then(|n| n.as_i64()),
                                remaining_fraction,
                                reset_time,
                                tag_title: v
                                    .get("tagTitle")
                                    .and_then(|s| s.as_str())
                                    .map(|s| s.to_string()),
                                recommended: v
                                    .get("recommended")
                                    .and_then(|b| b.as_bool())
                                    .unwrap_or(false),
                            });
                        }
                    }
                    models.sort_by(|a, b| a.id.cmp(&b.id));

                    let quotas = parse_quota_response(&json);
                    let quota_groups = fetch_quota_summary(client, access_token, fingerprint, project_id)
                        .await
                        .unwrap_or_default();

                    if !models.is_empty() || !quotas.is_empty() {
                        return Ok(FetchAvailableModelsResult {
                            default_agent_model_id,
                            models,
                            quotas,
                            quota_groups,
                        });
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

pub async fn fetch_quota(
    client: &Client,
    access_token: &str,
    fingerprint: &DeviceFingerprint,
    project_id: &str,
) -> Result<Vec<ModelQuota>, String> {
    let res = fetch_available_models_raw(client, access_token, fingerprint, project_id).await?;
    Ok(res.quotas)
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

        let allowed_patterns = ["Claude", "Anthropic", "GPT", "Gemini", "Argon", "Hydrogen", "chat", "tab_flash", "MODEL_PLACEHOLDER"];
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_merge_models_with_quotas() {
        let models = vec![
            UpstreamModelInfo {
                id: "gemini-4-hydrogen-xhigh".to_string(),
                display_name: Some("Gemini 4 Hydrogen (XHigh)".to_string()),
                internal_model: None,
                max_input_tokens: Some(1048576),
                max_output_tokens: Some(262144),
                remaining_fraction: Some(0.58),
                reset_time: None,
                tag_title: Some("New".to_string()),
                recommended: false,
            },
            UpstreamModelInfo {
                id: "gemini-4-hydrogen-low".to_string(),
                display_name: Some("Gemini 4 Hydrogen (Low)".to_string()),
                internal_model: None,
                max_input_tokens: Some(1048576),
                max_output_tokens: Some(262144),
                remaining_fraction: Some(0.58),
                reset_time: None,
                tag_title: Some("New".to_string()),
                recommended: false,
            },
            UpstreamModelInfo {
                id: "gemini-4-argon-high".to_string(),
                display_name: Some("Gemini 4 Argon (High)".to_string()),
                internal_model: None,
                max_input_tokens: Some(1048576),
                max_output_tokens: Some(65536),
                remaining_fraction: Some(0.91),
                reset_time: None,
                tag_title: Some("New".to_string()),
                recommended: false,
            },
            UpstreamModelInfo {
                id: "gemini-4-argon-medium".to_string(),
                display_name: Some("Gemini 4 Argon (Medium)".to_string()),
                internal_model: None,
                max_input_tokens: Some(1048576),
                max_output_tokens: Some(65536),
                remaining_fraction: Some(0.91),
                reset_time: None,
                tag_title: Some("New".to_string()),
                recommended: false,
            },
        ];

        let groups = vec![
            QuotaGroupSummary {
                display_name: "Gemini Models".to_string(),
                description: None,
                five_hour: Some(QuotaBucket {
                    window: "5h".to_string(),
                    remaining_fraction: Some(0.53),
                    ..Default::default()
                }),
                weekly: Some(QuotaBucket {
                    window: "weekly".to_string(),
                    remaining_fraction: Some(0.18),
                    ..Default::default()
                }),
            },
            QuotaGroupSummary {
                display_name: "Gemini 4 Argon".to_string(),
                description: None,
                five_hour: None,
                weekly: Some(QuotaBucket {
                    window: "weekly".to_string(),
                    remaining_fraction: Some(0.91),
                    ..Default::default()
                }),
            },
        ];

        let merged = merge_models_with_quotas(&models, &groups);
        assert_eq!(merged.len(), 2);

        let argon = merged.iter().find(|m| m.base_id == "gemini-4-argon").unwrap();
        assert_eq!(argon.display_name, "Gemini 4 Argon");
        assert_eq!(argon.tiers, vec!["medium", "high"]);
        assert_eq!(argon.five_hour_fraction, None);
        assert_eq!(argon.weekly_fraction, Some(0.91));

        let hydrogen = merged.iter().find(|m| m.base_id == "gemini-4-hydrogen").unwrap();
        assert_eq!(hydrogen.display_name, "Gemini 4 Hydrogen");
        assert_eq!(hydrogen.tiers, vec!["low", "xhigh"]);
        assert_eq!(hydrogen.five_hour_fraction, Some(0.53));
        assert_eq!(hydrogen.weekly_fraction, Some(0.18));
    }
}
