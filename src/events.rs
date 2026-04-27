use crossterm::event::KeyEvent;

use crate::tunnel::TunnelState;

#[derive(Debug)]
pub enum AppEvent {
    Key(KeyEvent),
    TunnelStateChanged { name: String, state: TunnelState },
    Log { tunnel: String, line: String },
    Tick,
}
