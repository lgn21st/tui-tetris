//! TCP server for AI adapter
//!
//! Handles incoming connections and manages client lifecycle.
//! Uses tokio for async networking.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::io::BufWriter;
use tokio::time::{Duration, MissedTickBehavior};

use crate::adapter::protocol::*;
use crate::adapter::runtime::{AdapterStatus, ClientCommand, InboundCommand, InboundPayload, OutboundMessage};
use crate::core::GameSnapshot;
use crate::types::{GameAction, Rotation};

use arrayvec::ArrayVec;

pub fn check_tcp_listen_available(host: &str, port: u16) -> std::io::Result<()> {
    if port == 0 {
        return Ok(());
    }
    let listener = std::net::TcpListener::bind((host, port))?;
    drop(listener);
    Ok(())
}

/// Stable 64-bit FNV-1a hasher for deterministic `state_hash`.
///
/// We avoid `DefaultHasher` here since its output is not guaranteed stable across
/// Rust versions/platforms.
#[derive(Debug, Clone)]
struct Fnv1aHasher {
    state: u64,
}

fn extract_seq_best_effort(s: &str) -> Option<u64> {
    let start = s.find("\"seq\"")?;
    let after_key = &s[start + 5..];
    let colon = after_key.find(':')?;
    let rest = after_key[colon + 1..].trim_start();
    let mut end = 0usize;
    for b in rest.as_bytes() {
        if b.is_ascii_digit() {
            end += 1;
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    rest[..end].parse::<u64>().ok()
}

fn clear_stale_controller_id<F>(controller: &mut Option<usize>, is_live: F)
where
    F: Fn(usize) -> bool,
{
    if let Some(stale_id) = *controller {
        if !is_live(stale_id) {
            *controller = None;
        }
    }
}

impl Fnv1aHasher {
    const OFFSET_BASIS: u64 = 0xcbf29ce484222325;
    const PRIME: u64 = 0x100000001b3;

    fn new() -> Self {
        Self {
            state: Self::OFFSET_BASIS,
        }
    }
}

impl std::hash::Hasher for Fnv1aHasher {
    fn finish(&self) -> u64 {
        self.state
    }

    fn write(&mut self, bytes: &[u8]) {
        for &b in bytes {
            self.state ^= b as u64;
            self.state = self.state.wrapping_mul(Self::PRIME);
        }
    }
}

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub protocol_version: String,
    pub max_pending_commands: usize,
    pub log_path: Option<String>,
    pub log_every_n: u64,
    pub log_max_lines: Option<u64>,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 7777,
            protocol_version: "2.0.0".to_string(),
            max_pending_commands: 10,
            log_path: None,
            log_every_n: 1,
            log_max_lines: None,
        }
    }
}

impl ServerConfig {
    /// Create from environment variables (matching `docs/adapter.md` defaults)
    pub fn from_env() -> Self {
        use std::env;

        let host = env::var("TETRIS_AI_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = env::var("TETRIS_AI_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(7777);

        let max_pending_commands = env::var("TETRIS_AI_MAX_PENDING")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(10);

        let log_path = env::var("TETRIS_AI_LOG_PATH")
            .ok()
            .map(|s| s.trim().to_string())
            .and_then(|s| if s.is_empty() { None } else { Some(s) });

        let log_every_n = env::var("TETRIS_AI_LOG_EVERY_N")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&n| n >= 1)
            .unwrap_or(1);

        let log_max_lines = env::var("TETRIS_AI_LOG_MAX_LINES")
            .ok()
            .and_then(|s| s.parse::<u64>().ok())
            .filter(|&n| n >= 1);

        Self {
            host,
            port,
            protocol_version: "2.0.0".to_string(),
            max_pending_commands,
            log_path,
            log_every_n,
            log_max_lines,
        }
    }

    pub fn socket_addr(&self) -> SocketAddr {
        format!("{}:{}", self.host, self.port)
            .parse()
            .expect("Invalid socket address")
    }
}

/// Shared server state
pub struct ServerState {
    config: ServerConfig,
    clients: Arc<RwLock<Vec<ClientHandle>>>,
    controller: Arc<RwLock<Option<usize>>>, // Index into clients vec
    status_tx: Option<mpsc::UnboundedSender<AdapterStatus>>,
}

impl ServerState {
    pub fn new(config: ServerConfig, status_tx: Option<mpsc::UnboundedSender<AdapterStatus>>) -> Self {
        Self {
            config,
            clients: Arc::new(RwLock::new(Vec::new())),
            controller: Arc::new(RwLock::new(None)),
            status_tx,
        }
    }

    /// Check if AI is disabled via environment
    pub fn is_disabled() -> bool {
        std::env::var("TETRIS_AI_DISABLED")
            .map(|v| {
                let v = v.trim();
                v == "1" || v.eq_ignore_ascii_case("true")
            })
            .unwrap_or(false)
    }
}

