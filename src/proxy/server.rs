use super::routes_gemini::{gemini_native_handler, list_gemini_models};
use super::routes_openai::{chat_completions, list_models, AppState};
use super::routes_web_api::*;
use super::web_assets::static_handler;
use crate::account::manager::AccountManager;
use crate::config::AppConfig;
use crate::logger::LogBroadcaster;
use axum::{
    routing::{delete, get, post},
    Router,
};
use reqwest::Client;
use std::sync::Arc;
use std::time::Duration;
use tower_http::cors::CorsLayer;

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
        // Initial refresh
        am_clone.refresh_all_quotas().await;
        let accounts = am_clone.get_accounts().await;
        broadcaster_clone.emit_event(crate::logger::ServerSseEvent::Update(serde_json::json!({
            "accounts": accounts,
            "strategy": config_strategy
        })));

        let mut interval = tokio::time::interval(refresh_interval);
        loop {
            interval.tick().await;
            am_clone.refresh_all_quotas().await;
            let accounts = am_clone.get_accounts().await;
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
