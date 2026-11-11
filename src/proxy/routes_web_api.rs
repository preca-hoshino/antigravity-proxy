use super::routes_openai::{AppState, SUPPORTED_MODELS};
use crate::upstream::headers::generate_fingerprint;
use crate::upstream::oauth::get_project_id;
use axum::{
    extract::{Path, State},
    http::StatusCode,
    response::{
        sse::{Event, KeepAlive, Sse},
        IntoResponse, Response,
    },
    Json,
};
use futures_util::stream::Stream;
use serde_json::Value;
use std::collections::HashMap;
use std::convert::Infallible;
use std::sync::Arc;
use std::time::Duration;
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;

pub async fn get_status(State(state): State<Arc<AppState>>) -> Response {
    let accounts = state.account_manager.get_accounts().await;

    let mut cooldowns: HashMap<String, i64> = HashMap::new();
    for acc in &accounts {
        for (k, v) in &acc.cooldowns {
            cooldowns.insert(k.clone(), *v);
        }
    }

    let res = serde_json::json!({
        "accounts": accounts,
        "strategy": state.config.rotation.strategy,
        "cooldowns": cooldowns,
        "supportedModels": SUPPORTED_MODELS
    });

    Json(res).into_response()
}

pub async fn get_config(State(state): State<Arc<AppState>>) -> Response {
    Json(&*state.config).into_response()
}

pub async fn update_config(State(_state): State<Arc<AppState>>, Json(_body): Json<Value>) -> Response {
    Json(serde_json::json!({ "status": "ok" })).into_response()
}

pub async fn set_strategy(State(_state): State<Arc<AppState>>, Json(body): Json<Value>) -> Response {
    let strategy = body.get("strategy").and_then(|s| s.as_str()).unwrap_or("hybrid");
    tracing::info!(strategy = %strategy, "Updated rotation strategy");
    Json(serde_json::json!({ "status": "ok", "strategy": strategy })).into_response()
}

pub async fn reset_all_cooldowns(State(state): State<Arc<AppState>>) -> Response {
    state.account_manager.reset_all_cooldowns().await;
    let accounts = state.account_manager.get_accounts().await;
    state.broadcaster.emit_event(crate::logger::ServerSseEvent::Update(serde_json::json!({
        "accounts": accounts,
        "cooldowns": {},
        "strategy": state.config.rotation.strategy
    })));
    Json(serde_json::json!({ "status": "ok" })).into_response()
}

pub async fn delete_account(State(state): State<Arc<AppState>>, Path(email): Path<String>) -> Response {
    match state.account_manager.remove_account(&email).await {
        Ok(true) => {
            let accounts = state.account_manager.get_accounts().await;
            state.broadcaster.emit_event(crate::logger::ServerSseEvent::Update(serde_json::json!({
                "accounts": accounts,
                "strategy": state.config.rotation.strategy
            })));
            Json(serde_json::json!({ "status": "ok" })).into_response()
        }
        _ => (StatusCode::NOT_FOUND, Json(serde_json::json!({ "error": "Account not found" }))).into_response(),
    }
}

pub async fn reset_account_cooldown(State(state): State<Arc<AppState>>, Path(email): Path<String>) -> Response {
    state.account_manager.reset_cooldown(&email).await;
    let accounts = state.account_manager.get_accounts().await;
    let mut cooldowns: HashMap<String, i64> = HashMap::new();
    for acc in &accounts {
        for (k, v) in &acc.cooldowns {
            cooldowns.insert(k.clone(), *v);
        }
    }
    state.broadcaster.emit_event(crate::logger::ServerSseEvent::Cooldown(serde_json::json!({
        "cooldowns": cooldowns
    })));
    state.broadcaster.emit_event(crate::logger::ServerSseEvent::Update(serde_json::json!({
        "accounts": accounts,
        "strategy": state.config.rotation.strategy,
        "cooldowns": cooldowns
    })));
    Json(serde_json::json!({ "status": "ok" })).into_response()
}

