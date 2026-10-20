use tokio::sync::broadcast;

/// Web 端事件总线：替代 Tauri 的 `Emitter`/`listen`。
#[derive(Clone)]
pub struct EventBus {
    tx: broadcast::Sender<String>,
}

impl EventBus {
    pub fn new(capacity: usize) -> Self {
        let (tx, _) = broadcast::channel(capacity);
        Self { tx }
    }

    pub fn publish<S: serde::Serialize>(&self, event: &str, payload: S) {
        let envelope = serde_json::json!({ "event": event, "payload": payload });
        if let Ok(text) = serde_json::to_string(&envelope) {
            let _ = self.tx.send(text);
        }
    }

    pub fn subscribe(&self) -> broadcast::Receiver<String> {
        self.tx.subscribe()
    }
}
