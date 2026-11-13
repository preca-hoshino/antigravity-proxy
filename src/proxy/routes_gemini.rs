use super::routes_openai::AppState;
use crate::upstream::headers::get_impersonation_headers;
use axum::{
    body::Body,
    extract::{Path, State},
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub async fn list_gemini_models() -> Response {
    let models = serde_json::json!({
        "models": [
            {
                "name": "models/gemini-4-argon",
                "version": "gemini-4-argon-high",
                "displayName": "gemini-4-argon",
                "description": "Gemini 4 Argon High through AntigravityProxy",
                "inputTokenLimit": 1048576,
                "outputTokenLimit": 65535,
                "supportedGenerationMethods": ["generateContent", "streamGenerateContent"]
            },
            {
                "name": "models/gemini-4-hydrogen",
                "version": "gemini-4-hydrogen-high",
                "displayName": "gemini-4-hydrogen",
                "description": "Gemini 4 Hydrogen High through AntigravityProxy",
                "inputTokenLimit": 1048576,
                "outputTokenLimit": 65535,
                "supportedGenerationMethods": ["generateContent", "streamGenerateContent"]
            },
            {
                "name": "models/gemini-3.8-flash-high",
                "version": "gemini-3.8-flash-high",
                "displayName": "gemini-3.8-flash-high",
                "description": "Gemini 3.8 Flash High through AntigravityProxy",
                "inputTokenLimit": 1048576,
                "outputTokenLimit": 65535,
                "supportedGenerationMethods": ["generateContent", "streamGenerateContent"]
            },
            {
                "name": "models/gemini-3.7-flash-high",
                "version": "gemini-3.7-flash-high",
                "displayName": "gemini-3.7-flash-high",
                "description": "Gemini 3.7 Flash High through AntigravityProxy",
                "inputTokenLimit": 1048576,
                "outputTokenLimit": 65535,
                "supportedGenerationMethods": ["generateContent", "streamGenerateContent"]
            },
            {
                "name": "models/gemini-3.1-pro-high",
                "version": "gemini-3.1-pro-high",
                "displayName": "gemini-3.1-pro-high",
                "description": "Gemini 3.1 Pro High through AntigravityProxy",
                "inputTokenLimit": 1048576,
                "outputTokenLimit": 65535,
                "supportedGenerationMethods": ["generateContent", "streamGenerateContent"]
            }
        ]
    });

    Json(models).into_response()
}

pub async fn gemini_native_handler(
    State(state): State<Arc<AppState>>,
    Path(target): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let (model, action) = match target.split_once(':') {
        Some((m, a)) => (m, a),
        None => (target.as_str(), "generateContent"),
    };
    let clean_model = model.trim_start_matches("models/");
    let is_streaming = action.eq_ignore_ascii_case("streamGenerateContent");
    let pool = "sandbox";

    let endpoints: Vec<String> = state.config.endpoints.sandbox.clone();

    let account = match state.account_manager.get_best_account(pool, clean_model, &[]).await {
        Some(acc) => acc,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(serde_json::json!({
                    "error": { "code": 503, "message": "No ready accounts available" }
                })),
            )
                .into_response()
        }
    };

    let project_id = account.project_id.clone().unwrap_or_default();
    let access_token = account.access_token.clone().unwrap_or_default();
    let fp = account.fingerprint.clone().unwrap();

    let antigravity_body = serde_json::json!({
        "project": project_id,
        "model": clean_model,
        "userAgent": "antigravity",
        "requestId": format!("agent-{}", Uuid::new_v4()),
        "requestType": "agent",
        "request": body
    });

    let endpoint = &endpoints[0];
    let headers = get_impersonation_headers(&access_token, &fp, Some(clean_model));

    match state
        .client
        .post(endpoint)
        .headers(headers)
        .json(&antigravity_body)
        .send()
        .await
    {
        Ok(upstream_res) => {
            let status = upstream_res.status();
            if status.is_success() {
                if is_streaming {
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "text/event-stream")
                        .body(Body::from_stream(upstream_res.bytes_stream()))
                        .unwrap();
                } else {
                    let bytes = upstream_res.bytes().await.unwrap_or_default();
                    return Response::builder()
                        .status(StatusCode::OK)
                        .header(header::CONTENT_TYPE, "application/json")
                        .body(Body::from(bytes))
                        .unwrap();
                }
            } else {
                let err_text = upstream_res.text().await.unwrap_or_default();
                return (
                    status,
                    Json(serde_json::json!({
                        "error": { "code": status.as_u16(), "message": err_text }
                    })),
                )
                    .into_response();
            }
        }
        Err(e) => (
            StatusCode::BAD_GATEWAY,
            Json(serde_json::json!({
                "error": { "code": 502, "message": e.to_string() }
            })),
        )
            .into_response(),
    }
}