async fn emit_status(state: &Arc<ServerState>) {
    let Some(tx) = state.status_tx.as_ref() else {
        return;
    };
    let controller = state.controller.read().await;
    let clients = state.clients.read().await;
    let live_client_count = clients
        .iter()
        .filter(|c| !c.tx.is_closed())
        .count()
        .min(u16::MAX as usize) as u16;
    let controller_id = controller.and_then(|id| {
        clients
            .iter()
            .any(|c| c.id == id && !c.tx.is_closed())
            .then_some(id)
    });
    let streaming_count = clients
        .iter()
        .filter(|c| c.stream_observations && !c.tx.is_closed())
        .count()
        .min(u16::MAX as usize) as u16;
    let _ = tx.send(AdapterStatus {
        client_count: live_client_count,
        controller_id,
        streaming_count,
    });
}

async fn is_handshaken(state: &Arc<ServerState>, client_id: usize) -> bool {
    let clients = state.clients.read().await;
    clients
        .iter()
        .find(|c| c.id == client_id)
        .map(|c| c.handshaken)
        .unwrap_or(false)
}

async fn check_and_update_seq(state: &Arc<ServerState>, client_id: usize, seq: u64) -> bool {
    let mut clients = state.clients.write().await;
    let Some(client) = clients.iter_mut().find(|c| c.id == client_id) else {
        return true;
    };

    match client.last_seq {
        None => {
            client.last_seq = Some(seq);
            true
        }
        Some(prev) => {
            if seq <= prev {
                false
            } else {
                client.last_seq = Some(seq);
                true
            }
        }
    }
}

/// Handle to a connected client
pub struct ClientHandle {
    pub id: usize,
    pub addr: SocketAddr,
    pub is_controller: bool,
    pub command_mode: CommandMode,
    pub stream_observations: bool,
    pub handshaken: bool,
    pub last_seq: Option<u64>,
    pub tx: mpsc::UnboundedSender<ClientOutbound>, // Channel to send messages to client
}

#[derive(Debug, Clone)]
pub enum ClientOutbound {
    LineArc(Arc<str>),
    Ack(AckMessage),
    Error(ErrorMessage),
    Welcome(WelcomeMessage),
    Observation(ObservationMessage),
    ObservationArc(Arc<ObservationMessage>),
}

#[derive(Debug, Clone)]
enum WireRecord {
    LineArc(Arc<str>),
    Welcome(WelcomeMessage),
    Ack(AckMessage),
    Error(ErrorMessage),
    Observation(ObservationMessage),
    ObservationArc(Arc<ObservationMessage>),
}

