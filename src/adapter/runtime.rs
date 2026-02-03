//! Adapter runtime integration.
//!
//! Bridges the sync game loop with the async TCP server.

use tokio::runtime::Runtime;
use tokio::sync::mpsc;

use arrayvec::ArrayVec;

use std::sync::Arc;

use crate::adapter::server::{run_server, ServerConfig, ServerState};
use crate::adapter::protocol::{AckMessage, ErrorMessage, ObservationMessage};
use crate::types::{GameAction, Rotation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterStatus {
    pub client_count: u16,
    pub controller_id: Option<usize>,
}

/// Message delivered to the game loop.
#[derive(Debug, Clone)]
pub struct InboundCommand {
    pub client_id: usize,
    pub seq: u64,
    pub payload: InboundPayload,
}

/// Inbound payload types from the adapter.
#[derive(Debug, Clone)]
pub enum InboundPayload {
    /// Controller command to apply.
    Command(ClientCommand),
    /// Request an immediate observation snapshot for this client.
    SnapshotRequest,
}

/// Command payload.
#[derive(Debug, Clone)]
pub enum ClientCommand {
    Actions(ArrayVec<GameAction, 32>),
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
    ToClientObservation { client_id: usize, obs: ObservationMessage },
    BroadcastObservation { obs: ObservationMessage },
    /// Same as `ToClientObservation` but avoids cloning large observations for fan-out paths.
    ToClientObservationArc {
        client_id: usize,
        obs: Arc<ObservationMessage>,
    },
    /// Same as `BroadcastObservation` but avoids cloning large observations for each client.
    BroadcastObservationArc { obs: Arc<ObservationMessage> },
    ToClientAck { client_id: usize, ack: AckMessage },
    ToClientError { client_id: usize, err: ErrorMessage },
}

/// Running adapter instance.
pub struct Adapter {
    _rt: Runtime,
    cmd_rx: mpsc::Receiver<InboundCommand>,
    out_tx: mpsc::UnboundedSender<OutboundMessage>,
    status_rx: mpsc::UnboundedReceiver<AdapterStatus>,
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
        let (status_tx, status_rx) = mpsc::unbounded_channel::<AdapterStatus>();

        let rt = Runtime::new().expect("Failed to create tokio runtime");
        rt.spawn(async move {
            let _ = run_server(config, cmd_tx, out_rx, None, Some(status_tx)).await;
        });

        Some(Self {
            _rt: rt,
            cmd_rx,
            out_tx,
            status_rx,
        })
    }

    pub fn try_recv(&mut self) -> Option<InboundCommand> {
        self.cmd_rx.try_recv().ok()
    }

    pub fn try_recv_status(&mut self) -> Option<AdapterStatus> {
        self.status_rx.try_recv().ok()
    }

    pub fn send(&self, msg: OutboundMessage) {
        let _ = self.out_tx.send(msg);
    }
}