pub async fn update_account_project(
    State(state): State<Arc<AppState>>,
    Path(email): Path<String>,
    Json(body): Json<Value>,
) -> Response {
    let project_id = body.get("projectId").and_then(|p| p.as_str()).unwrap_or("");
    let _ = state.account_manager.update_project_id(&email, project_id).await;
    let accounts = state.account_manager.get_accounts().await;
    state.broadcaster.emit_event(crate::logger::ServerSseEvent::Update(serde_json::json!({
        "accounts": accounts,
        "strategy": state.config.rotation.strategy
    })));
    Json(serde_json::json!({ "status": "ok" })).into_response()
}

pub async fn rediscover_account_project(State(state): State<Arc<AppState>>, Path(email): Path<String>) -> Response {
    let accounts = state.account_manager.get_accounts().await;
    if let Some(acc) = accounts.iter().find(|a| a.email == email) {
        if let Some(token) = &acc.access_token {
            let fp = acc.fingerprint.clone().unwrap_or_else(|| generate_fingerprint(Some(&email)));
            if let Ok(new_project) = get_project_id(&state.client, token, &fp).await {
                if !new_project.is_empty() {
                    let _ = state.account_manager.update_project_id(&email, &new_project).await;
                    let accounts = state.account_manager.get_accounts().await;
                    state.broadcaster.emit_event(crate::logger::ServerSseEvent::Update(serde_json::json!({
                        "accounts": accounts,
                        "strategy": state.config.rotation.strategy
                    })));
                    return Json(serde_json::json!({ "status": "ok", "projectId": new_project })).into_response();
                }
            }
        }
    }

    (StatusCode::BAD_REQUEST, Json(serde_json::json!({ "error": "Rediscover failed" }))).into_response()
}

pub async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl Stream<Item = Result<Event, Infallible>>> {
    let accounts = state.account_manager.get_accounts().await;
    let mut cooldowns: HashMap<String, i64> = HashMap::new();
    for acc in &accounts {
        for (k, v) in &acc.cooldowns {
            cooldowns.insert(k.clone(), *v);
        }
    }

    let init_payload = serde_json::json!({
        "version": env!("CARGO_PKG_VERSION"),
        "accounts": accounts,
        "strategy": state.config.rotation.strategy,
        "supportedModels": SUPPORTED_MODELS,
        "cooldowns": cooldowns,
        "logs": []
    });

    let init_event = Event::default().event("init").data(serde_json::to_string(&init_payload).unwrap_or_default());

    let rx = state.broadcaster.sender.subscribe();
    let stream = async_stream::stream! {
        yield Ok(init_event);

        let mut bstream = BroadcastStream::new(rx);
        while let Some(msg) = bstream.next().await {
            if let Ok(item) = msg {
                match item {
                    crate::logger::ServerSseEvent::Log(log_item) => {
                        let data = serde_json::to_string(&serde_json::json!({ "message": log_item.message })).unwrap_or_default();
                        yield Ok(Event::default().event("log").data(data));
                    }
                    crate::logger::ServerSseEvent::Init(init_data) => {
                        let data = serde_json::to_string(&init_data).unwrap_or_default();
                        yield Ok(Event::default().event("init").data(data));
                    }
                    crate::logger::ServerSseEvent::Update(update_data) => {
                        let mut data_val = update_data;
                        if let Some(obj) = data_val.as_object_mut() {
                            obj.insert("supportedModels".to_string(), serde_json::json!(SUPPORTED_MODELS));
                        }
                        let data = serde_json::to_string(&data_val).unwrap_or_default();
                        yield Ok(Event::default().event("update").data(data));
                    }
                    crate::logger::ServerSseEvent::Cooldown(cd_data) => {
                        let data = serde_json::to_string(&cd_data).unwrap_or_default();
                        yield Ok(Event::default().event("cooldown").data(data));
                    }
                    crate::logger::ServerSseEvent::Flash(flash_data) => {
                        let data = serde_json::to_string(&flash_data).unwrap_or_default();
                        yield Ok(Event::default().event("flash").data(data));
                    }
                }
            }
        }
    };

    Sse::new(stream).keep_alive(KeepAlive::new().interval(Duration::from_secs(15)))
}
