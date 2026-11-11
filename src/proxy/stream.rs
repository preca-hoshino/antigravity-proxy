use async_stream::stream;
use bytes::Bytes;
use futures_util::Stream;
use serde_json::Value;
use std::collections::HashSet;
use std::sync::Arc;
use std::time::Instant;
use crate::logger::LogBroadcaster;

pub fn transform_google_sse_to_openai_stream(
    mut upstream_stream: impl Stream<Item = Result<Bytes, reqwest::Error>> + Unpin + Send + 'static,
    model: String,
    request_id: String,
    include_usage: bool,
    start_time: Instant,
    broadcaster: Arc<LogBroadcaster>,
) -> impl Stream<Item = Result<String, std::io::Error>> + Send + 'static {
    stream! {
        let created = chrono::Utc::now().timestamp();
        let mut buffer = String::new();
        let mut emitted_role = false;
        let mut current_tool_calls_active = false;
        let mut seen_tool_calls: HashSet<String> = HashSet::new();
        let mut last_usage = None;
        let mut first_token_time: Option<Instant> = None;
        let mut raw_usage_obj: Option<Value> = None;

        while let Some(chunk_res) = futures_util::StreamExt::next(&mut upstream_stream).await {
            let chunk = match chunk_res {
                Ok(c) => c,
                Err(_) => break,
            };

            let text = String::from_utf8_lossy(&chunk);
            buffer.push_str(&text);

            while let Some(pos) = buffer.find('\n') {
                let line = buffer[..pos].trim().to_string();
                buffer = buffer[pos + 1..].to_string();

                if line.is_empty() {
                    continue;
                }

                if let Some(data_str) = line.strip_prefix("data: ") {
                    if data_str == "[DONE]" {
                        break;
                    }

                    if let Ok(json) = serde_json::from_str::<Value>(data_str) {
                        let response_obj = json.get("response").unwrap_or(&json);

                        if let Some(usage_meta) = response_obj.get("usageMetadata") {
                            raw_usage_obj = Some(usage_meta.clone());
                            let prompt = usage_meta.get("promptTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
                            let completion = usage_meta.get("candidatesTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
                            let total = usage_meta.get("totalTokenCount").and_then(|v| v.as_i64()).unwrap_or(prompt + completion);
                            let cached = usage_meta.get("cachedContentTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
                            let thoughts = usage_meta.get("thoughtsTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);

                            let generated = if thoughts > 0 {
                                completion + thoughts
                            } else if total > prompt && completion == 0 {
                                total - prompt
                            } else {
                                completion
                            };

                            last_usage = Some(serde_json::json!({
                                "prompt_tokens": prompt,
                                "completion_tokens": generated,
                                "total_tokens": total,
                                "prompt_tokens_details": {
                                    "cached_tokens": cached
                                },
                                "completion_tokens_details": {
                                    "reasoning_tokens": thoughts
                                }
                            }));
                        }

                        if let Some(candidates) = response_obj.get("candidates").and_then(|c| c.as_array()) {
                            if let Some(first_cand) = candidates.first() {
                                let parts = first_cand.get("content").and_then(|c| c.get("parts")).and_then(|p| p.as_array()).cloned().unwrap_or_default();
                                let finish_reason = first_cand.get("finishReason").and_then(|f| f.as_str());

                                if !emitted_role {
                                    emitted_role = true;
                                    let init_chunk = serde_json::json!({
                                        "id": request_id,
                                        "object": "chat.completion.chunk",
                                        "created": created,
                                        "model": model,
                                        "choices": [{
                                            "index": 0,
                                            "delta": { "role": "assistant", "content": "" },
                                            "finish_reason": null
                                        }]
                                    });
                                    yield Ok(format!("data: {}\n\n", init_chunk));
                                }

                                for part in parts {
                                    let mut delta = serde_json::Map::new();

                                    if let Some(text_content) = part.get("text").and_then(|t| t.as_str()) {
                                        if first_token_time.is_none() && !text_content.is_empty() {
                                            first_token_time = Some(Instant::now());
                                        }
                                        let is_thought = part.get("thought").and_then(|t| t.as_bool()).unwrap_or(false);
                                        if is_thought {
                                            delta.insert("reasoning_content".to_string(), Value::String(text_content.to_string()));
                                        } else {
                                            delta.insert("content".to_string(), Value::String(text_content.to_string()));
                                        }
                                    }

                                    if let Some(func_call) = part.get("functionCall") {
                                        if first_token_time.is_none() {
                                            first_token_time = Some(Instant::now());
                                        }
                                        current_tool_calls_active = true;
                                        let name = func_call.get("name").and_then(|n| n.as_str()).unwrap_or("");
                                        let call_id = func_call.get("id").and_then(|id| id.as_str()).unwrap_or("call_generated");
                                        let dedupe_key = format!("{}:{}", call_id, name);

                                        if !seen_tool_calls.contains(&dedupe_key) {
                                            seen_tool_calls.insert(dedupe_key);
                                            let args = func_call.get("args").map(|a| {
                                                if a.is_string() { a.as_str().unwrap().to_string() } else { a.to_string() }
                                            }).unwrap_or_else(|| "{}".to_string());

                                            let tool_call_obj = serde_json::json!([{
                                                "index": 0,
                                                "id": call_id,
                                                "type": "function",
                                                "function": {
                                                    "name": name,
                                                    "arguments": args
                                                }
                                            }]);
                                            delta.insert("tool_calls".to_string(), tool_call_obj);
                                        }
                                    }

                                    if !delta.is_empty() {
                                        let chunk = serde_json::json!({
                                            "id": request_id,
                                            "object": "chat.completion.chunk",
                                            "created": created,
                                            "model": model,
                                            "choices": [{
                                                "index": 0,
                                                "delta": delta,
                                                "finish_reason": null
                                            }]
                                        });
                                        yield Ok(format!("data: {}\n\n", chunk));
                                    }
                                }

                                if let Some(fr) = finish_reason {
                                    let openai_finish_reason = if current_tool_calls_active {
                                        "tool_calls"
                                    } else {
                                        match fr {
                                            "STOP" => "stop",
                                            "MAX_TOKENS" => "length",
                                            "SAFETY" => "content_filter",
                                            _ => "stop",
                                        }
                                    };

                                    let final_choice_chunk = serde_json::json!({
                                        "id": request_id,
                                        "object": "chat.completion.chunk",
                                        "created": created,
                                        "model": model,
                                        "choices": [{
                                            "index": 0,
                                            "delta": {},
                                            "finish_reason": openai_finish_reason
                                        }]
                                    });
                                    yield Ok(format!("data: {}\n\n", final_choice_chunk));
                                }
                            }
                        }
                    }
                }
            }
        }

        // Calculate and log metrics
        let total_duration = start_time.elapsed();
        let (ttft_str, decode_duration) = if let Some(ft) = first_token_time {
            let ttft_dur = ft.duration_since(start_time);
            let decode_dur = total_duration.saturating_sub(ttft_dur);
            (format!("{:.0}ms", ttft_dur.as_millis()), decode_dur)
        } else {
            ("-".to_string(), total_duration)
        };

        let mut prompt_tok = 0i64;
        let mut comp_tok = 0i64;
        let mut total_tok = 0i64;
        let mut cached_tok = 0i64;
        let mut thoughts_tok = 0i64;

        if let Some(ref meta) = raw_usage_obj {
            prompt_tok = meta.get("promptTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
            comp_tok = meta.get("candidatesTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
            total_tok = meta.get("totalTokenCount").and_then(|v| v.as_i64()).unwrap_or(prompt_tok + comp_tok);
            cached_tok = meta.get("cachedContentTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
            thoughts_tok = meta.get("thoughtsTokenCount").and_then(|v| v.as_i64()).unwrap_or(0);
        }

        // Google 在 thinking 开启时，部分请求 candidatesTokenCount 仅为正文 token（不含思考），或者模型完全在输出思考/工具调用
        // 总生成的 output tokens 实质上为 candidatesTokenCount + thoughtsTokenCount，或者 max(comp_tok, total - prompt)
        let generated_tok = if thoughts_tok > 0 {
            comp_tok + thoughts_tok
        } else if total_tok > prompt_tok && comp_tok == 0 {
            total_tok - prompt_tok
        } else {
            comp_tok
        };

        let total_secs = total_duration.as_secs_f64();
        let decode_secs = decode_duration.as_secs_f64();

        // 优先使用实际生成耗时计算纯生成速率 Decode Speed（排除前置网络握手等待时间）
        // 如果 output token 统计正常，按实际生成计算；若 decode_secs 极短则回退总时间避免虚高
        let effective_secs = if decode_secs >= 0.2 {
            decode_secs
        } else {
            total_secs
        };

        let tps_str = if generated_tok > 0 && effective_secs > 0.05 {
            format!("{:.1} tok/s", (generated_tok as f64) / effective_secs)
        } else {
            "-".to_string()
        };

        let cache_rate_str = if prompt_tok > 0 {
            format!("{:.1}%", (cached_tok as f64 / prompt_tok as f64) * 100.0)
        } else {
            "-".to_string()
        };

        let metric_log = format!(
            "[METRICS] Model: {} | Total Tokens: {} (Prompt: {}, Output: {} [Visible: {}, Thinking: {}], Cache: {}) | Duration: {:.2}s | TTFT: {} | Speed: {} | Cache Hit: {}",
            model, total_tok, prompt_tok, generated_tok, comp_tok, thoughts_tok, cached_tok, total_secs, ttft_str, tps_str, cache_rate_str
        );
        tracing::info!(target: "metrics", "{}", metric_log);
        broadcaster.broadcast("info", &metric_log);

        if include_usage {
            if let Some(usage) = last_usage {
                let usage_chunk = serde_json::json!({
                    "id": request_id,
                    "object": "chat.completion.chunk",
                    "created": created,
                    "model": model,
                    "choices": [],
                    "usage": usage
                });
                yield Ok(format!("data: {}\n\n", usage_chunk));
            }
        }

        yield Ok("data: [DONE]\n\n".to_string());
    }
}