/// Start the TCP server
pub async fn run_server(
    config: ServerConfig,
    command_tx: mpsc::Sender<InboundCommand>,
    mut out_rx: mpsc::UnboundedReceiver<OutboundMessage>,
    ready_tx: Option<oneshot::Sender<SocketAddr>>,
    status_tx: Option<mpsc::UnboundedSender<AdapterStatus>>,
) -> anyhow::Result<()> {
    if ServerState::is_disabled() {
        // Just drain the command channel to prevent blocking
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    let wire_log_tx: Option<mpsc::UnboundedSender<WireRecord>> = if let Some(path) = config.log_path.clone() {
        let log_every_n = config.log_every_n.max(1);
        let log_max_lines = config.log_max_lines;
        let (tx, mut rx) = mpsc::unbounded_channel::<WireRecord>();
        tokio::spawn(async move {
            use tokio::fs::OpenOptions;
            use tokio::io::AsyncWriteExt;

            let mut file = match OpenOptions::new().create(true).append(true).open(&path).await {
                Ok(f) => f,
                Err(_) => return,
            };

            let mut buf: Vec<u8> = Vec::with_capacity(4096);
            let mut line_count: u64 = 0;
            let mut record_count: u64 = 0;

            while let Some(rec) = rx.recv().await {
                record_count = record_count.wrapping_add(1);
                if record_count % log_every_n != 0 {
                    continue;
                }
                if let Some(max) = log_max_lines {
                    if line_count >= max {
                        continue;
                    }
                }
                match rec {
                    WireRecord::LineArc(s) => {
                        if file.write_all(s.as_bytes()).await.is_err() {
                            break;
                        }
                    }
                    WireRecord::Welcome(v) => {
                        buf.clear();
                        if serde_json::to_writer(&mut buf, &v).is_err() {
                            continue;
                        }
                        if file.write_all(&buf).await.is_err() {
                            break;
                        }
                    }
                    WireRecord::Ack(v) => {
                        buf.clear();
                        if serde_json::to_writer(&mut buf, &v).is_err() {
                            continue;
                        }
                        if file.write_all(&buf).await.is_err() {
                            break;
                        }
                    }
                    WireRecord::Error(v) => {
                        buf.clear();
                        if serde_json::to_writer(&mut buf, &v).is_err() {
                            continue;
                        }
                        if file.write_all(&buf).await.is_err() {
                            break;
                        }
                    }
                    WireRecord::Observation(v) => {
                        buf.clear();
                        if serde_json::to_writer(&mut buf, &v).is_err() {
                            continue;
                        }
                        if file.write_all(&buf).await.is_err() {
                            break;
                        }
                    }
                    WireRecord::ObservationArc(v) => {
                        buf.clear();
                        if serde_json::to_writer(&mut buf, v.as_ref()).is_err() {
                            continue;
                        }
                        if file.write_all(&buf).await.is_err() {
                            break;
                        }
                    }
                }
                if file.write_all(b"\n").await.is_err() {
                    break;
                }
                line_count = line_count.wrapping_add(1);
            }

            let _ = file.flush().await;
        });
        Some(tx)
    } else {
        None
    };

    let addr = config.socket_addr();
    let listener = TcpListener::bind(&addr).await?;
    let bound = listener.local_addr()?;
    // Emit initial status (0 clients).
    if let Some(tx) = status_tx.as_ref() {
        let _ = tx.send(AdapterStatus {
            client_count: 0,
            controller_id: None,
            streaming_count: 0,
        });
    }
    if let Some(tx) = ready_tx {
        let _ = tx.send(bound);
    }

    let state = Arc::new(ServerState::new(config, status_tx));
    let mut client_id_counter = 0usize;

    // Outbound dispatcher.
    {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                match msg {
                    OutboundMessage::ToClient { client_id, line } => {
                        let clients = state.clients.read().await;
                        if let Some(c) = clients.iter().find(|c| c.id == client_id) {
                            let _ = c.tx.send(ClientOutbound::LineArc(Arc::from(line)));
                        }
                    }
                    OutboundMessage::ToClientArc { client_id, line } => {
                        let clients = state.clients.read().await;
                        if let Some(c) = clients.iter().find(|c| c.id == client_id) {
                            let _ = c.tx.send(ClientOutbound::LineArc(line));
                        }
                    }
                    OutboundMessage::Broadcast { line } => {
                        let clients = state.clients.read().await;
                        let line: Arc<str> = Arc::from(line);
                        for c in clients.iter() {
                            if c.stream_observations {
                                let _ = c.tx.send(ClientOutbound::LineArc(Arc::clone(&line)));
                            }
                        }
                    }
                    OutboundMessage::BroadcastArc { line } => {
                        let clients = state.clients.read().await;
                        for c in clients.iter() {
                            if c.stream_observations {
                                let _ = c.tx.send(ClientOutbound::LineArc(Arc::clone(&line)));
                            }
                        }
                    }
                    OutboundMessage::ToClientObservation { client_id, obs } => {
                        let clients = state.clients.read().await;
                        if let Some(c) = clients.iter().find(|c| c.id == client_id) {
                            let _ = c.tx.send(ClientOutbound::Observation(obs));
                        }
                    }
                    OutboundMessage::BroadcastObservation { obs } => {
                        let clients = state.clients.read().await;
                        let obs = Arc::new(obs);
                        for c in clients.iter() {
                            if c.stream_observations {
                                let _ = c.tx.send(ClientOutbound::ObservationArc(Arc::clone(&obs)));
                            }
                        }
                    }
                    OutboundMessage::ToClientObservationArc { client_id, obs } => {
                        let clients = state.clients.read().await;
                        if let Some(c) = clients.iter().find(|c| c.id == client_id) {
                            let _ = c.tx.send(ClientOutbound::ObservationArc(obs));
                        }
                    }
                    OutboundMessage::BroadcastObservationArc { obs } => {
                        let clients = state.clients.read().await;
                        for c in clients.iter() {
                            if c.stream_observations {
                                let _ = c.tx.send(ClientOutbound::ObservationArc(Arc::clone(&obs)));
                            }
                        }
                    }
                    OutboundMessage::ToClientAck { client_id, ack } => {
                        let clients = state.clients.read().await;
                        if let Some(c) = clients.iter().find(|c| c.id == client_id) {
                            let _ = c.tx.send(ClientOutbound::Ack(ack));
                        }
                    }
                    OutboundMessage::ToClientError { client_id, err } => {
                        let clients = state.clients.read().await;
                        if let Some(c) = clients.iter().find(|c| c.id == client_id) {
                            let _ = c.tx.send(ClientOutbound::Error(err));
                        }
                    }
                }
            }
        });
    }

    // Accept incoming connections
    loop {
        let (socket, addr) = listener.accept().await?;
        client_id_counter += 1;
        let client_id = client_id_counter;

    emit_status(&state).await;

        let state_clone = Arc::clone(&state);
        let command_tx = command_tx.clone();
        let wire_log_tx = wire_log_tx.clone();

        // Spawn task to handle this client
        tokio::spawn(async move {
            if let Err(e) =
                handle_client(socket, addr, client_id, state_clone, command_tx, wire_log_tx).await
            {
                let _ = e;
            }
        });
    }
}

