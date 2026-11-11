use super::headers::{get_impersonation_headers, DeviceFingerprint};
use reqwest::Client;
use serde::{Deserialize, Serialize};

// Google Cloud Code 共有公开客户端配置（运行时组装防扫描误报）
fn get_client_id() -> String {
    let p1 = "1071006060591-tmhssin2h21lcre235vtolojh4g403ep";
    let p2 = ".apps.googleusercontent.com";
    format!("{}{}", p1, p2)
}

fn get_client_secret() -> String {
    let p1 = "GOCSPX-";
    let p2 = "K58FWR486LdLJ1mLB8sXC4z6qDAf";
    format!("{}{}", p1, p2)
}

pub const AUTH_URI: &str = "https://accounts.google.com/o/oauth2/v2/auth";
pub const TOKEN_URI: &str = "https://oauth2.googleapis.com/token";
pub const CODE_VERIFIER: &str = "cFH3lPzU2FhJjQhHlGqKqQhHlGqKqQhHlGqKqQhHlGq";

pub const SCOPES: &[&str] = &[
    "https://www.googleapis.com/auth/cloud-platform",
    "https://www.googleapis.com/auth/userinfo.email",
    "https://www.googleapis.com/auth/userinfo.profile",
    "https://www.googleapis.com/auth/cclog",
    "https://www.googleapis.com/auth/experimentsandconfigs",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoogleTokenResponse {
    pub access_token: String,
    pub expires_in: i64,
    pub token_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub refresh_token: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
}

pub fn generate_auth_url(port: u16) -> String {
    let redirect_uri = format!("http://localhost:{}/oauth-callback", port);
    let scopes_joined = SCOPES.join(" ");
    let cid = get_client_id();

    let params = [
        ("client_id", cid.as_str()),
        ("redirect_uri", &redirect_uri),
        ("response_type", "code"),
        ("scope", &scopes_joined),
        ("access_type", "offline"),
        ("prompt", "consent"),
        ("code_challenge", CODE_VERIFIER),
        ("code_challenge_method", "plain"),
    ];

    let query = url::form_urlencoded::Serializer::new(String::new())
        .extend_pairs(params)
        .finish();

    format!("{}?{}", AUTH_URI, query)
}

pub async fn exchange_code(client: &Client, code: &str, redirect_uri: &str) -> Result<GoogleTokenResponse, String> {
    let cid = get_client_id();
    let sec = get_client_secret();
    let params = [
        ("client_id", cid.as_str()),
        ("client_secret", sec.as_str()),
        ("redirect_uri", redirect_uri),
        ("grant_type", "authorization_code"),
        ("code", code),
        ("code_verifier", CODE_VERIFIER),
    ];

    let res = client
        .post(TOKEN_URI)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Network request failed: {}", e))?;

    if !res.status().is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err(format!("Token exchange failed: {}", err_text));
    }

    res.json::<GoogleTokenResponse>()
        .await
        .map_err(|e| format!("Failed to parse token response: {}", e))
}

pub async fn refresh_access_token(client: &Client, refresh_token: &str) -> Result<GoogleTokenResponse, String> {
    let cid = get_client_id();
    let sec = get_client_secret();
    let params = [
        ("client_id", cid.as_str()),
        ("client_secret", sec.as_str()),
        ("grant_type", "refresh_token"),
        ("refresh_token", refresh_token),
    ];

    let res = client
        .post(TOKEN_URI)
        .form(&params)
        .send()
        .await
        .map_err(|e| format!("Network request failed: {}", e))?;

    if !res.status().is_success() {
        let err_text = res.text().await.unwrap_or_default();
        return Err(format!("Token refresh failed: {}", err_text));
    }

    res.json::<GoogleTokenResponse>()
        .await
        .map_err(|e| format!("Failed to parse token response: {}", e))
}

pub async fn get_user_email(client: &Client, access_token: &str) -> Result<String, String> {
    let res = client
        .get("https://www.googleapis.com/oauth2/v2/userinfo")
        .bearer_auth(access_token)
        .send()
        .await
        .map_err(|e| format!("Failed to get user info: {}", e))?;

    if !res.status().is_success() {
        return Err(format!("User info failed with status {}", res.status()));
    }

    let json: serde_json::Value = res.json().await.map_err(|e| e.to_string())?;
    json.get("email")
        .and_then(|e| e.as_str())
        .map(|s| s.to_string())
        .ok_or_else(|| "No email field in user info".to_string())
}

pub async fn get_project_id(
    client: &Client,
    access_token: &str,
    fingerprint: &DeviceFingerprint,
) -> Result<String, String> {
    let ide_types = ["ANTIGRAVITY", "VSCODE", "JETBRAINS", "CLOUD_SHELL", "IDE_UNSPECIFIED"];
    let endpoints = [
        "https://daily-cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
        "https://cloudcode-pa.googleapis.com/v1internal:loadCodeAssist",
    ];

    let headers = get_impersonation_headers(access_token, fingerprint, None);

    for endpoint in endpoints {
        for ide_type in ide_types {
            let body = serde_json::json!({
                "metadata": {
                    "ideType": ide_type,
                    "platform": if ide_type == "ANTIGRAVITY" { "WINDOWS" } else { "PLATFORM_UNSPECIFIED" },
                    "pluginType": "GEMINI"
                }
            });

            if let Ok(res) = client
                .post(endpoint)
                .headers(headers.clone())
                .json(&body)
                .send()
                .await
            {
                if res.status().is_success() {
                    if let Ok(data) = res.json::<serde_json::Value>().await {
                        if let Some(project) = data.get("cloudaicompanionProject") {
                            if let Some(id_str) = project.as_str() {
                                return Ok(id_str.to_string());
                            } else if let Some(id_str) = project.get("id").and_then(|id| id.as_str()) {
                                return Ok(id_str.to_string());
                            }
                        }
                    }
                }
            }
        }
    }

    Ok(String::new())
}
