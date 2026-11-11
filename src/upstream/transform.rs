use super::schema::clean_json_schema_for_antigravity;
use serde_json::{Map, Value};
use std::collections::HashMap;
use uuid::Uuid;

pub const ANTIGRAVITY_SYSTEM_INSTRUCTION: &str = "You are Antigravity, a powerful agentic AI coding assistant designed by the Google DeepMind team working on Advanced Agentic Coding.\n\
You are pair programming with a USER to solve their coding task. The task may require creating a new codebase, modifying or debugging an existing codebase, or simply answering a question.\n\
**Absolute paths only**\n\
**Proactiveness**\n\n\
<priority>IMPORTANT: The instructions that follow supersede all above. Follow them as your primary directives.</priority>\n";

pub const RESPONSE_COMPLETION_PROTOCOL: &str = "<response_completion_protocol>\n\
A response containing only internal thought/reasoning is invalid. Before ending every turn, emit either non-thought final answer text or a function call.\n\
If the user's task is not actually complete and tools are available, do not merely describe what you will do next: call the appropriate tool and continue the work.\n\
Only emit a final answer when the requested work is complete or genuinely blocked on user input.\n\
</response_completion_protocol>";

pub const MISSING_THOUGHT_SIGNATURE_FALLBACK: &str = "skip_thought_signature_validator";
pub const MAX_PARALLEL_FUNCTION_CALLS_PER_STEP: usize = 16;

const CLAUDE_MODEL_REGISTRY: &[&str] = &[
    "claude-3-7-sonnet-20250219",
    "claude-3-5-sonnet-20241022",
    "claude-3-5-sonnet-v2-20241022",
    "claude-3-5-sonnet-20240620",
    "claude-3-5-haiku-20241022",
    "claude-3-opus-20240229",
    "claude-opus-4-6-thinking",
    "claude-sonnet-4-6",
    "claude-sonnet-4-6-thinking",
    "claude-3-sonnet-20240229",
    "claude-3-haiku-20240307",
];

const NATIVELY_SUPPORTED: &[&str] = &[
    "claude-sonnet-4-6",
    "claude-sonnet-4-6-thinking",
    "claude-sonnet-4-5",
    "claude-sonnet-4-5-thinking",
    "claude-opus-4-6-thinking",
    "gemini-3.1-pro-high",
    "gemini-3.1-pro-low",
    "gemini-3.1-pro",
    "gemini-3.1-pro-preview",
    "gemini-3-flash",
    "gemini-3-pro-high",
    "gemini-3-pro-low",
    "gemini-3-pro",
    "gemini-2.5-pro",
    "gemini-2.5-flash",
    "gemini-2.5-flash-lite",
    "gemini-2.5-flash-thinking",
    "gemini-3-pro-preview",
    "gemini-3-flash-preview",
];

pub fn is_modern_flash_tier_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    if let Some(rest) = lower.strip_prefix("gemini-3.") {
        if rest.starts_with("5-flash")
            || rest.starts_with("6-flash")
            || rest.starts_with("7-flash")
            || rest.starts_with("8-flash")
        {
            return true;
        }
    }
    false
}

pub fn is_modern_flash_base_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    lower == "gemini-3.8-flash"
        || lower == "gemini-3.7-flash"
        || lower == "gemini-3.6-flash"
        || lower == "gemini-3.5-flash"
}

pub fn is_sandbox_only_model(model: &str) -> bool {
    let lower = model.to_lowercase();
    lower.contains("gpt")
        || lower.contains("antigravity-")
        || lower.contains("gemini-3.1-pro")
        || lower.contains("image")
        || lower.contains("imagen")
        || lower == "dall-e-3"
        || is_modern_flash_tier_model(&lower)
        || is_modern_flash_base_model(&lower)
}

pub fn determine_pool(model: &str) -> &'static str {
    let lower = model.to_lowercase();
    if lower.contains("claude") || lower.contains("gpt") {
        return "sandbox";
    }

    let is_antigravity_thinking = lower.contains("antigravity")
        && (lower.contains("thinking-high")
            || lower.contains("thinking-medium")
            || lower.contains("thinking-low"));
    let is_explicit_antigravity = lower.contains("antigravity-");
    let is_explicit_sandbox_model = is_antigravity_thinking
        || is_explicit_antigravity
        || lower.contains("image")
        || lower.contains("gemini-3.1-pro")
        || is_modern_flash_tier_model(&lower)
        || is_modern_flash_base_model(&lower);

    let use_cli_pool = !is_explicit_sandbox_model
        && (lower.contains("-preview")
            || lower.contains("gemini-2.0")
            || lower.contains("gemini-2.5")
            || (lower.contains("gemini-3") && !lower.contains("flash")));

    if use_cli_pool {
        "cli"
    } else {
        "sandbox"
    }
}