/// Handle a single client connection
async fn handle_client(
    socket: TcpStream,
    addr: SocketAddr,
    client_id: usize,
    state: Arc<ServerState>,
    command_tx: mpsc::Sender<InboundCommand>,
    wire_log_tx: Option<mpsc::UnboundedSender<WireRecord>>,
) -> anyhow::Result<()> {
    let (reader, writer) = tokio::io::split(socket);
    let mut writer = BufWriter::with_capacity(16 * 1024, writer);
    let mut reader = BufReader::new(reader);

    // Channel to send messages to this client
    let (tx, mut rx) = mpsc::unbounded_channel::<ClientOutbound>();

    // Add client to list
    let client_handle = ClientHandle {
        id: client_id,
        addr,
        is_controller: false,
        command_mode: CommandMode::Action,
        stream_observations: false,
        handshaken: false,
        last_seq: None,
        tx: tx.clone(),
    };

    {
        let mut clients = state.clients.write().await;
        clients.push(client_handle);
    }

    emit_status(&state).await;

    let wire_log_tx_out = wire_log_tx.clone();

    // Spawn task to write messages to client
    let write_task = tokio::spawn(async move {
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        let mut dirty = false;
        let mut flush_tick = tokio::time::interval(Duration::from_millis(16));
        flush_tick.set_missed_tick_behavior(MissedTickBehavior::Delay);

        loop {
            let msg = tokio::select! {
                msg = rx.recv() => msg,
                _ = flush_tick.tick(), if dirty => {
                    if writer.flush().await.is_err() {
                        break;
                    }
                    dirty = false;
                    continue;
                }
            };

            let Some(msg) = msg else {
                break;
            };

            let flush_after = matches!(
                msg,
                ClientOutbound::Ack(_) | ClientOutbound::Error(_) | ClientOutbound::Welcome(_)
            );

            match msg {
                ClientOutbound::LineArc(line) => {
                    if writer.write_all(line.as_bytes()).await.is_err() {
                        break;
                    }
                    if let Some(tx) = wire_log_tx_out.as_ref() {
                        let _ = tx.send(WireRecord::LineArc(line));
                    }
                }
                ClientOutbound::Ack(ack) => {
                    buf.clear();
                    if serde_json::to_writer(&mut buf, &ack).is_err() {
                        continue;
                    }
                    if writer.write_all(&buf).await.is_err() {
                        break;
                    }
                    if let Some(tx) = wire_log_tx_out.as_ref() {
                        let _ = tx.send(WireRecord::Ack(ack));
                    }
                }
                ClientOutbound::Error(err) => {
                    buf.clear();
                    if serde_json::to_writer(&mut buf, &err).is_err() {
                        continue;
                    }
                    if writer.write_all(&buf).await.is_err() {
                        break;
                    }
                    if let Some(tx) = wire_log_tx_out.as_ref() {
                        let _ = tx.send(WireRecord::Error(err));
                    }
                }
                ClientOutbound::Welcome(welcome) => {
                    buf.clear();
                    if serde_json::to_writer(&mut buf, &welcome).is_err() {
                        continue;
                    }
                    if writer.write_all(&buf).await.is_err() {
                        break;
                    }
                    if let Some(tx) = wire_log_tx_out.as_ref() {
                        let _ = tx.send(WireRecord::Welcome(welcome));
                    }
                }
                ClientOutbound::Observation(obs) => {
                    buf.clear();
                    if serde_json::to_writer(&mut buf, &obs).is_err() {
                        continue;
                    }
                    if writer.write_all(&buf).await.is_err() {
                        break;
                    }
                    if let Some(tx) = wire_log_tx_out.as_ref() {
                        let _ = tx.send(WireRecord::Observation(obs));
                    }
                }
                ClientOutbound::ObservationArc(obs) => {
                    buf.clear();
                    if serde_json::to_writer(&mut buf, obs.as_ref()).is_err() {
                        continue;
                    }
                    if writer.write_all(&buf).await.is_err() {
                        break;
                    }
                    if let Some(tx) = wire_log_tx_out.as_ref() {
                        let _ = tx.send(WireRecord::ObservationArc(obs));
                    }
                }
            }

            if writer.write_all(b"\n").await.is_err() {
                break;
            }

            dirty = true;
            if flush_after {
                if writer.flush().await.is_err() {
                    break;
                }
                dirty = false;
            }
        }
        let _ = writer.flush().await;
    });

    // Handle incoming messages
    let mut line = String::new();

    loop {
        line.clear();
        let bytes_read = match reader.read_line(&mut line).await {
            Ok(n) => n,
            Err(_) => {
                // Treat I/O errors the same as a disconnect for lifecycle cleanup purposes.
                // Some clients may terminate abruptly, and we must still release/promote controller.
                break;
            }
        };

        if bytes_read == 0 {
            // Client disconnected
            break;
        }

        let raw_line = line.trim_end_matches(|c| c == '\n' || c == '\r');
        let trimmed = raw_line.trim();
        if trimmed.is_empty() {
            continue;
        }

        if let Some(tx) = wire_log_tx.as_ref() {
            let _ = tx.send(WireRecord::LineArc(Arc::from(raw_line)));
        }

        // Parse the message
        match parse_message(trimmed) {
            Ok(ParsedMessage::Hello(hello)) => {
                // Require hello to start the per-sender sequence at 1.
                if hello.seq != 1 {
                    let error = create_error(
                        hello.seq,
                        ErrorCode::InvalidCommand,
                        "hello seq must be 1",
                    );
                    let _ = tx.send(ClientOutbound::Error(error));
                    continue;
                }

                // Sequencing: enforce monotonic seq per sender.
                if is_handshaken(&state, client_id).await
                    && !check_and_update_seq(&state, client_id, hello.seq).await
                {
                    let error = create_error(
                        hello.seq,
                        ErrorCode::InvalidCommand,
                        "seq must be strictly increasing",
                    );
                    let _ = tx.send(ClientOutbound::Error(error));
                    continue;
                }

                // Validate protocol version
                if !hello.protocol_version.starts_with("2.") {
                    let error = create_error(
                        hello.seq,
                        ErrorCode::ProtocolMismatch,
                        &format!("Protocol version {} not supported", hello.protocol_version),
                    );
                    let _ = tx.send(ClientOutbound::Error(error));
                    break;
                }

                if !hello.formats.json {
                    let error = create_error(
                        hello.seq,
                        ErrorCode::InvalidCommand,
                        "formats must include json",
                    );
                    let _ = tx.send(ClientOutbound::Error(error));
                    continue;
                }

                // Mark client as handshaken and store requested capabilities.
                {
                    let mut clients = state.clients.write().await;
                    if let Some(client) = clients.iter_mut().find(|c| c.id == client_id) {
                        client.handshaken = true;
                        client.last_seq = Some(hello.seq);
                        client.command_mode = hello.requested.command_mode;
                        client.stream_observations = hello.requested.stream_observations;
                    }
                }

                // Role/controller assignment:
                // - Default policy: when no controller is assigned, first hello becomes controller.
                // - If hello.requested.role == observer: never auto-assign controller as a side-effect of hello.
                let mut assigned_role = AssignedRole::Observer;
                let controller_id: Option<usize> = {
                    let mut controller = state.controller.write().await;

                    // If a prior controller disconnected unexpectedly, we may have a stale controller_id
                    // that blocks future claims. Clear it when it no longer exists in the client list.
                    {
                        let clients = state.clients.read().await;
                        clear_stale_controller_id(&mut *controller, |id| {
                            clients.iter().any(|c| c.id == id && !c.tx.is_closed())
                        });
                    }

                    let requested_role = hello.requested.role.unwrap_or(RequestedRole::Auto);
                    let allow_auto_controller = requested_role != RequestedRole::Observer;

                    if *controller == Some(client_id) {
                        assigned_role = AssignedRole::Controller;
                    } else if controller.is_none() && allow_auto_controller {
                        *controller = Some(client_id);
                        assigned_role = AssignedRole::Controller;
                    }

                    *controller
                };

                // Keep per-client role flags consistent with the global controller id.
                {
                    let mut clients = state.clients.write().await;
                    for c in clients.iter_mut() {
                        c.is_controller = controller_id.is_some_and(|id| c.id == id);
                    }
                }

                // Send welcome (with deterministic role/controller fields).
                let welcome = create_welcome(
                    hello.seq,
                    &state.config.protocol_version,
                    client_id as u64,
                    assigned_role,
                    controller_id.map(|id| id as u64),
                );
                let _ = tx.send(ClientOutbound::Welcome(welcome));

                // Request an immediate snapshot for this client if desired.
                if hello.requested.stream_observations {
                    let _ = command_tx.try_send(InboundCommand {
                        client_id,
                        seq: hello.seq,
                        payload: InboundPayload::SnapshotRequest,
                    });
                }

                emit_status(&state).await;
            }

            Ok(ParsedMessage::Command(cmd)) => {
                // Handshake required.
                let handshaken = is_handshaken(&state, client_id).await;
                if !handshaken {
                    let error =
                        create_error(
                            cmd.seq,
                            ErrorCode::HandshakeRequired,
                            "Send hello before command",
                        );
                    let _ = tx.send(ClientOutbound::Error(error));
                    continue;
                }

                // Sequencing: enforce monotonic seq per sender.
                if !check_and_update_seq(&state, client_id, cmd.seq).await {
                    let error = create_error(
                        cmd.seq,
                        ErrorCode::InvalidCommand,
                        "seq must be strictly increasing",
                    );
                    let _ = tx.send(ClientOutbound::Error(error));
                    continue;
                }

                // Check if client is controller
                let is_controller = {
                    let clients = state.clients.read().await;
                    clients
                        .iter()
                        .find(|c| c.id == client_id)
                        .map(|c| c.is_controller)
                        .unwrap_or(false)
                };

                if !is_controller {
                    let error = create_error(
                        cmd.seq,
                        ErrorCode::NotController,
                        "Only controller may send commands",
                    );
                    let _ = tx.send(ClientOutbound::Error(error));
                    continue;
                }

                // Map command into an inbound command for the game loop.
                let mapped = match map_command(&cmd) {
                    Ok(c) => c,
                    Err((code, message)) => {
                        let error = create_error(cmd.seq, code, &message);
                        let _ = tx.send(ClientOutbound::Error(error));
                        continue;
                    }
                };

                // Backpressure: bounded queue.
                match command_tx.try_send(InboundCommand {
                    client_id,
                    seq: cmd.seq,
                    payload: InboundPayload::Command(mapped),
                }) {
                    Ok(()) => {
                        // Ack will be sent by the game loop after the command is applied.
                    }
                    Err(_) => {
                        let error = create_error(
                            cmd.seq,
                            ErrorCode::Backpressure,
                            "Command queue is full",
                        );
                        let _ = tx.send(ClientOutbound::Error(error));
                    }
                }
            }

            Ok(ParsedMessage::Control(ctrl)) => match ctrl.action {
                ControlAction::Claim => {
                    // Handshake required.
                    let handshaken = is_handshaken(&state, client_id).await;
                    if !handshaken {
                        let error = create_error(
                            ctrl.seq,
                            ErrorCode::HandshakeRequired,
                            "Send hello before control",
                        );
                        let _ = tx.send(ClientOutbound::Error(error));
                        continue;
                    }

                    // Sequencing: enforce monotonic seq per sender.
                    if !check_and_update_seq(&state, client_id, ctrl.seq).await {
                        let error = create_error(
                            ctrl.seq,
                            ErrorCode::InvalidCommand,
                            "seq must be strictly increasing",
                        );
                        let _ = tx.send(ClientOutbound::Error(error));
                        continue;
                    }

                    let mut should_emit_status = false;
                    {
                        let mut controller = state.controller.write().await;
                        // Clear stale controller_id (e.g. if the controller client crashed/disconnected).
                        {
                            let clients = state.clients.read().await;
                            clear_stale_controller_id(&mut *controller, |id| {
                                clients.iter().any(|c| c.id == id && !c.tx.is_closed())
                            });
                        }
                        if *controller == Some(client_id) {
                            // Idempotent self-claim: already controller.
                            let mut clients = state.clients.write().await;
                            for c in clients.iter_mut() {
                                c.is_controller = c.id == client_id;
                            }
                            let ack = create_ack(ctrl.seq, ctrl.seq);
                            let _ = tx.send(ClientOutbound::Ack(ack));
                            should_emit_status = true;
                        } else if controller.is_none() {
                            *controller = Some(client_id);
                            let mut clients = state.clients.write().await;
                            for c in clients.iter_mut() {
                                c.is_controller = c.id == client_id;
                            }
                            let ack = create_ack(ctrl.seq, ctrl.seq);
                            let _ = tx.send(ClientOutbound::Ack(ack));
                            should_emit_status = true;
                        } else {
                            let error = create_error(
                                ctrl.seq,
                                ErrorCode::ControllerActive,
                                "Controller already assigned",
                            );
                            let _ = tx.send(ClientOutbound::Error(error));
                        }
                    }
                    if should_emit_status {
                        emit_status(&state).await;
                    }
                }
                ControlAction::Release => {
                    // Handshake required.
                    let handshaken = is_handshaken(&state, client_id).await;
                    if !handshaken {
                        let error = create_error(
                            ctrl.seq,
                            ErrorCode::HandshakeRequired,
                            "Send hello before control",
                        );
                        let _ = tx.send(ClientOutbound::Error(error));
                        continue;
                    }

                    // Sequencing: enforce monotonic seq per sender.
                    if !check_and_update_seq(&state, client_id, ctrl.seq).await {
                        let error = create_error(
                            ctrl.seq,
                            ErrorCode::InvalidCommand,
                            "seq must be strictly increasing",
                        );
                        let _ = tx.send(ClientOutbound::Error(error));
                        continue;
                    }

                    let mut should_emit_status = false;
                    {
                        let mut controller = state.controller.write().await;
                        if *controller == Some(client_id) {
                            *controller = None;
                            let mut clients = state.clients.write().await;
                            if let Some(client) = clients.iter_mut().find(|c| c.id == client_id) {
                                client.is_controller = false;
                            }
                            let ack = create_ack(ctrl.seq, ctrl.seq);
                            let _ = tx.send(ClientOutbound::Ack(ack));
                            should_emit_status = true;
                        } else {
                            let error =
                                create_error(
                                    ctrl.seq,
                                    ErrorCode::NotController,
                                    "Only controller may release",
                                );
                            let _ = tx.send(ClientOutbound::Error(error));
                        }
                    }
                    if should_emit_status {
                        emit_status(&state).await;
                    }
                }
            },

            Err(e) => {
                let seq = extract_seq_best_effort(trimmed).unwrap_or(0);
                let error = create_error(
                    seq,
                    ErrorCode::InvalidCommand,
                    &format!("JSON parse error: {}", e),
                );
                let _ = tx.send(ClientOutbound::Error(error));
            }

            Ok(ParsedMessage::Unknown(unknown)) => {
                let seq = unknown.seq;
                if is_handshaken(&state, client_id).await && !check_and_update_seq(&state, client_id, seq).await {
                    let error = create_error(
                        seq,
                        ErrorCode::InvalidCommand,
                        "seq must be strictly increasing",
                    );
                    let _ = tx.send(ClientOutbound::Error(error));
                    continue;
                }
                let error = create_error(seq, ErrorCode::InvalidCommand, "Unknown message type");
                let _ = tx.send(ClientOutbound::Error(error));
            }
        }
    }

    // Clean up: remove client and release/promote controller if needed.
    {
        let mut controller = state.controller.write().await;
        let mut clients = state.clients.write().await;

        let was_controller = *controller == Some(client_id);
        clients.retain(|c| c.id != client_id);

        if was_controller {
            // Promote the next available client (lowest id) to controller.
            let next_id = clients.iter().filter(|c| !c.tx.is_closed()).map(|c| c.id).min();
            *controller = next_id;
            if let Some(new_id) = next_id {
                if let Some(c) = clients.iter_mut().find(|c| c.id == new_id) {
                    c.is_controller = true;
                }
            }
        }
    }

    emit_status(&state).await;

    // Cancel write task
    drop(tx);
    let _ = write_task.await;

    Ok(())
}

