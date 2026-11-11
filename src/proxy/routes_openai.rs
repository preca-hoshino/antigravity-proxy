use super::stream::transform_google_sse_to_openai_stream;
use crate::account::manager::AccountManager;
use crate::config::AppConfig;
use crate::logger::LogBroadcaster;
use crate::upstream::headers::{get_gemini_cli_headers, get_impersonation_headers};
use crate::upstream::transform::{determine_pool, is_sandbox_only_model, transform_to_google_body};
use axum::{
    body::Body,
    extract::State,
    http::{header, StatusCode},
    response::{IntoResponse, Response},
    Json,
};
use reqwest::Client;
use serde_json::Value;
use std::sync::Arc;
use uuid::Uuid;

pub const SUPPORTED_MODELS: &[&str] = &[
    "gemini-3.8-flash",
    "gemini-3.8-flash-high",
    "gemini-3.8-flash-medium",
    "gemini-3.8-flash-low",
    "gemini-3.7-flash",
    "gemini-3.7-flash-high",
    "gemini-3.7-flash-medium",
    "gemini-3.7-flash-low",
    "gemini-3.1-pro-high",
    "gemini-3.1-pro-low",
    "gemini-3-flash",
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "claude-sonnet-4-6",
    "claude-opus-4-6",
    "claude-sonnet-4-5",
];

pub struct AppState {
    pub account_manager: Arc<AccountManager>,
    pub config: Arc<AppConfig>,
    pub client: Client,
    pub broadcaster: Arc<LogBroadcaster>,
}

pub async fn list_models() -> Response {
    let data: Vec<Value> = SUPPORTED_MODELS
        .iter()
        .map(|id| {
            serde_json::json!({
                "id": id,
                "object": "model",
                "created": 1700000000,
                "owned_by": "antigravity"
            })
        })
        .collect();

    Json(serde_json::json!({
        "object": "list",
        "data": data
    }))
    .into_response()
}

