//! TCP server for AI adapter
//!
//! Handles incoming connections and manages client lifecycle.
//! Uses tokio for async networking.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, RwLock};

use crate::adapter::protocol::*;
use crate::adapter::runtime::{ClientCommand, InboundCommand, InboundPayload, OutboundMessage};
use crate::core::GameState;
use crate::types::{GameAction, PieceKind, Rotation};

use arrayvec::ArrayVec;

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
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 7777,
            protocol_version: "2.0.0".to_string(),
            max_pending_commands: 10,
            log_path: None,
        }
    }
}

impl ServerConfig {
    /// Create from environment variables (matching swiftui-tetris)
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

        Self {
            host,
            port,
            protocol_version: "2.0.0".to_string(),
            max_pending_commands,
            log_path,
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
}

impl ServerState {
    pub fn new(config: ServerConfig) -> Self {
        Self {
            config,
            clients: Arc::new(RwLock::new(Vec::new())),
            controller: Arc::new(RwLock::new(None)),
        }
    }

    /// Check if AI is disabled via environment
    pub fn is_disabled() -> bool {
        std::env::var("TETRIS_AI_DISABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false)
    }
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
    pub command_mode: String, // "action" or "place"
    pub stream_observations: bool,
    pub handshaken: bool,
    pub last_seq: Option<u64>,
    pub tx: mpsc::UnboundedSender<ClientOutbound>, // Channel to send messages to client
}

#[derive(Debug, Clone)]
pub enum ClientOutbound {
    Line(String),
    Ack(AckMessage),
    Error(ErrorMessage),
    Welcome(WelcomeMessage),
    Observation(ObservationMessage),
}

#[derive(Debug, Clone)]
enum WireRecord {
    Bytes(Vec<u8>),
    Welcome(WelcomeMessage),
    Ack(AckMessage),
    Error(ErrorMessage),
    Observation(ObservationMessage),
}

/// Start the TCP server
pub async fn run_server(
    config: ServerConfig,
    command_tx: mpsc::Sender<InboundCommand>,
    mut out_rx: mpsc::UnboundedReceiver<OutboundMessage>,
    ready_tx: Option<oneshot::Sender<SocketAddr>>,
) -> anyhow::Result<()> {
    if ServerState::is_disabled() {
        println!("[Adapter] AI control disabled via TETRIS_AI_DISABLED");
        // Just drain the command channel to prevent blocking
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }

    let wire_log_tx: Option<mpsc::UnboundedSender<WireRecord>> = if let Some(path) = config.log_path.clone() {
        let (tx, mut rx) = mpsc::unbounded_channel::<WireRecord>();
        tokio::spawn(async move {
            use tokio::fs::OpenOptions;
            use tokio::io::AsyncWriteExt;

            let mut file = match OpenOptions::new().create(true).append(true).open(&path).await {
                Ok(f) => f,
                Err(_) => return,
            };

            let mut buf: Vec<u8> = Vec::with_capacity(4096);

            while let Some(rec) = rx.recv().await {
                match rec {
                    WireRecord::Bytes(b) => {
                        if file.write_all(&b).await.is_err() {
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
                }
                if file.write_all(b"\n").await.is_err() {
                    break;
                }
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
    println!("[Adapter] TCP server listening on {}", bound);
    if let Some(tx) = ready_tx {
        let _ = tx.send(bound);
    }

    let state = Arc::new(ServerState::new(config));
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
                            let _ = c.tx.send(ClientOutbound::Line(line));
                        }
                    }
                    OutboundMessage::Broadcast { line } => {
                        let clients = state.clients.read().await;
                        for c in clients.iter() {
                            if c.stream_observations {
                                let _ = c.tx.send(ClientOutbound::Line(line.clone()));
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
                        for c in clients.iter() {
                            if c.stream_observations {
                                let _ = c.tx.send(ClientOutbound::Observation(obs.clone()));
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

        println!("[Adapter] Client {} connected from {}", client_id, addr);

        let state_clone = Arc::clone(&state);
        let command_tx = command_tx.clone();
        let wire_log_tx = wire_log_tx.clone();

        // Spawn task to handle this client
        tokio::spawn(async move {
            if let Err(e) =
                handle_client(socket, addr, client_id, state_clone, command_tx, wire_log_tx).await
            {
                eprintln!("[Adapter] Client {} error: {}", client_id, e);
            }
            println!("[Adapter] Client {} disconnected", client_id);
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
    let (reader, mut writer) = tokio::io::split(socket);
    let mut reader = BufReader::new(reader);

    // Channel to send messages to this client
    let (tx, mut rx) = mpsc::unbounded_channel::<ClientOutbound>();

    // Add client to list
    let client_handle = ClientHandle {
        id: client_id,
        addr,
        is_controller: false,
        command_mode: "action".to_string(),
        stream_observations: false,
        handshaken: false,
        last_seq: None,
        tx: tx.clone(),
    };

    {
        let mut clients = state.clients.write().await;
        clients.push(client_handle);
    }

    let wire_log_tx_out = wire_log_tx.clone();

    // Spawn task to write messages to client
    let write_task = tokio::spawn(async move {
        let mut buf: Vec<u8> = Vec::with_capacity(4096);
        while let Some(msg) = rx.recv().await {
            match msg {
                ClientOutbound::Line(line) => {
                    let bytes = line.into_bytes();
                    if writer.write_all(&bytes).await.is_err() {
                        break;
                    }
                    if let Some(tx) = wire_log_tx_out.as_ref() {
                        let _ = tx.send(WireRecord::Bytes(bytes));
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
            }

            if writer.write_all(b"\n").await.is_err() {
                break;
            }
            if writer.flush().await.is_err() {
                break;
            }
        }
    });

    // Handle incoming messages
    let mut line = String::new();
    let mut _client_hello: Option<HelloMessage> = None;

    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;

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
            let _ = tx.send(WireRecord::Bytes(raw_line.as_bytes().to_vec()));
        }

        // Parse the message
        match parse_message(trimmed) {
            Ok(ParsedMessage::Hello(hello)) => {
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

                _client_hello = Some(hello.clone());

                // Mark client as handshaken.
                {
                    let mut clients = state.clients.write().await;
                    if let Some(client) = clients.iter_mut().find(|c| c.id == client_id) {
                        client.handshaken = true;
                        client.last_seq = Some(hello.seq);
                    }
                }

                // Send welcome
                let welcome = create_welcome(hello.seq, &state.config.protocol_version);
                let _ = tx.send(ClientOutbound::Welcome(welcome));

                // Request an immediate snapshot for this client if desired.
                if hello.requested.stream_observations {
                    let _ = command_tx.try_send(InboundCommand {
                        client_id,
                        seq: hello.seq,
                        payload: InboundPayload::SnapshotRequest,
                    });
                }

                // First client to hello becomes controller
                let mut controller = state.controller.write().await;
                if controller.is_none() {
                    *controller = Some(client_id);
                    let mut clients = state.clients.write().await;
                    if let Some(client) = clients.iter_mut().find(|c| c.id == client_id) {
                        client.is_controller = true;
                        client.command_mode = hello.requested.command_mode.clone();
                        client.stream_observations = hello.requested.stream_observations;
                    }
                    println!("[Adapter] Client {} is now controller", client_id);
                } else {
                    // Store capabilities for observers too.
                    let mut clients = state.clients.write().await;
                    if let Some(client) = clients.iter_mut().find(|c| c.id == client_id) {
                        client.command_mode = hello.requested.command_mode.clone();
                        client.stream_observations = hello.requested.stream_observations;
                    }
                }
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

            Ok(ParsedMessage::Control(ctrl)) => match ctrl.action.as_str() {
                "claim" => {
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

                    let mut controller = state.controller.write().await;
                    if controller.is_none() {
                        *controller = Some(client_id);
                        let mut clients = state.clients.write().await;
                        if let Some(client) = clients.iter_mut().find(|c| c.id == client_id) {
                            client.is_controller = true;
                        }
                        let ack = create_ack(ctrl.seq, ctrl.seq);
                        let _ = tx.send(ClientOutbound::Ack(ack));
                    } else {
                        let error = create_error(
                            ctrl.seq,
                            ErrorCode::ControllerActive,
                            "Controller already assigned",
                        );
                        let _ = tx.send(ClientOutbound::Error(error));
                    }
                }
                "release" => {
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

                    let mut controller = state.controller.write().await;
                    if *controller == Some(client_id) {
                        *controller = None;
                        let mut clients = state.clients.write().await;
                        if let Some(client) = clients.iter_mut().find(|c| c.id == client_id) {
                            client.is_controller = false;
                        }
                        let ack = create_ack(ctrl.seq, ctrl.seq);
                        let _ = tx.send(ClientOutbound::Ack(ack));
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
                _ => {
                    let error = create_error(
                        ctrl.seq,
                        ErrorCode::InvalidCommand,
                        &format!("Unknown control action: {}", ctrl.action),
                    );
                    let _ = tx.send(ClientOutbound::Error(error));
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

            Ok(ParsedMessage::Unknown(value)) => {
                let seq = value.get("seq").and_then(|v| v.as_u64()).unwrap_or(0);
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
            let next_id = clients.iter().map(|c| c.id).min();
            *controller = next_id;
            if let Some(new_id) = next_id {
                if let Some(c) = clients.iter_mut().find(|c| c.id == new_id) {
                    c.is_controller = true;
                }
                println!("[Adapter] Controller {} promoted", new_id);
            } else {
                println!("[Adapter] Controller {} released", client_id);
            }
        }
    }

    // Cancel write task
    drop(tx);
    let _ = write_task.await;

    Ok(())
}

/// Map a protocol command into an engine command.
fn map_command(cmd: &CommandMessage) -> Result<ClientCommand, (ErrorCode, String)> {
    match cmd.mode.as_str() {
        "action" => {
            let Some(ref action_strings) = cmd.actions else {
                return Err((ErrorCode::InvalidCommand, "Missing actions".to_string()));
            };
            let mut actions = ArrayVec::<GameAction, 32>::new();
            for a in action_strings {
                match parse_action(a) {
                    Some(act) => {
                        if actions.try_push(act).is_err() {
                            return Err((
                                ErrorCode::InvalidCommand,
                                "Too many actions".to_string(),
                            ));
                        }
                    }
                    None => {
                        return Err((ErrorCode::InvalidCommand, format!("Unknown action: {}", a)))
                    }
                }
            }
            Ok(ClientCommand::Actions(actions))
        }
        "place" => {
            let Some(ref place) = cmd.place else {
                return Err((ErrorCode::InvalidPlace, "Missing place".to_string()));
            };
            let rot_s = place.rotation.as_str();
            let rot = if rot_s.eq_ignore_ascii_case("north") {
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
            };
            Ok(ClientCommand::Place {
                x: place.x,
                rotation: rot,
                use_hold: place.use_hold,
            })
        }
        _ => Err((ErrorCode::InvalidCommand, format!("Unknown mode: {}", cmd.mode))),
    }
}

/// Parse action string to GameAction
fn parse_action(action: &str) -> Option<GameAction> {
    if action.eq_ignore_ascii_case("moveLeft") {
        Some(GameAction::MoveLeft)
    } else if action.eq_ignore_ascii_case("moveRight") {
        Some(GameAction::MoveRight)
    } else if action.eq_ignore_ascii_case("softDrop") {
        Some(GameAction::SoftDrop)
    } else if action.eq_ignore_ascii_case("hardDrop") {
        Some(GameAction::HardDrop)
    } else if action.eq_ignore_ascii_case("rotateCw") {
        Some(GameAction::RotateCw)
    } else if action.eq_ignore_ascii_case("rotateCcw") {
        Some(GameAction::RotateCcw)
    } else if action.eq_ignore_ascii_case("hold") {
        Some(GameAction::Hold)
    } else if action.eq_ignore_ascii_case("pause") {
        Some(GameAction::Pause)
    } else if action.eq_ignore_ascii_case("restart") {
        Some(GameAction::Restart)
    } else {
        None
    }
}

/// Build observation message from game state
pub fn build_observation(
    game_state: &GameState,
    seq: u64,
    episode_id: u32,
    piece_id: u32,
    step_in_piece: u32,
    last_event: Option<LastEvent>,
) -> ObservationMessage {
    use std::hash::{Hash, Hasher};

    // Build board snapshot (no heap)
    let mut cells = [[0u8; 10]; 20];
    for y in 0..20 {
        for x in 0..10 {
            let v = game_state
                .board
                .get(x as i8, y as i8)
                .and_then(|c| c)
                .map(|kind| match kind {
                    PieceKind::I => 1,
                    PieceKind::O => 2,
                    PieceKind::T => 3,
                    PieceKind::S => 4,
                    PieceKind::Z => 5,
                    PieceKind::J => 6,
                    PieceKind::L => 7,
                })
                .unwrap_or(0);
            cells[y][x] = v;
        }
    }

    // Build state hash
    let mut hasher = Fnv1aHasher::new();
    game_state.board.cells().hash(&mut hasher);
    if let Some(active) = game_state.active {
        active.hash(&mut hasher);
    }
    game_state.hold.hash(&mut hasher);
    game_state.can_hold.hash(&mut hasher);
    game_state.next_queue.iter().for_each(|k| k.hash(&mut hasher));
    game_state.paused.hash(&mut hasher);
    game_state.game_over.hash(&mut hasher);
    episode_id.hash(&mut hasher);
    piece_id.hash(&mut hasher);
    step_in_piece.hash(&mut hasher);
    game_state.piece_queue.seed().hash(&mut hasher);
    game_state.score.hash(&mut hasher);
    game_state.level.hash(&mut hasher);
    game_state.lines.hash(&mut hasher);
    game_state.drop_timer_ms.hash(&mut hasher);
    game_state.lock_timer_ms.hash(&mut hasher);
    game_state.line_clear_timer_ms.hash(&mut hasher);
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
        std::array::from_fn(|i| PieceKindLower::from(game_state.next_queue[i]));

    let next = next_queue[0];

    // Build active piece
    let active = game_state.active.map(|piece| ActivePieceSnapshot {
        kind: PieceKindLower::from(piece.kind),
        rotation: RotationLower::from(piece.rotation),
        x: piece.x,
        y: piece.y,
    });

    // Build hold
    let hold = game_state.hold.map(PieceKindLower::from);

    ObservationMessage {
        msg_type: ObservationType::Observation,
        seq,
        ts: current_timestamp_ms(),
        playable: !game_state.game_over && !game_state.paused,
        paused: game_state.paused,
        game_over: game_state.game_over,
        episode_id,
        seed: game_state.piece_queue.seed(),
        piece_id,
        step_in_piece,
        board: BoardSnapshot {
            width: 10,
            height: 20,
            cells,
        },
        active,
        next,
        next_queue,
        hold,
        can_hold: game_state.can_hold,
        last_event,
        state_hash,
        score: game_state.score,
        level: game_state.level,
        lines: game_state.lines,
        timers: TimersSnapshot {
            drop_ms: game_state.drop_timer_ms,
            lock_ms: game_state.lock_timer_ms,
            line_clear_ms: game_state.line_clear_timer_ms,
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

    #[test]
    fn test_parse_action() {
        assert_eq!(parse_action("moveLeft"), Some(GameAction::MoveLeft));
        assert_eq!(parse_action("rotateCw"), Some(GameAction::RotateCw));
        assert_eq!(parse_action("unknown"), None);
    }

    #[test]
    fn test_server_config_from_env() {
        // This test just ensures it doesn't panic
        let _config = ServerConfig::from_env();
    }

    #[test]
    fn test_state_hash_changes_when_meta_changes() {
        let mut gs = GameState::new(1);
        gs.start();

        let obs1 = build_observation(&gs, 1, 0, 1, 0, None);
        let obs2 = build_observation(&gs, 2, 1, 2, 3, None);
        assert_ne!(obs1.state_hash, obs2.state_hash);
    }

    #[test]
    fn test_state_hash_changes_when_hold_changes() {
        let mut gs = GameState::new(1);
        gs.start();

        let obs1 = build_observation(&gs, 1, gs.episode_id, gs.piece_id, gs.step_in_piece, None);
        assert!(gs.apply_action(GameAction::Hold));
        let obs2 = build_observation(&gs, 2, gs.episode_id, gs.piece_id, gs.step_in_piece, None);
        assert_ne!(obs1.state_hash, obs2.state_hash);
    }
}