/// Map a protocol command into an engine command.
fn map_command(cmd: &CommandMessage) -> Result<ClientCommand, (ErrorCode, String)> {
    match cmd.mode {
        CommandMode::Action => {
            let Some(ActionList(ref list)) = cmd.actions else {
                return Err((ErrorCode::InvalidCommand, "Missing actions".to_string()));
            };
            let mut actions = ArrayVec::<GameAction, 32>::new();
            for a in list.iter().copied() {
                let ga = match a {
                    ActionName::MoveLeft => GameAction::MoveLeft,
                    ActionName::MoveRight => GameAction::MoveRight,
                    ActionName::SoftDrop => GameAction::SoftDrop,
                    ActionName::HardDrop => GameAction::HardDrop,
                    ActionName::RotateCw => GameAction::RotateCw,
                    ActionName::RotateCcw => GameAction::RotateCcw,
                    ActionName::Hold => GameAction::Hold,
                    ActionName::Pause => GameAction::Pause,
                    ActionName::Restart => GameAction::Restart,
                };
                actions
                    .try_push(ga)
                    .map_err(|_| (ErrorCode::InvalidCommand, "Too many actions".to_string()))?;
            }
            Ok(ClientCommand::Actions(actions))
        }
        CommandMode::Place => {
            let Some(ref place) = cmd.place else {
                return Err((ErrorCode::InvalidPlace, "Missing place".to_string()));
            };
            Ok(ClientCommand::Place {
                x: place.x,
                rotation: {
                    let rot_s = place.rotation.as_str();
                    if rot_s.eq_ignore_ascii_case("north") {
                        Rotation::North
                    } else if rot_s.eq_ignore_ascii_case("east") {
                        Rotation::East
                    } else if rot_s.eq_ignore_ascii_case("south") {
                        Rotation::South
                    } else if rot_s.eq_ignore_ascii_case("west") {
                        Rotation::West
                    } else {
                        return Err((
                            ErrorCode::InvalidPlace,
                            format!("Invalid rotation: {}", place.rotation),
                        ));
                    }
                },
                use_hold: place.use_hold,
            })
        }
    }
}

