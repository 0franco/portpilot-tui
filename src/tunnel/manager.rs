use std::collections::HashMap;

use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;

use crate::config::schema::TunnelConfig;
use crate::events::AppEvent;
use crate::tunnel::worker;

pub struct TunnelManager {
    tokens: HashMap<String, CancellationToken>,
    tx: mpsc::Sender<AppEvent>,
}

impl TunnelManager {
    pub fn new(tx: mpsc::Sender<AppEvent>) -> Self {
        Self { tokens: HashMap::new(), tx }
    }

    pub fn start(&mut self, config: TunnelConfig) {
        if self.tokens.contains_key(&config.name) {
            return;
        }
        let token = CancellationToken::new();
        self.tokens.insert(config.name.clone(), token.clone());
        worker::spawn(config, self.tx.clone(), token);
    }

    pub fn stop(&mut self, name: &str) {
        if let Some(token) = self.tokens.remove(name) {
            token.cancel();
        }
    }

    pub fn stop_all(&mut self) {
        for token in self.tokens.values() {
            token.cancel();
        }
        self.tokens.clear();
    }

    pub fn is_running(&self, name: &str) -> bool {
        self.tokens.contains_key(name)
    }
}
