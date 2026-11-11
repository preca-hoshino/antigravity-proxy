use serde::Serialize;
use std::sync::Arc;
use tokio::sync::broadcast;
use tracing_subscriber::{layer::SubscriberExt, util::SubscriberInitExt, EnvFilter};

#[derive(Debug, Clone, Serialize)]
pub struct LogItem {
    pub id: u64,
    pub timestamp: String,
    pub level: String,
    pub message: String,
    pub raw: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "type", content = "payload")]
pub enum ServerSseEvent {
    Log(LogItem),
    Init(serde_json::Value),
    Update(serde_json::Value),
    Cooldown(serde_json::Value),
    Flash(serde_json::Value),
}

pub struct LogBroadcaster {
    pub sender: broadcast::Sender<ServerSseEvent>,
}

impl LogBroadcaster {
    pub fn new() -> Self {
        let (sender, _) = broadcast::channel(500);
        Self { sender }
    }

    pub fn broadcast(&self, level: &str, message: &str) {
        static SEQ: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);
        let id = SEQ.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let time_str = chrono::Local::now().format("%H:%M:%S").to_string();
        let raw = format!("[{}] [{}] {}", time_str, level.to_uppercase(), message);

        let item = LogItem {
            id,
            timestamp: time_str,
            level: level.to_string(),
            message: message.to_string(),
            raw,
        };

        let _ = self.sender.send(ServerSseEvent::Log(item));
    }

    pub fn emit_event(&self, event: ServerSseEvent) {
        let _ = self.sender.send(event);
    }
}

pub fn init_logger() -> Arc<LogBroadcaster> {
    let broadcaster = Arc::new(LogBroadcaster::new());

    let env_filter = EnvFilter::try_from_default_env()
        .unwrap_or_else(|_| EnvFilter::new("info,antigravity_proxy=debug,tower_http=info"));

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_level(true)
        .with_ansi(true)
        .compact();

    tracing_subscriber::registry()
        .with(env_filter)
        .with(fmt_layer)
        .init();

    broadcaster
}