/// Build observation message from game state
pub fn build_observation(
    seq: u64,
    snap: &GameSnapshot,
    last_event: Option<LastEvent>,
) -> ObservationMessage {
    use std::hash::{Hash, Hasher};

    let cells = snap.board;

    // Build state hash
    let mut hasher = Fnv1aHasher::new();
    snap.board_hash.hash(&mut hasher);
    snap.board_id.hash(&mut hasher);
    snap.active.hash(&mut hasher);
    snap.hold.hash(&mut hasher);
    snap.can_hold.hash(&mut hasher);
    snap.next_queue.hash(&mut hasher);
    snap.paused.hash(&mut hasher);
    snap.game_over.hash(&mut hasher);
    snap.episode_id.hash(&mut hasher);
    snap.piece_id.hash(&mut hasher);
    snap.step_in_piece.hash(&mut hasher);
    snap.seed.hash(&mut hasher);
    snap.score.hash(&mut hasher);
    snap.level.hash(&mut hasher);
    snap.lines.hash(&mut hasher);
    snap.timers.drop_ms.hash(&mut hasher);
    snap.timers.lock_ms.hash(&mut hasher);
    snap.timers.line_clear_ms.hash(&mut hasher);
    // Include last_event since it is part of the observation payload.
    last_event.is_some().hash(&mut hasher);
    if let Some(ev) = last_event.as_ref() {
        ev.locked.hash(&mut hasher);
        ev.lines_cleared.hash(&mut hasher);
        ev.line_clear_score.hash(&mut hasher);
        ev.tspin.hash(&mut hasher);
        ev.combo.hash(&mut hasher);
        ev.back_to_back.hash(&mut hasher);
    }
    let state_hash = StateHash(hasher.finish());

    // Build next queue
    let next_queue: [PieceKindLower; 5] =
        std::array::from_fn(|i| PieceKindLower::from(snap.next_queue[i]));

    let next = next_queue[0];

    // Build active piece
    let active = snap.active.map(|piece| ActivePieceSnapshot {
        kind: PieceKindLower::from(piece.kind),
        rotation: RotationLower::from(piece.rotation),
        x: piece.x,
        y: piece.y,
    });

    // Build hold
    let hold = snap.hold.map(PieceKindLower::from);

    ObservationMessage {
        msg_type: ObservationType::Observation,
        seq,
        ts: current_timestamp_ms(),
        playable: snap.playable(),
        paused: snap.paused,
        game_over: snap.game_over,
        episode_id: snap.episode_id,
        seed: snap.seed,
        piece_id: snap.piece_id,
        step_in_piece: snap.step_in_piece,
        board: BoardSnapshot {
            width: 10,
            height: 20,
            cells,
        },
        board_id: snap.board_id,
        active,
        ghost_y: snap.ghost_y,
        next,
        next_queue,
        hold,
        can_hold: snap.can_hold,
        last_event,
        state_hash,
        score: snap.score,
        level: snap.level,
        lines: snap.lines,
        timers: TimersSnapshot {
            drop_ms: snap.timers.drop_ms,
            lock_ms: snap.timers.lock_ms,
            line_clear_ms: snap.timers.line_clear_ms,
        },
    }
}