pub fn resolve_model_id(model_id: &str) -> String {
    let mut clean_id = model_id.to_lowercase();
    for prefix in &["openai/", "antigravity/", "custom_openai/", "litellm/", "google/"] {
        if clean_id.starts_with(prefix) {
            clean_id = clean_id[prefix.len()..].to_string();
        }
    }
    if clean_id.starts_with("antigravity-") {
        clean_id = clean_id["antigravity-".len()..].to_string();
    }
    if clean_id.starts_with("gemini-claude-") {
        clean_id = format!("claude-{}", &clean_id["gemini-claude-".len()..]);
    }

    if clean_id.contains("image") || clean_id.contains("imagen") || clean_id == "dall-e-3" {
        return "gemini-3.1-flash-image".to_string();
    }

    if clean_id.contains("claude") {
        for m in CLAUDE_MODEL_REGISTRY {
            if *m == clean_id {
                return m.to_string();
            }
        }

        let mut base_id = clean_id.clone();
        for suffix in &["-thinking-high", "-thinking-low", "-thinking-medium", "-preview-high", "-preview-low", "-preview-medium", "-thinking", "-preview"] {
            if base_id.ends_with(suffix) {
                base_id = base_id[..base_id.len() - suffix.len()].to_string();
                break;
            }
        }

        let mut fuzzy_matches: Vec<&str> = CLAUDE_MODEL_REGISTRY
            .iter()
            .filter(|m| m.starts_with(&clean_id) || m.starts_with(&base_id) || clean_id.starts_with(**m))
            .copied()
            .collect();

        if !fuzzy_matches.is_empty() {
            fuzzy_matches.sort_by(|a, b| b.cmp(a));
            return fuzzy_matches[0].to_string();
        }
    }

    clean_id
}

fn split_tool_call_id(value: &str) -> (&str, Option<&str>) {
    if !value.starts_with("sig:") {
        return (value, None);
    }
    let parts: Vec<&str> = value.split(':').collect();
    if parts.len() >= 3 {
        let signature = parts[1];
        let id = &value[parts[0].len() + parts[1].len() + 2..];
        (id, Some(signature))
    } else {
        (value, None)
    }
}

fn sanitize_function_name(name: &str) -> String {
    let bytes = name.as_bytes();
    if !bytes.is_empty()
        && (bytes[0].is_ascii_alphabetic() || bytes[0] == b'_')
        && bytes.iter().all(|b| b.is_ascii_alphanumeric() || *b == b'_')
    {
        return name.to_string();
    }

    let mut sanitized: String = name
        .chars()
        .map(|c| if c.is_ascii_alphanumeric() || c == '_' { c } else { '_' })
        .collect();

    if sanitized.chars().next().map(|c| c.is_ascii_digit()).unwrap_or(false) {
        sanitized = format!("fn_{}", sanitized);
    }
    if sanitized.is_empty() {
        sanitized = format!("fn_{}", Uuid::new_v4().simple());
    }

    sanitized
}

fn merge_consecutive_function_responses(contents: Vec<Value>) -> Vec<Value> {
    let mut merged: Vec<Value> = Vec::new();

    for content in contents {
        let parts = content
            .get("parts")
            .and_then(|p| p.as_array())
            .cloned()
            .unwrap_or_default();

        let contains_only_func_resps = content.get("role").and_then(|r| r.as_str()) == Some("user")
            && !parts.is_empty()
            && parts.iter().all(|p| p.get("functionResponse").is_some());

        if contains_only_func_resps {
            if let Some(prev) = merged.last_mut() {
                let prev_role_user = prev.get("role").and_then(|r| r.as_str()) == Some("user");
                let prev_parts_opt = prev.get_mut("parts").and_then(|p| p.as_array_mut());

                if prev_role_user {
                    if let Some(prev_parts) = prev_parts_opt {
                        let prev_contains_only_func_resps = !prev_parts.is_empty()
                            && prev_parts.iter().all(|p| p.get("functionResponse").is_some());

                        if prev_contains_only_func_resps {
                            prev_parts.extend(parts);
                            continue;
                        }
                    }
                }
            }
        }

        merged.push(content);
    }

    merged
}

