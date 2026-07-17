//! Adapter runtime integration.
//!
//! Bridges the sync game loop with the async TCP server.

use std::net::SocketAddr;
use std::time::Duration;

use tokio::runtime::Runtime;
use tokio::sync::{mpsc, oneshot, watch};

use arrayvec::ArrayVec;

use std::sync::Arc;

use crate::adapter::protocol::{AckMessage, ErrorMessage, ObservationMessage};
use crate::adapter::server::{run_server_with_startup, ServerConfig, ServerState};
use crate::types::{GameAction, Rotation};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterStatus {
    pub client_count: u16,
    pub controller_id: Option<usize>,
    pub streaming_count: u16,
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
    Actions {
        actions: ArrayVec<GameAction, 32>,
        restart_seed: Option<u32>,
    },
    Place {
        x: i8,
        rotation: Rotation,
        use_hold: bool,
    },
}

/// Outbound message to be delivered by the server.
#[derive(Debug, Clone)]
pub enum OutboundMessage {
    ToClient {
        client_id: usize,
        line: String,
    },
    /// Send a pre-built line without taking ownership of a `String`.
    ToClientArc {
        client_id: usize,
        line: Arc<str>,
    },
    Broadcast {
        line: String,
    },
    /// Broadcast a pre-built line without cloning a `String` per fan-out source.
    BroadcastArc {
        line: Arc<str>,
    },
    ToClientObservation {
        client_id: usize,
        obs: ObservationMessage,
    },
    BroadcastObservation {
        obs: ObservationMessage,
    },
    /// Same as `ToClientObservation` but avoids cloning large observations for fan-out paths.
    ToClientObservationArc {
        client_id: usize,
        obs: Arc<ObservationMessage>,
    },
    /// Same as `BroadcastObservation` but avoids cloning large observations for each client.
    BroadcastObservationArc {
        obs: Arc<ObservationMessage>,
    },
    ToClientAck {
        client_id: usize,
        ack: AckMessage,
    },
    ToClientError {
        client_id: usize,
        err: ErrorMessage,
    },
}

/// Running adapter instance.
pub struct Adapter {
    _rt: Runtime,
    cmd_rx: mpsc::Receiver<InboundCommand>,
    out_tx: mpsc::UnboundedSender<OutboundMessage>,
    status_rx: watch::Receiver<AdapterStatus>,
    listen_addr: SocketAddr,
}

impl Adapter {
    /// Start the adapter from environment variables.
    ///
    /// Returns None if `TETRIS_AI_DISABLED` is set.
    pub fn start_from_env() -> anyhow::Result<Option<Self>> {
        if ServerState::is_disabled() {
            return Ok(None);
        }

        Self::start(ServerConfig::from_env()).map(Some)
    }

    /// Start the adapter with an explicit configuration.
    ///
    /// This returns only after the async server has completed its authoritative
    /// TCP bind, so a successful adapter always has a valid listen address.
    pub fn start(config: ServerConfig) -> anyhow::Result<Self> {
        if ServerState::is_disabled() {
            return Err(anyhow::anyhow!("AI adapter is disabled"));
        }

        let max_pending = config.max_pending_commands.max(1);
        let (cmd_tx, cmd_rx) = mpsc::channel::<InboundCommand>(max_pending);
        let (out_tx, out_rx) = mpsc::unbounded_channel::<OutboundMessage>();
        let (status_tx, status_rx) = watch::channel(AdapterStatus {
            client_count: 0,
            controller_id: None,
            streaming_count: 0,
        });
        let (startup_tx, startup_rx) = oneshot::channel::<Result<SocketAddr, String>>();

        let rt = Runtime::new()
            .map_err(|error| anyhow::anyhow!("failed to create adapter runtime: {error}"))?;
        rt.spawn(async move {
            let _ =
                run_server_with_startup(config, cmd_tx, out_rx, startup_tx, Some(status_tx)).await;
        });

        let listen_addr = match rt
            .block_on(async move { tokio::time::timeout(Duration::from_secs(2), startup_rx).await })
        {
            Ok(Ok(Ok(address))) => address,
            Ok(Ok(Err(error))) => return Err(anyhow::anyhow!(error)),
            Ok(Err(_)) => return Err(anyhow::anyhow!("AI adapter startup task stopped early")),
            Err(_) => return Err(anyhow::anyhow!("AI adapter startup timed out")),
        };

        Ok(Self {
            _rt: rt,
            cmd_rx,
            out_tx,
            status_rx,
            listen_addr,
        })
    }

    pub fn try_recv(&mut self) -> Option<InboundCommand> {
        self.cmd_rx.try_recv().ok()
    }

    pub fn try_recv_status(&mut self) -> Option<AdapterStatus> {
        match self.status_rx.has_changed() {
            Ok(true) => Some(*self.status_rx.borrow_and_update()),
            Ok(false) | Err(_) => None,
        }
    }

    pub fn listen_addr(&self) -> SocketAddr {
        self.listen_addr
    }

    pub fn send(&self, msg: OutboundMessage) {
        let _ = self.out_tx.send(msg);
    }
}
