use super::routes_gemini::{gemini_native_handler, list_gemini_models};
use super::routes_openai::{chat_completions, list_models, AppState};
use super::routes_web_api::*;
use super::web_assets::static_handler;
use crate::account::manager::AccountManager;
use crate::config::AppConfig;
use crate::logger::LogBroadcaster;
use crate::upstream::quota::{print_models_summary_table, FetchAvailableModelsResult};
use axum::{
    routing::{delete, get, post},
    Router,
};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;

fn print_startup_models_table(results: &[(String, FetchAvailableModelsResult)]) {
    if results.is_empty() {
        tracing::warn!("No upstream model catalog fetched on startup (no valid/refreshed accounts found)");
        return;
    }

    for (email, res) in results {
        print_models_summary_table(email, res);
    }
}

fn log_periodic_quotas(
    results: &[(String, FetchAvailableModelsResult)],
    accounts: &[crate::account::storage::AntigravityAccount],
    broadcaster: &Arc<LogBroadcaster>,
) {
    for acc in accounts {
        let mut summary_parts = Vec::new();

        if let Some((_, res)) = results.iter().find(|(e, _)| e == &acc.email) {
            for g in &res.quota_groups {
                let short_name = if g.display_name.to_lowercase().contains("argon") {
                    "Argon"
                } else if g.display_name.to_lowercase().contains("claude") || g.display_name.to_lowercase().contains("gpt") {
                    "Claude/GPT"
                } else if g.display_name.to_lowercase().contains("gemini") {
                    "Gemini"
                } else {
                    g.display_name.as_str()
                };

                let mut win_parts = Vec::new();
                if let Some(ref b) = g.five_hour {
                    if let Some(f) = b.remaining_fraction {
                        win_parts.push(format!("5h: {:.1}%", f * 100.0));
                    }
                }
                if let Some(ref b) = g.weekly {
                    if let Some(f) = b.remaining_fraction {
                        win_parts.push(format!("Wk: {:.1}%", f * 100.0));
                    }
                }
                if !win_parts.is_empty() {
                    summary_parts.push(format!("{} ({})", short_name, win_parts.join(", ")));
                }
            }
        }

        if summary_parts.is_empty() {
            if let Some(ref quotas) = acc.quota {
                let find_pct = |keywords: &[&str]| -> Option<f64> {
                    quotas
                        .iter()
                        .find(|q| {
                            let gn = q.group_name.to_lowercase();
                            let mid = q.model_id.to_lowercase();
                            keywords.iter().any(|k| gn.contains(k) || mid.contains(k))
                        })
                        .map(|q| q.get_percentage())
                };

                if let Some(p) = find_pct(&["argon"]) {
                    summary_parts.push(format!("Argon: {:.1}%", p));
                }
                if let Some(p) = find_pct(&["hydrogen"]) {
                    summary_parts.push(format!("Hydrogen: {:.1}%", p));
                }
                if let Some(p) = find_pct(&["3.8-flash", "3.8 flash"]) {
                    summary_parts.push(format!("3.8-Flash: {:.1}%", p));
                }
                if let Some(p) = find_pct(&["opus-5-5", "opus 5.5", "sonnet-5-5", "sonnet 5.5", "claude"]) {
                    summary_parts.push(format!("Claude: {:.1}%", p));
                }

                if summary_parts.is_empty() {
                    for q in quotas.iter().take(4) {
                        summary_parts.push(format!("{}: {:.1}%", q.group_name, q.get_percentage()));
                    }
                }
            }
        }

        let summary = if summary_parts.is_empty() {
            "No quota data".to_string()
        } else {
            summary_parts.join(" | ")
        };

        let log_msg = format!("[QUOTA] Account: {} | Health: {:.0} | {}", acc.email, acc.health_score, summary);
        tracing::info!(target: "quota", "{}", log_msg);
        broadcaster.broadcast("info", &log_msg);
    }
}

pub fn build_router(
    account_manager: Arc<AccountManager>,
    config: Arc<AppConfig>,
    client: Client,
    broadcaster: Arc<LogBroadcaster>,
) -> Router {
    let state = Arc::new(AppState {
        account_manager: account_manager.clone(),
        config: config.clone(),
        client,
        broadcaster,
    });

    // Background quota refresh loop
    let am_clone = account_manager.clone();
    let broadcaster_clone = state.broadcaster.clone();
    let config_strategy = config.rotation.strategy.clone();
    let refresh_interval = Duration::from_millis(config.quota.refresh_interval_ms);
    tokio::spawn(async move {
        // Initial startup fetch & human-readable model catalog print
        let initial_results = am_clone.refresh_and_fetch_models().await;
        print_startup_models_table(&initial_results);

        let accounts = am_clone.get_accounts().await;
        log_periodic_quotas(&initial_results, &accounts, &broadcaster_clone);
        broadcaster_clone.emit_event(crate::logger::ServerSseEvent::Update(serde_json::json!({
            "accounts": accounts,
            "strategy": config_strategy
        })));

        let mut interval = tokio::time::interval(refresh_interval);
        // Consume the immediate first tick so we don't duplicate startup print immediately
        interval.tick().await;
        loop {
            interval.tick().await;
            let results = am_clone.refresh_and_fetch_models().await;
            let accounts = am_clone.get_accounts().await;
            log_periodic_quotas(&results, &accounts, &broadcaster_clone);
            broadcaster_clone.emit_event(crate::logger::ServerSseEvent::Update(serde_json::json!({
                "accounts": accounts,
                "strategy": config_strategy
            })));
        }
    });

    Router::new()
        // OpenAI Compatible endpoints
        .route("/v1/models", get(list_models))
        .route("/v1/chat/completions", post(chat_completions))
        // Gemini Native endpoints
        .route("/v1beta/models", get(list_gemini_models))
        .route("/v1beta/models/{target}", post(gemini_native_handler))
        .route("/v1/models/{target}", post(gemini_native_handler))
        // Web Console API endpoints
        .route("/api/status", get(get_status))
        .route("/api/config", get(get_config).post(update_config))
        .route("/api/strategy", post(set_strategy))
        .route("/api/accounts/reset-all", post(reset_all_cooldowns))
        .route("/api/accounts/{email}", delete(delete_account))
        .route("/api/accounts/{email}/reset", post(reset_account_cooldown))
        .route("/api/accounts/{email}/project", post(update_account_project))
        .route("/api/accounts/{email}/project/rediscover", post(rediscover_account_project))
        .route("/api/sse", get(sse_handler))
        // Embedded Frontend Static Assets fallback
        .fallback(static_handler)
        .layer(CorsLayer::permissive())
        .with_state(state)
}