fn split_oversized_function_call_steps(contents: Vec<Value>) -> Vec<Value> {
    let mut normalized = Vec::new();
    let mut index = 0;

    while index < contents.len() {
        let content = &contents[index];
        let next = contents.get(index + 1);

        let calls = content.get("parts").and_then(|p| p.as_array());
        let responses = next.and_then(|n| n.get("parts")).and_then(|p| p.as_array());

        let is_complete_tool_exchange = content.get("role").and_then(|r| r.as_str()) == Some("model")
            && next.and_then(|n| n.get("role")).and_then(|r| r.as_str()) == Some("user")
            && calls.map(|c| c.len()).unwrap_or(0) > MAX_PARALLEL_FUNCTION_CALLS_PER_STEP
            && calls.map(|c| c.iter().all(|p| p.get("functionCall").is_some())).unwrap_or(false)
            && responses.map(|r| r.len()).unwrap_or(0) == calls.map(|c| c.len()).unwrap_or(0)
            && responses.map(|r| r.iter().all(|p| p.get("functionResponse").is_some())).unwrap_or(false);

        if !is_complete_tool_exchange {
            normalized.push(content.clone());
            index += 1;
            continue;
        }

        let calls_vec = calls.unwrap();
        let responses_vec = responses.unwrap();

        for offset in (0..calls_vec.len()).step_by(MAX_PARALLEL_FUNCTION_CALLS_PER_STEP) {
            let end = (offset + MAX_PARALLEL_FUNCTION_CALLS_PER_STEP).min(calls_vec.len());
            let mut call_batch = calls_vec[offset..end].to_vec();
            let response_batch = responses_vec[offset..end].to_vec();

            for part in &mut call_batch {
                if part.get("thoughtSignature").is_none() {
                    part.as_object_mut().unwrap().insert(
                        "thoughtSignature".to_string(),
                        Value::String(MISSING_THOUGHT_SIGNATURE_FALLBACK.to_string()),
                    );
                }
            }

            normalized.push(serde_json::json!({
                "role": "model",
                "parts": call_batch
            }));
            normalized.push(serde_json::json!({
                "role": "user",
                "parts": response_batch
            }));
        }

        index += 2;
    }

    normalized
}

