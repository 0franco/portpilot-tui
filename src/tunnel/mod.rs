pub mod manager;
pub mod worker;

#[derive(Clone, Debug)]
pub enum TunnelState {
    Stopped,
    Connecting,
    Up { pid: u32 },
    Failed { reason: String },
}

impl TunnelState {
    pub fn label(&self) -> &'static str {
        match self {
            TunnelState::Stopped      => "STOPPED",
            TunnelState::Connecting   => "CONNECTING",
            TunnelState::Up { .. }    => "UP",
            TunnelState::Failed { .. } => "FAILED",
        }
    }
}
