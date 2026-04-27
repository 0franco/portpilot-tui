use std::collections::HashMap;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::schema::TunnelConfig;
use crate::events::AppEvent;
use crate::tunnel::worker;

pub struct TunnelManager {
    tokens: HashMap<String, CancellationToken>,
    // JoinHandles let us await actual worker completion on shutdown.
    handles: HashMap<String, tokio::task::JoinHandle<()>>,
    tx: mpsc::Sender<AppEvent>,
}

impl TunnelManager {
    pub fn new(tx: mpsc::Sender<AppEvent>) -> Self {
        Self {
            tokens: HashMap::new(),
            handles: HashMap::new(),
            tx,
        }
    }

    pub fn start(&mut self, config: TunnelConfig) {
        if self.tokens.contains_key(&config.name) {
            return;
        }
        let token = CancellationToken::new();
        let handle = worker::spawn(config.clone(), self.tx.clone(), token.clone());
        self.tokens.insert(config.name.clone(), token);
        self.handles.insert(config.name.clone(), handle);
    }

    pub fn stop(&mut self, name: &str) {
        if let Some(token) = self.tokens.remove(name) {
            token.cancel();
        }
        self.handles.remove(name);
    }

    /// Cancel all workers and wait for them to finish before returning.
    pub async fn stop_all(&mut self) {
        for token in self.tokens.values() {
            token.cancel();
        }
        self.tokens.clear();

        let handles: Vec<_> = self.handles.drain().map(|(_, h)| h).collect();
        let _ = tokio::time::timeout(
            std::time::Duration::from_secs(3),
            futures_util::future::join_all(handles),
        )
        .await;
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.tokens.contains_key(name)
    }
}
