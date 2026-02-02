//! Adapter runtime integration.
//!
//! Bridges the sync game loop with the async TCP server.

use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use crate::adapter::server::{run_server, ServerConfig, ServerState};
use crate::types::{GameAction, Rotation};

/// Command delivered to the game loop.
#[derive(Debug, Clone)]
pub struct InboundCommand {
    pub client_id: usize,
    pub seq: u64,
    pub command: ClientCommand,
}

/// Command payload.
#[derive(Debug, Clone)]
pub enum ClientCommand {
    Actions(Vec<GameAction>),
    Place {
        x: i8,
        rotation: Rotation,
        use_hold: bool,
    },
}

/// Outbound message to be delivered by the server.
#[derive(Debug, Clone)]
pub enum OutboundMessage {
    ToClient { client_id: usize, line: String },
    Broadcast { line: String },
}

/// Running adapter instance.
pub struct Adapter {
    _rt: Runtime,
    cmd_rx: mpsc::Receiver<InboundCommand>,
    out_tx: mpsc::UnboundedSender<OutboundMessage>,
}

impl Adapter {
    /// Start the adapter from environment variables.
    ///
    /// Returns None if `TETRIS_AI_DISABLED` is set.
    pub fn start_from_env() -> Option<Self> {
        if ServerState::is_disabled() {
            return None;
        }

        let config = ServerConfig::from_env();
        let max_pending = config.max_pending_commands.max(1);
        let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(max_pending);
        let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();

        let rt = Runtime::new().expect("Failed to create tokio runtime");
        rt.spawn(async move {
            let _ = run_server(config, cmd_tx, out_rx, None).await;
        });

        Some(Self {
            _rt: rt,
            cmd_rx,
            out_tx,
        })
    }

    pub fn try_recv(&mut self) -> Option<InboundCommand> {
        self.cmd_rx.try_recv().ok()
    }

    pub fn send(&self, msg: OutboundMessage) {
        let _ = self.out_tx.send(msg);
    }
}