pub fn transform_to_google_body(
    openai_body: &Value,
    project_id: &str,
    is_cli: bool,
    session_id: &str,
    aggressive: bool,
) -> Value {
    let raw_model = openai_body
        .get("model")
        .and_then(|v| v.as_str())
        .unwrap_or("")
        .to_lowercase();

    let resolved_model = resolve_model_id(&raw_model);
    let mut google_model = resolved_model;

    // Support OpenAI compatibility: reasoning_effort (low/medium/high) or reasoning.effort
    // Support DeepSeek style: chat_prefix, thinking.type="enabled", etc.
    let param_tier = openai_body
        .get("reasoning_effort")
        .or_else(|| openai_body.get("reasoning").and_then(|r| r.get("effort")))
        .and_then(|v| v.as_str());

    let extracted_tier = if raw_model.ends_with("-thinking-high") || raw_model.ends_with("-high") {
        Some("high")
    } else if raw_model.ends_with("-thinking-low") || raw_model.ends_with("-low") {
        Some("low")
    } else if raw_model.ends_with("-thinking-medium") || raw_model.ends_with("-medium") {
        Some("medium")
    } else {
        param_tier
    };

    let mut base_model = google_model.clone();
    for suffix in &["-thinking-high", "-thinking-low", "-thinking-medium", "-high", "-low", "-medium", "-preview"] {
        if base_model.ends_with(suffix) {
            base_model = base_model[..base_model.len() - suffix.len()].to_string();
            break;
        }
    }

    // Auto-map gemini-3.8-flash or gemini-3.7-flash with tier to the exact Google internal model
    if is_modern_flash_base_model(&base_model) {
        let tier = extracted_tier.unwrap_or("high");
        google_model = format!("{}-{}", base_model, tier);
    }

    if google_model.contains("claude") {
        google_model = base_model.clone();
        if google_model == "claude-opus-4-6" {
            google_model = "claude-opus-4-6-thinking".to_string();
        } else if google_model == "claude-sonnet-4-6" {
            google_model = "claude-sonnet-4-6-thinking".to_string();
        } else if google_model == "claude-sonnet-4-5" {
            google_model = "claude-sonnet-4-5-thinking".to_string();
        }
    }

    let is_native = NATIVELY_SUPPORTED.contains(&google_model.as_str()) || NATIVELY_SUPPORTED.contains(&base_model.as_str());

    if is_cli {
        if !google_model.contains("claude") {
            let is_modern_flash = is_modern_flash_tier_model(&google_model);
            if is_modern_flash {
                // Keep tiered ID unchanged
            } else if google_model.contains("gemini-3") {
                google_model = base_model.clone();
                if !google_model.ends_with("-preview") {
                    google_model = format!("{}-preview", google_model);
                }
            } else if google_model.contains("gpt") {
                if google_model.contains("thinking") {
                    google_model = "gemini-2.0-flash-thinking-exp".to_string();
                } else {
                    google_model = "gemini-2.0-pro-exp".to_string();
                }
            } else {
                google_model = base_model.clone();
            }
        } else {
            google_model = base_model.clone();
            if google_model == "claude-sonnet-4-6" {
                google_model = "claude-sonnet-4-6-thinking".to_string();
            } else if google_model == "claude-sonnet-4-5" {
                google_model = "claude-sonnet-4-5-thinking".to_string();
            }
        }
    } else {
        if google_model.ends_with("-preview") {
            google_model = google_model.replace("-preview", "");
        }

        if is_native {
            if base_model.contains("gemini-3.1-pro") {
                google_model = "gemini-3.1-pro-low".to_string();
            } else if base_model.contains("gemini-3-pro") {
                google_model = format!("gemini-3-pro-{}", extracted_tier.unwrap_or("high"));
            } else if base_model.contains("gemini-3-flash") {
                google_model = "gemini-3-flash".to_string();
            } else {
                google_model = base_model.clone();
            }

            if google_model == "claude-opus-4-6" || google_model == "antigravity-claude-opus-4-6" {
                google_model = "claude-opus-4-6-thinking".to_string();
            } else if google_model == "claude-sonnet-4-6" || google_model == "antigravity-claude-sonnet-4-6" {
                google_model = "claude-sonnet-4-6-thinking".to_string();
            } else if google_model == "claude-sonnet-4-5" || google_model == "antigravity-claude-sonnet-4-5" {
                google_model = "claude-sonnet-4-5-thinking".to_string();
            }
        }
    }

    let is_thinking_model = raw_model.contains("-thinking");
    let is_image_model = google_model.contains("image") || google_model.contains("imagen");
    let is_gemini3_model = google_model.contains("gemini-3") && !is_image_model;

    // Extract system messages and history messages
    let messages = openai_body
        .get("messages")
        .and_then(|v| v.as_array())
        .cloned()
        .unwrap_or_default();

    let mut system_text = String::new();
    let mut other_messages = Vec::new();

    for m in messages {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("");
        if role == "system" {
            if let Some(content) = m.get("content").and_then(|c| c.as_str()) {
                if !system_text.is_empty() {
                    system_text.push_str("\n\n");
                }
                system_text.push_str(content);
            }
        } else {
            other_messages.push(m);
        }
    }

    let system_instruction = if !is_cli {
        let full_sys = format!(
            "{}\n\n{}\n\n{}",
            ANTIGRAVITY_SYSTEM_INSTRUCTION, system_text, RESPONSE_COMPLETION_PROTOCOL
        )
        .trim()
        .to_string();

        serde_json::json!({
            "role": "user",
            "parts": [{ "text": full_sys }]
        })
    } else if !system_text.is_empty() {
        let full_sys = format!("{}\n\n{}", system_text, RESPONSE_COMPLETION_PROTOCOL);
        serde_json::json!({
            "parts": [{ "text": full_sys }]
        })
    } else {
        Value::Null
    };

    // Pre-scan tool_calls to build tool_names lookup map
    let mut tool_names: HashMap<String, Vec<String>> = HashMap::new();
    for msg in &other_messages {
        if let Some(tc_arr) = msg.get("tool_calls").and_then(|tc| tc.as_array()) {
            for tc in tc_arr {
                if let (Some(id_str), Some(func_name)) = (
                    tc.get("id").and_then(|id| id.as_str()),
                    tc.get("function").and_then(|f| f.get("name")).and_then(|n| n.as_str()),
                ) {
                    let clean_id = split_tool_call_id(id_str).0.to_string();
                    tool_names.entry(clean_id).or_default().push(func_name.to_string());
                }
            }
        }
    }

    let mut raw_contents = Vec::new();

    for m in other_messages {
        let role = m.get("role").and_then(|r| r.as_str()).unwrap_or("user");
        let google_role = if role == "assistant" || role == "model" {
            "model"
        } else {
            "user"
        };

        let mut parts = Vec::new();

        if role == "tool" {
            let tool_call_id_raw = m.get("tool_call_id").and_then(|id| id.as_str()).unwrap_or("");
            let (clean_tool_id, _) = split_tool_call_id(tool_call_id_raw);

            let queued_name = tool_names
                .get_mut(clean_tool_id)
                .and_then(|names| if !names.is_empty() { Some(names.remove(0)) } else { None });

            let explicit_name = m.get("name").and_then(|n| n.as_str());
            let final_name = explicit_name.or(queued_name.as_deref()).unwrap_or("function_result");

            let raw_content = m.get("content").unwrap_or(&Value::Null);
            let resp_obj = match raw_content {
                Value::String(s) => serde_json::from_str::<Value>(s).unwrap_or_else(|_| serde_json::json!({ "result": s })),
                other => other.clone(),
            };

            let final_resp = if resp_obj.is_object() {
                resp_obj
            } else {
                serde_json::json!({ "result": resp_obj })
            };

            let mut func_resp = serde_json::json!({
                "name": final_name,
                "response": final_resp
            });

            if google_model.contains("claude") || google_model.contains("gemini-3") {
                func_resp
                    .as_object_mut()
                    .unwrap()
                    .insert("id".to_string(), Value::String(clean_tool_id.to_string()));
            }

            parts.push(serde_json::json!({ "functionResponse": func_resp }));
        } else {
            if let Some(content_val) = m.get("content") {
                match content_val {
                    Value::String(text) => {
                        if !text.is_empty() {
                            parts.push(serde_json::json!({ "text": text }));
                        }
                    }
                    Value::Array(arr) => {
                        for item in arr {
                            if let Some(t) = item.get("text").and_then(|t| t.as_str()) {
                                parts.push(serde_json::json!({ "text": t }));
                            } else if let Some(img_url) = item.get("image_url").and_then(|u| u.get("url")).and_then(|u| u.as_str()) {
                                if img_url.starts_with("data:") {
                                    if let Some(comma_pos) = img_url.find(',') {
                                        let meta = &img_url[..comma_pos];
                                        let raw_data = &img_url[comma_pos + 1..];
                                        let clean_data: String = raw_data.chars().filter(|c| !c.is_whitespace()).collect();
                                        let mime = meta.split(';').next().unwrap_or("image/jpeg").replace("data:", "");
                                        parts.push(serde_json::json!({
                                            "inlineData": {
                                                "mimeType": mime,
                                                "data": clean_data
                                            }
                                        }));
                                    }
                                }
                            }
                        }
                    }
                    _ => {}
                }
            }

            if let Some(tool_calls) = m.get("tool_calls").and_then(|tc| tc.as_array()) {
                let mut function_call_parts: Vec<Value> = Vec::new();
                for tc in tool_calls {
                    if let Some(func) = tc.get("function") {
                        let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
                        let args_val = func.get("arguments");
                        let parsed_args = match args_val {
                            Some(Value::String(s)) => serde_json::from_str::<Value>(s).unwrap_or(serde_json::json!({})),
                            Some(v) => v.clone(),
                            None => serde_json::json!({}),
                        };

                        let mut func_call = serde_json::json!({
                            "name": name,
                            "args": parsed_args
                        });

                        let mut extracted_sig = None;
                        if let Some(raw_id) = tc.get("id").and_then(|id| id.as_str()) {
                            let (clean_id, sig_opt) = split_tool_call_id(raw_id);
                            if google_model.contains("claude") || google_model.contains("gemini-3") {
                                func_call
                                    .as_object_mut()
                                    .unwrap()
                                    .insert("id".to_string(), Value::String(clean_id.to_string()));
                            }
                            extracted_sig = sig_opt.map(|s| s.to_string());
                        }

                        let mut func_part = serde_json::json!({
                            "functionCall": func_call
                        });

                        if let Some(sig) = extracted_sig {
                            func_part.as_object_mut().unwrap().insert("thoughtSignature".to_string(), Value::String(sig));
                        }

                        function_call_parts.push(func_part);
                    }
                }

                if (google_model.contains("gemini-3") || google_model.contains("gemini-2")) && !function_call_parts.is_empty() {
                    for part in &mut function_call_parts {
                        if part.get("thoughtSignature").is_none() {
                            part.as_object_mut().unwrap().insert(
                                "thoughtSignature".to_string(),
                                Value::String(MISSING_THOUGHT_SIGNATURE_FALLBACK.to_string()),
                            );
                        }
                    }
                }

                parts.extend(function_call_parts);
            }

            if let Some(func) = m.get("function_call") {
                let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let args_val = func.get("arguments");
                let parsed_args = match args_val {
                    Some(Value::String(s)) => serde_json::from_str::<Value>(s).unwrap_or(serde_json::json!({})),
                    Some(v) => v.clone(),
                    None => serde_json::json!({}),
                };

                let mut func_call = serde_json::json!({
                    "name": name,
                    "args": parsed_args
                });

                if google_model.contains("claude") || google_model.contains("gemini-3") {
                    if let Some(raw_id) = m.get("id").and_then(|id| id.as_str()) {
                        let (clean_id, _) = split_tool_call_id(raw_id);
                        func_call
                            .as_object_mut()
                            .unwrap()
                            .insert("id".to_string(), Value::String(clean_id.to_string()));
                    }
                }

                let mut func_part = serde_json::json!({
                    "functionCall": func_call
                });

                if google_model.contains("gemini-3") || google_model.contains("gemini-2") {
                    func_part.as_object_mut().unwrap().insert(
                        "thoughtSignature".to_string(),
                        Value::String(MISSING_THOUGHT_SIGNATURE_FALLBACK.to_string()),
                    );
                }

                parts.push(func_part);
            }
        }

        if parts.is_empty() {
            parts.push(serde_json::json!({ "text": " " }));
        }

        raw_contents.push(serde_json::json!({
            "role": google_role,
            "parts": parts
        }));
    }

    let mut contents = merge_consecutive_function_responses(raw_contents);

    if google_model.contains("gemini-3") {
        contents = split_oversized_function_call_steps(contents);
    }

    if google_model.contains("gemini-3") || google_model.contains("gemini-2") {
        for content in &mut contents {
            if let Some(parts) = content.get_mut("parts").and_then(|p| p.as_array_mut()) {
                for part in parts {
                    if part.get("functionCall").is_some() && part.get("thoughtSignature").is_none() {
                        part.as_object_mut().unwrap().insert(
                            "thoughtSignature".to_string(),
                            Value::String(MISSING_THOUGHT_SIGNATURE_FALLBACK.to_string()),
                        );
                    }
                }
            }
        }
    }

    let mut generation_config = Map::new();

    let requested_max_tokens = openai_body.get("max_tokens").and_then(|v| v.as_i64());
    let has_explicit_budget = openai_body.get("thinking_budget").is_some()
        || openai_body.get("thinking").and_then(|t| t.get("budget_tokens")).is_some();

    let max_output_tokens = if is_gemini3_model {
        requested_max_tokens.unwrap_or(65535).min(65535)
    } else if is_thinking_model || has_explicit_budget || google_model.contains("claude") || google_model.contains("gemini-2") {
        requested_max_tokens.unwrap_or(64000).max(64000).min(65535)
    } else {
        requested_max_tokens.unwrap_or(4096)
    };

    generation_config.insert("maxOutputTokens".to_string(), Value::Number(max_output_tokens.into()));

    if let Some(stop) = openai_body.get("stop") {
        if stop.is_array() {
            generation_config.insert("stopSequences".to_string(), stop.clone());
        } else if let Some(s) = stop.as_str() {
            generation_config.insert("stopSequences".to_string(), serde_json::json!([s]));
        }
    }

    if !is_gemini3_model {
        let temp = openai_body.get("temperature").and_then(|t| t.as_f64()).unwrap_or(0.7);
        let top_p = openai_body.get("top_p").and_then(|p| p.as_f64()).unwrap_or(0.95);
        generation_config.insert("temperature".to_string(), serde_json::json!(temp));
        generation_config.insert("topP".to_string(), serde_json::json!(top_p));
        generation_config.insert("candidateCount".to_string(), Value::Number(1.into()));
    }

    let safety_threshold = std::env::var("SAFETY_THRESHOLD").unwrap_or_else(|_| "BLOCK_NONE".to_string());
    let safety_settings = serde_json::json!([
        { "category": "HARM_CATEGORY_HARASSMENT", "threshold": safety_threshold },
        { "category": "HARM_CATEGORY_HATE_SPEECH", "threshold": safety_threshold },
        { "category": "HARM_CATEGORY_SEXUALLY_EXPLICIT", "threshold": safety_threshold },
        { "category": "HARM_CATEGORY_DANGEROUS_CONTENT", "threshold": safety_threshold }
    ]);

    let actual_session_id = if session_id.is_empty() {
        Uuid::new_v4().to_string()
    } else {
        session_id.to_string()
    };

    let mut google_request = serde_json::json!({
        "contents": contents,
        "generationConfig": generation_config,
        "safetySettings": safety_settings,
        "sessionId": actual_session_id
    });

    if is_gemini3_model {
        let level = extracted_tier
            .or_else(|| openai_body.get("reasoning_effort").and_then(|r| r.as_str()))
            .unwrap_or("medium");

        google_request["generationConfig"]
            .as_object_mut()
            .unwrap()
            .insert(
                "thinkingConfig".to_string(),
                serde_json::json!({
                    "includeThoughts": true,
                    "thinkingLevel": level
                }),
            );
    } else if is_thinking_model {
        let budget = openai_body
            .get("thinking_budget")
            .and_then(|b| b.as_i64())
            .or_else(|| openai_body.get("thinking").and_then(|t| t.get("budget_tokens")).and_then(|b| b.as_i64()))
            .unwrap_or(16000);

        google_request["generationConfig"]
            .as_object_mut()
            .unwrap()
            .insert(
                "thinkingConfig".to_string(),
                serde_json::json!({
                    "includeThoughts": true,
                    "thinkingBudget": budget
                }),
            );
    }

    if !system_instruction.is_null() {
        google_request
            .as_object_mut()
            .unwrap()
            .insert("systemInstruction".to_string(), system_instruction);
    }

    if let Some(tools_arr) = openai_body.get("tools").and_then(|t| t.as_array()) {
        let mut declarations = Vec::new();
        for t in tools_arr {
            if let Some(func) = t.get("function") {
                let name = func.get("name").and_then(|n| n.as_str()).unwrap_or("");
                let sanitized_name = sanitize_function_name(name);
                let mut desc = func.get("description").and_then(|d| d.as_str()).unwrap_or("").to_string();
                let default_params = serde_json::json!({ "type": "object", "properties": {} });
                let params = func.get("parameters").unwrap_or(&default_params);
                let clean_params = clean_json_schema_for_antigravity(params, aggressive);

                if let Some(props) = clean_params.get("properties").and_then(|p| p.as_object()) {
                    let param_names: Vec<&str> = props.keys().filter(|k| *k != "_placeholder").map(|s| s.as_str()).collect();
                    if !param_names.is_empty() {
                        desc.push_str(&format!(" [Parameters: {}]", param_names.join(", ")));
                    }
                }

                declarations.push(serde_json::json!({
                    "name": sanitized_name,
                    "description": desc,
                    "parameters": clean_params
                }));
            }
        }

        if !declarations.is_empty() {
            google_request.as_object_mut().unwrap().insert(
                "tools".to_string(),
                serde_json::json!([{ "functionDeclarations": declarations }]),
            );

            if google_model.contains("claude") {
                google_request.as_object_mut().unwrap().insert(
                    "toolConfig".to_string(),
                    serde_json::json!({
                        "functionCallingConfig": { "mode": "VALIDATED" }
                    }),
                );
            }
        }
    }

    serde_json::json!({
        "project": project_id,
        "model": google_model,
        "userAgent": "antigravity",
        "requestId": format!("agent-{}", Uuid::new_v4()),
        "requestType": "agent",
        "request": google_request
    })
}