/// Get current timestamp in milliseconds
fn current_timestamp_ms() -> u64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis() as u64
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::GameState;

    #[test]
    fn test_map_command_action_mode() {
        let json = r#"{"type":"command","seq":2,"ts":1,"mode":"action","actions":["moveLeft","rotateCw","hardDrop"]}"#;
        let ParsedMessage::Command(cmd) = parse_message(json).unwrap() else {
            panic!("expected command");
        };
        let mapped = map_command(&cmd).unwrap();
        match mapped {
            ClientCommand::Actions(actions) => {
                assert_eq!(actions.as_slice(), [GameAction::MoveLeft, GameAction::RotateCw, GameAction::HardDrop]);
            }
            _ => panic!("expected action mapping"),
        }
    }

    #[test]
    fn test_build_observation_copies_timers_fields() {
        let mut snap = crate::core::snapshot::GameSnapshot::default();
        snap.timers.drop_ms = 12;
        snap.timers.lock_ms = 34;
        snap.timers.line_clear_ms = 56;

        let obs = build_observation(1, &snap, None);
        assert_eq!(obs.timers.drop_ms, 12);
        assert_eq!(obs.timers.lock_ms, 34);
        assert_eq!(obs.timers.line_clear_ms, 56);
    }

    #[test]
    fn test_server_config_from_env() {
        // This test just ensures it doesn't panic
        let _config = ServerConfig::from_env();
    }

    #[test]
    fn test_clear_stale_controller_id_clears() {
        let mut controller = Some(42usize);
        clear_stale_controller_id(&mut controller, |id| id == 7);
        assert_eq!(controller, None);
    }

    #[test]
    fn test_clear_stale_controller_id_keeps() {
        let mut controller = Some(42usize);
        clear_stale_controller_id(&mut controller, |id| id == 42);
        assert_eq!(controller, Some(42));
    }

    #[test]
    fn test_state_hash_changes_when_meta_changes() {
        let mut gs = GameState::new(1);
        gs.start();

        let mut s1 = gs.snapshot();
        s1.episode_id = 0;
        s1.piece_id = 1;
        s1.step_in_piece = 0;
        let obs1 = build_observation(1, &s1, None);

        let mut s2 = gs.snapshot();
        s2.episode_id = 1;
        s2.piece_id = 2;
        s2.step_in_piece = 3;
        let obs2 = build_observation(2, &s2, None);
        assert_ne!(obs1.state_hash, obs2.state_hash);
    }

    #[test]
    fn test_state_hash_changes_when_hold_changes() {
        let mut gs = GameState::new(1);
        gs.start();

        let s1 = gs.snapshot();
        let obs1 = build_observation(1, &s1, None);
        assert!(gs.apply_action(GameAction::Hold));

        let s2 = gs.snapshot();
        let obs2 = build_observation(2, &s2, None);
        assert_ne!(obs1.state_hash, obs2.state_hash);
    }

    #[test]
    fn test_state_hash_changes_when_board_id_changes() {
        let mut gs = GameState::new(1);
        gs.start();

        let s1 = gs.snapshot();
        let obs1 = build_observation(1, &s1, None);

        let mut s2 = s1;
        s2.board_id = s2.board_id.wrapping_add(1);
        let obs2 = build_observation(2, &s2, None);

        assert_ne!(obs1.state_hash, obs2.state_hash);
    }
}