pub async fn chat_completions(State(state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let raw_model = body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("gemini-3.8-flash-high")
        .to_string();

    let is_stream = body.get("stream").and_then(|v| v.as_bool()).unwrap_or(false);
    let include_usage = body
        .get("stream_options")
        .and_then(|so| so.get("include_usage"))
        .and_then(|v| v.as_bool())
        .unwrap_or(false);

    let session_id = Uuid::new_v4().to_string();
    let request_id = format!("chatcmpl-{}", Uuid::new_v4().simple());

    let pool = determine_pool(&raw_model);

    let endpoints: Vec<String> = if pool == "cli" {
        state.config.endpoints.cli.clone()
    } else if is_sandbox_only_model(&raw_model) {
        let compatible: Vec<String> = state
            .config
            .endpoints
            .sandbox
            .iter()
            .filter(|ep| ep.contains(".sandbox.googleapis.com"))
            .cloned()
            .collect();
        if !compatible.is_empty() {
            compatible
        } else {
            state.config.endpoints.sandbox.clone()
        }
    } else {
        state.config.endpoints.sandbox.clone()
    };

    let max_attempts = state.config.retry.max_attempts;
    let mut tried_emails = Vec::new();
    let request_start = std::time::Instant::now();

    state.broadcaster.broadcast(
        "request",
        &format!("POST /v1/chat/completions - Model: {} | Stream: {}", raw_model, is_stream),
    );

    for attempt in 1..=max_attempts {
        let account = match state.account_manager.get_best_account(pool, &raw_model, &tried_emails).await {
            Some(acc) => acc,
            None => break,
        };

        tried_emails.push(account.email.clone());
        let project_id = account.project_id.clone().unwrap_or_default();
        let access_token = account.access_token.clone().unwrap_or_default();
        let fp = account.fingerprint.clone().unwrap();

        let endpoint = &endpoints[(attempt - 1) % endpoints.len()];
        let google_body = transform_to_google_body(&body, &project_id, pool == "cli", &session_id, false);
        let target_model = google_body
            .get("model")
            .and_then(|m| m.as_str())
            .unwrap_or(&raw_model);

        let is_claude = target_model.contains("claude");
        let headers = if pool == "cli" && !is_claude {
            get_gemini_cli_headers(&access_token, &fp)
        } else {
            get_impersonation_headers(&access_token, &fp, Some(target_model))
        };

        tracing::info!(
            model = %raw_model,
            target_model = %target_model,
            account = %account.email,
            endpoint = %endpoint,
            attempt = attempt,
            max_attempts = max_attempts,
            "Sending request to Google v1internal"
        );

        match state
            .client
            .post(endpoint)
            .headers(headers)
            .json(&google_body)
            .send()
            .await
        {
            Ok(upstream_res) => {
                let status = upstream_res.status();

                if status.is_success() {
                    state.broadcaster.broadcast(
                        "info",
                        &format!("Upstream response 200 OK from {} for {}", account.email, raw_model),
                    );

                    if is_stream {
                        let stream = transform_google_sse_to_openai_stream(
                            upstream_res.bytes_stream(),
                            raw_model.clone(),
                            request_id.clone(),
                            include_usage,
                            request_start,
                            state.broadcaster.clone(),
                        );

                        return Response::builder()
                            .status(StatusCode::OK)
                            .header(header::CONTENT_TYPE, "text/event-stream")
                            .header(header::CACHE_CONTROL, "no-cache")
                            .header(header::CONNECTION, "keep-alive")
                            .body(Body::from_stream(stream))
                            .unwrap();
                    } else {
                        let text = upstream_res.text().await.unwrap_or_default();
                        let mut full_content = String::new();
                        let mut full_reasoning = String::new();
                        let mut tool_calls = Vec::new();
                        let mut seen_tool_calls: std::collections::HashSet<String> = std::collections::HashSet::new();
                        let mut last_usage_meta: Option<Value> = None;

                        for line in text.lines() {
                            let trimmed = line.trim();
                            if let Some(data_str) = trimmed.strip_prefix("data: ") {
                                if data_str == "[DONE]" {
                                    break;
                                }
                                if let Ok(json) = serde_json::from_str::<Value>(data_str) {
                                    let resp_obj = json.get("response").unwrap_or(&json);
                                    if let Some(um) = resp_obj.get("usageMetadata") {
                                        last_usage_meta = Some(um.clone());
                                    }
                                    if let Some(candidates) = resp_obj.get("candidates").and_then(|c| c.as_array()) {
                                        if let Some(first_cand) = candidates.first() {
                                            if let Some(parts) = first_cand.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()) {
                                                for part in parts {
                                                    if let Some(t) = part.get("text").and_then(|t| t.as_str()) {
                                                        if part.get("thought").and_then(|th| th.as_bool()).unwrap_or(false) {
                                                            full_reasoning.push_str(t);
                                                        } else {
                                                            full_content.push_str(t);
                                                        }
                                                    }
                                                    if let Some(fc) = part.get("functionCall") {
                                                        let name = fc.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                                        let call_id = fc.get("id").and_then(|id| id.as_str()).unwrap_or("call_generated");
                                                        let dedupe_key = format!("{}:{}", call_id, name);

                                                        if !seen_tool_calls.contains(&dedupe_key) {
                                                            seen_tool_calls.insert(dedupe_key);
                                                            let args = fc.get("args").map(|a| {
                                                                if a.is_string() { a.as_str().unwrap().to_string() } else { a.to_string() }
                                                            }).unwrap_or_else(|| "{}".to_string());

                                                            tool_calls.push(serde_json::json!({
                                                                "id": call_id,
                                                                "type": "function",
                                                                "function": {
                                                                    "name": name,
                                                                    "arguments": args
                                                                }
                                                            }));
                                                        }
                                                    }
                                                }
                                            }
                                        }
                                    }
                                }
                            }
                        }

                        let total_duration = request_start.elapsed();
                        let total_secs = total_duration.as_secs_f64();
                        let mut prompt_tok = 0i64;
                        let mut comp_tok = 0i64;
                        let mut total_tok = 0i64;
                        let mut cached_tok = 0i64;
                        let mut thoughts_tok = 0i64;

                        if let Some(ref meta) = last_usage_meta {
                            prompt_tok = meta.get("promptTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
                            comp_tok = meta.get("candidatesTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
                            total_tok = meta.get("totalTokenCount").and_then(|v| v.as_i64()).unwrap_or(prompt_tok + comp_tok);
                            cached_tok = meta.get("cachedContentTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
                            thoughts_tok = meta.get("thoughtsTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
                        }

                        let generated_tok = if thoughts_tok > 0 {
                            comp_tok + thoughts_tok
                        } else if total_tok > prompt_tok && comp_tok == 0 {
                            total_tok - prompt_tok
                        } else {
                            comp_tok
                        };

                        let tps_str = if generated_tok > 0 && total_secs > 0.05 {
                            format!("{:.1} tok/s", (generated_tok as f64) / total_secs)
                        } else {
                            "-".to_string()
                        };

                        let cache_rate_str = if prompt_tok > 0 {
                            format!("{:.1}%", (cached_tok as f64 / prompt_tok as f64) * 100.0)
                        } else {
                            "-".to_string()
                        };

                        let metric_log = format!(
                            "[METRICS] Model: {} | Total Tokens: {} (Prompt: {}, Output: {} [Visible: {}, Thinking: {}], Cache: {}) | Duration: {:.2}s | TTFT: - | Speed: {} | Cache Hit: {}",
                            raw_model, total_tok, prompt_tok, generated_tok, comp_tok, thoughts_tok, cached_tok, total_secs, tps_str, cache_rate_str
                        );
                        tracing::info!(target: "metrics", "{}", metric_log);
                        state.broadcaster.broadcast("info", &metric_log);

                        let mut message_obj = serde_json::json!({
                            "role": "assistant",
                            "content": full_content
                        });

                        if !tool_calls.is_empty() {
                            message_obj.as_object_mut().unwrap().insert("tool_calls".to_string(), Value::Array(tool_calls.clone()));
                        }

                        if !full_reasoning.is_empty() {
                            message_obj.as_object_mut().unwrap().insert("reasoning_content".to_string(), Value::String(full_reasoning));
                        }

                        let finish_reason = if !tool_calls.is_empty() { "tool_calls" } else { "stop" };

                        let mut res_json = serde_json::json!({
                            "id": request_id,
                            "object": "chat.completion",
                            "created": chrono::Utc::now().timestamp(),
                            "model": raw_model,
                            "choices": [{
                                "index": 0,
                                "message": message_obj,
                                "finish_reason": finish_reason
                            }]
                        });

                        if last_usage_meta.is_some() {
                            let usage_obj = serde_json::json!({
                                "prompt_tokens": prompt_tok,
                                "completion_tokens": generated_tok,
                                "total_tokens": total_tok,
                                "prompt_tokens_details": {
                                    "cached_tokens": cached_tok
                                },
                                "completion_tokens_details": {
                                    "reasoning_tokens": thoughts_tok
                                }
                            });
                            res_json.as_object_mut().unwrap().insert("usage".to_string(), usage_obj);
                        }

                        return Json(res_json).into_response();
                    }
                } else if status.as_u16() == 429 {
                    tracing::warn!(account = %account.email, "Google API returned 429 RESOURCE_EXHAUSTED; cooling down account");
                    state.account_manager.mark_cooldown(&account.email, pool, 60_000).await;
                    continue;
                } else {
                    let err_body = upstream_res.text().await.unwrap_or_default();
                    tracing::error!(status = %status, account = %account.email, error = %err_body, "Google API error");
                    state.account_manager.mark_cooldown(&account.email, pool, 30_000).await;
                    continue;
                }
            }
            Err(e) => {
                tracing::error!(account = %account.email, error = %e, "Failed to connect to Google upstream");
                continue;
            }
        }
    }

    (
        StatusCode::BAD_GATEWAY,
        Json(serde_json::json!({
            "error": {
                "message": "All accounts or upstream routes exhausted/temporarily unavailable.",
                "type": "upstream_error",
                "code": 502
            }
        })),
    )
        .into_response()
}
