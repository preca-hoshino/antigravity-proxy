use reqwest::header::{HeaderMap, HeaderName, HeaderValue, ACCEPT, AUTHORIZATION, CONTENT_TYPE, USER_AGENT};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use uuid::Uuid;

pub const ANTIGRAVITY_VERSION: &str = "2.8.0";

#[allow(dead_code)]
const PLATFORMS: &[&str] = &["windows/amd64"];
#[allow(dead_code)]
const ARCHITECTURES: &[&str] = &["amd64"];
#[allow(dead_code)]
const IDE_TYPES: &[&str] = &["ANTIGRAVITY"];
#[allow(dead_code)]
const PLATFORM_NAMES: &[&str] = &["WINDOWS"];
#[allow(dead_code)]
const SDK_CLIENTS: &[&str] = &["google-cloud-sdk vscode_cloudshelleditor/0.1"];

const GEMINI_CLI_USER_AGENTS: &[&str] = &[
    "google-api-nodejs-client/9.15.1",
    "google-api-nodejs-client/9.14.0",
    "google-api-nodejs-client/9.13.0",
    "google-api-nodejs-client/10.3.0",
];

const GEMINI_CLI_API_CLIENTS: &[&str] = &[
    "gl-node/22.17.0",
    "gl-node/22.12.0",
    "gl-node/20.18.0",
    "gl-node/21.7.0",
    "gl-node/22.18.0",
];

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientMetadata {
    #[serde(rename = "ideType")]
    pub ide_type: String,
    pub platform: String,
    #[serde(rename = "pluginType")]
    pub plugin_type: String,
    #[serde(rename = "osVersion")]
    pub os_version: String,
    pub arch: String,
    #[serde(rename = "sqmId", skip_serializing_if = "Option::is_none")]
    pub sqm_id: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceFingerprint {
    #[serde(rename = "userAgent")]
    pub user_agent: String,
    #[serde(rename = "quotaUser")]
    pub quota_user: String,
    #[serde(rename = "deviceId")]
    pub device_id: String,
    pub platform: String,
    #[serde(rename = "apiClient")]
    pub api_client: String,
    #[serde(rename = "ideType")]
    pub ide_type: String,
    #[serde(rename = "platformName")]
    pub platform_name: String,
    #[serde(rename = "sessionToken")]
    pub session_token: String,
    #[serde(rename = "cliUserAgent")]
    pub cli_user_agent: String,
    #[serde(rename = "cliApiClient")]
    pub cli_api_client: String,
    #[serde(rename = "clientMetadata")]
    pub client_metadata: ClientMetadata,
    #[serde(rename = "createdAt")]
    pub created_at: i64,
}

fn get_os_release() -> String {
    // 伪装目标始终为 windows/amd64 + WINDOWS，保持 OS 版本与平台严格一致
    "10.0.22631".to_string()
}

pub fn generate_stable_quota_user(email: &str) -> String {
    let mut hasher = Sha256::new();
    hasher.update(email.as_bytes());
    let hash = hex::encode(hasher.finalize());
    format!("device-{}", &hash[..16])
}

pub fn generate_quota_user() -> String {
    let mut bytes = [0u8; 8];
    rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
    format!("device-{}", hex::encode(bytes))
}

pub fn generate_session_token() -> String {
    let mut bytes = [0u8; 16];
    rand::Rng::fill(&mut rand::thread_rng(), &mut bytes);
    hex::encode(bytes)
}

pub fn generate_fingerprint(email: Option<&str>) -> DeviceFingerprint {
    use rand::seq::SliceRandom;
    let mut rng = rand::thread_rng();

    let platform = *PLATFORMS.choose(&mut rng).unwrap_or(&"windows/amd64");
    let arch = "amd64";
    let ide_type = "ANTIGRAVITY";
    let platform_name = "WINDOWS";
    let os_version = get_os_release();

    let api_client = *SDK_CLIENTS.choose(&mut rng).unwrap_or(&SDK_CLIENTS[0]);
    let sqm_id = Uuid::new_v4().to_string();

    let quota_user = match email {
        Some(e) if !e.is_empty() => generate_stable_quota_user(e),
        _ => generate_quota_user(),
    };

    let device_id = match email {
        Some(e) if !e.is_empty() => generate_stable_quota_user(e).replace("device-", ""),
        _ => Uuid::new_v4().to_string(),
    };

    DeviceFingerprint {
        user_agent: format!("antigravity/{} {}", ANTIGRAVITY_VERSION, platform),
        quota_user,
        device_id,
        platform: platform.to_string(),
        api_client: api_client.to_string(),
        ide_type: ide_type.to_string(),
        platform_name: platform_name.to_string(),
        session_token: generate_session_token(),
        cli_user_agent: (*GEMINI_CLI_USER_AGENTS.choose(&mut rng).unwrap_or(&GEMINI_CLI_USER_AGENTS[0])).to_string(),
        cli_api_client: (*GEMINI_CLI_API_CLIENTS.choose(&mut rng).unwrap_or(&GEMINI_CLI_API_CLIENTS[0])).to_string(),
        client_metadata: ClientMetadata {
            ide_type: ide_type.to_string(),
            platform: platform_name.to_string(),
            plugin_type: "GEMINI".to_string(),
            os_version: os_version.clone(),
            arch: arch.to_string(),
            sqm_id: Some(sqm_id),
        },
        created_at: chrono::Utc::now().timestamp_millis(),
    }
}

pub fn get_impersonation_headers(access_token: &str, fingerprint: &DeviceFingerprint, _model: Option<&str>) -> HeaderMap {
    let mut headers = HeaderMap::new();

    headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", access_token)).unwrap());
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json"));
    headers.insert(ACCEPT, HeaderValue::from_static("text/event-stream"));
    headers.insert(USER_AGENT, HeaderValue::from_str(&format!("antigravity/{} windows/amd64", ANTIGRAVITY_VERSION)).unwrap());

    headers.insert(
        HeaderName::from_static("x-goog-api-client"),
        HeaderValue::from_static("google-cloud-sdk vscode_cloudshelleditor/0.1"),
    );

    if let Ok(v) = HeaderValue::from_str(&fingerprint.quota_user) {
        headers.insert(HeaderName::from_static("x-goog-quotauser"), v);
    }
    if let Ok(v) = HeaderValue::from_str(&fingerprint.device_id) {
        headers.insert(HeaderName::from_static("x-client-device-id"), v);
    }

    // 严格按照 TypeScript 端的键定义顺序生成 JSON 字符串:
    // {"ideType":"ANTIGRAVITY","platform":"WINDOWS","pluginType":"GEMINI","osVersion":"...","arch":"amd64"}
    let client_metadata_str = format!(
        r#"{{"ideType":"ANTIGRAVITY","platform":"WINDOWS","pluginType":"GEMINI","osVersion":"{}","arch":"amd64"}}"#,
        fingerprint.client_metadata.os_version
    );

    if let Ok(v) = HeaderValue::from_str(&client_metadata_str) {
        headers.insert(HeaderName::from_static("client-metadata"), v);
    }

    headers
}

pub fn get_gemini_cli_headers(access_token: &str, fingerprint: &DeviceFingerprint) -> HeaderMap {
    let mut headers = HeaderMap::new();

    headers.insert(AUTHORIZATION, HeaderValue::from_str(&format!("Bearer {}", access_token)).unwrap());
    if let Ok(v) = HeaderValue::from_str(&fingerprint.cli_user_agent) {
        headers.insert(USER_AGENT, v);
    }
    if let Ok(v) = HeaderValue::from_str(&fingerprint.cli_api_client) {
        headers.insert(HeaderName::from_static("x-goog-api-client"), v);
    }
    if let Ok(v) = HeaderValue::from_str(&fingerprint.quota_user) {
        headers.insert(HeaderName::from_static("x-goog-quotauser"), v);
    }
    if let Ok(v) = HeaderValue::from_str(&fingerprint.device_id) {
        headers.insert(HeaderName::from_static("x-client-device-id"), v);
    }
    headers.insert(CONTENT_TYPE, HeaderValue::from_static("application/json; charset=utf-8"));

    let cm = &fingerprint.client_metadata;
    let cm_str = format!(
        "ideType={},platform={},pluginType={},osVersion={},arch={}{}",
        cm.ide_type,
        cm.platform,
        cm.plugin_type,
        cm.os_version,
        cm.arch,
        cm.sqm_id.as_ref().map(|s| format!(",sqmId={}", s)).unwrap_or_default()
    );

    if let Ok(v) = HeaderValue::from_str(&cm_str) {
        headers.insert(HeaderName::from_static("client-metadata"), v);
    }

    headers
}
