//! TCP server for AI adapter
//!
//! Handles incoming connections and manages client lifecycle.
//! Uses tokio for async networking.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::BufWriter;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, RwLock};
use tokio::time::{Duration, MissedTickBehavior};

use crate::adapter::protocol::*;
use crate::adapter::runtime::{
    AdapterStatus, ClientCommand, InboundCommand, InboundPayload, OutboundMessage,
};
use crate::types::{GameAction, Rotation};

pub use crate::adapter::observation::build_observation;
pub use crate::adapter::server_config::{check_tcp_listen_available, ServerConfig};

use arrayvec::ArrayVec;

const BACKPRESSURE_RETRY_AFTER_MS: u64 = 50;
pub const MAX_INBOUND_LINE_BYTES: usize = 64 * 1024;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum BoundedLineRead {
    Eof,
    Line,
    TooLong,
}

async fn read_bounded_line<R>(
    reader: &mut R,
    line: &mut Vec<u8>,
) -> std::io::Result<BoundedLineRead>
where
    R: AsyncBufRead + Unpin,
{
    line.clear();
    loop {
        let available = reader.fill_buf().await?;
        if available.is_empty() {
            return Ok(if line.is_empty() {
                BoundedLineRead::Eof
            } else {
                BoundedLineRead::Line
            });
        }

        let newline = available.iter().position(|byte| *byte == b'\n');
        let payload_len = newline.unwrap_or(available.len());
        if line.len().saturating_add(payload_len) > MAX_INBOUND_LINE_BYTES {
            return Ok(BoundedLineRead::TooLong);
        }

        let consumed = newline.map_or(available.len(), |index| index + 1);
        line.extend_from_slice(&available[..consumed]);
        reader.consume(consumed);
        if newline.is_some() {
            return Ok(BoundedLineRead::Line);
        }
    }
}

fn is_compatible_protocol_version(version: &str) -> bool {
    fn valid_identifiers(value: &str, reject_numeric_leading_zero: bool) -> bool {
        !value.is_empty()
            && value.split('.').all(|identifier| {
                !identifier.is_empty()
                    && identifier
                        .bytes()
                        .all(|byte| byte.is_ascii_alphanumeric() || byte == b'-')
                    && (!reject_numeric_leading_zero
                        || !identifier.bytes().all(|byte| byte.is_ascii_digit())
                        || identifier.len() == 1
                        || !identifier.starts_with('0'))
            })
    }

    let mut build_parts = version.split('+');
    let base = build_parts.next().unwrap_or_default();
    if let Some(build) = build_parts.next() {
        if !valid_identifiers(build, false) || build_parts.next().is_some() {
            return false;
        }
    }

    let core = match base.split_once('-') {
        Some((core, prerelease)) if valid_identifiers(prerelease, true) => core,
        Some(_) => return false,
        None => base,
    };

    let mut components = core.split('.');
    let mut parse_numeric = || {
        let value = components.next()?;
        if value.is_empty()
            || !value.bytes().all(|byte| byte.is_ascii_digit())
            || (value.len() > 1 && value.starts_with('0'))
        {
            return None;
        }
        value.parse::<u64>().ok()
    };
    let Some(major) = parse_numeric() else {
        return false;
    };
    let Some(_minor) = parse_numeric() else {
        return false;
    };
    let Some(_patch) = parse_numeric() else {
        return false;
    };
    major == 2 && components.next().is_none()
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

async fn clear_stale_controller(state: &Arc<ServerState>, controller: &mut Option<usize>) {
    let clients = state.clients.read().await;
    clear_stale_controller_id(controller, |id| {
        clients.iter().any(|c| c.id == id && !c.tx.is_closed())
    });
}

fn sync_controller_flags(clients: &mut [ClientHandle], controller_id: Option<usize>) {
    for client in clients.iter_mut() {
        client.is_controller = controller_id.is_some_and(|id| client.id == id);
    }
}

fn send_client_error(
    tx: &mpsc::UnboundedSender<ClientOutbound>,
    seq: u64,
    code: ErrorCode,
    message: impl AsRef<str>,
) {
    let _ = tx.send(ClientOutbound::Error(create_error(
        seq,
        code,
        message.as_ref(),
    )));
}

fn encode_json_into_buf<T: serde::Serialize>(buf: &mut Vec<u8>, value: &T) -> bool {
    buf.clear();
    serde_json::to_writer(&mut *buf, value).is_ok()
}

fn log_wire_record(log_tx: Option<&mpsc::UnboundedSender<WireRecord>>, record: WireRecord) {
    if let Some(tx) = log_tx {
        let _ = tx.send(record);
    }
}

async fn write_json_and_log<W, T, F>(
    writer: &mut BufWriter<W>,
    buf: &mut Vec<u8>,
    value: T,
    log_tx: Option<&mpsc::UnboundedSender<WireRecord>>,
    wrap: F,
) -> std::io::Result<()>
where
    W: AsyncWrite + Unpin,
    T: serde::Serialize,
    F: FnOnce(T) -> WireRecord,
{
    if !encode_json_into_buf(buf, &value) {
        return Ok(());
    }
    writer.write_all(buf).await?;
    log_wire_record(log_tx, wrap(value));
    Ok(())
}

async fn enforce_strict_seq(
    state: &Arc<ServerState>,
    tx: &mpsc::UnboundedSender<ClientOutbound>,
    client_id: usize,
    seq: u64,
) -> bool {
    if !check_and_update_seq(state, client_id, seq).await {
        send_client_error(
            tx,
            seq,
            ErrorCode::InvalidCommand,
            "seq must be strictly increasing",
        );
        return false;
    }
    true
}

async fn enforce_handshake_and_seq(
    state: &Arc<ServerState>,
    tx: &mpsc::UnboundedSender<ClientOutbound>,
    client_id: usize,
    seq: u64,
    noun: &str,
) -> bool {
    if !is_handshaken(state, client_id).await {
        send_client_error(
            tx,
            seq,
            ErrorCode::HandshakeRequired,
            format!("Send hello before {}", noun),
        );
        return false;
    }
    enforce_strict_seq(state, tx, client_id, seq).await
}

/// Shared server state
pub struct ServerState {
    config: ServerConfig,
    clients: Arc<RwLock<Vec<ClientHandle>>>,
    controller: Arc<RwLock<Option<usize>>>, // Index into clients vec
    status_tx: Option<mpsc::UnboundedSender<AdapterStatus>>,
}

impl ServerState {
    pub fn new(
        config: ServerConfig,
        status_tx: Option<mpsc::UnboundedSender<AdapterStatus>>,
    ) -> Self {
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
        Some(prev) if seq <= prev => false,
        Some(_) => {
            client.last_seq = Some(seq);
            true
        }
    }
}

/// Handle to a connected client
pub struct ClientHandle {
    pub id: usize,
    pub addr: SocketAddr,
    pub is_controller: bool,
    pub requested_role: RequestedRole,
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

    let wire_log_tx: Option<mpsc::UnboundedSender<WireRecord>> =
        if let Some(path) = config.log_path.clone() {
            let log_every_n = config.log_every_n.max(1);
            let log_max_lines = config.log_max_lines;
            let (tx, mut rx) = mpsc::unbounded_channel::<WireRecord>();
            tokio::spawn(async move {
                use tokio::fs::OpenOptions;
                use tokio::io::AsyncWriteExt;

                let mut file = match OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&path)
                    .await
                {
                    Ok(f) => f,
                    Err(_) => return,
                };

                let mut buf: Vec<u8> = Vec::with_capacity(4096);
                let mut line_count: u64 = 0;
                let mut record_count: u64 = 0;

                while let Some(rec) = rx.recv().await {
                    record_count = record_count.wrapping_add(1);
                    if !record_count.is_multiple_of(log_every_n) {
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

    let addr = config.socket_addr()?;
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
            if let Err(e) = handle_client(
                socket,
                addr,
                client_id,
                state_clone,
                command_tx,
                wire_log_tx,
            )
            .await
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
        requested_role: RequestedRole::Auto,
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
                    log_wire_record(wire_log_tx_out.as_ref(), WireRecord::LineArc(line));
                }
                ClientOutbound::Ack(ack) => {
                    if write_json_and_log(
                        &mut writer,
                        &mut buf,
                        ack,
                        wire_log_tx_out.as_ref(),
                        WireRecord::Ack,
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                }
                ClientOutbound::Error(err) => {
                    if write_json_and_log(
                        &mut writer,
                        &mut buf,
                        err,
                        wire_log_tx_out.as_ref(),
                        WireRecord::Error,
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                }
                ClientOutbound::Welcome(welcome) => {
                    if write_json_and_log(
                        &mut writer,
                        &mut buf,
                        welcome,
                        wire_log_tx_out.as_ref(),
                        WireRecord::Welcome,
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                }
                ClientOutbound::Observation(obs) => {
                    if write_json_and_log(
                        &mut writer,
                        &mut buf,
                        obs,
                        wire_log_tx_out.as_ref(),
                        WireRecord::Observation,
                    )
                    .await
                    .is_err()
                    {
                        break;
                    }
                }
                ClientOutbound::ObservationArc(obs) => {
                    if !encode_json_into_buf(&mut buf, obs.as_ref()) {
                        continue;
                    }
                    if writer.write_all(&buf).await.is_err() {
                        break;
                    }
                    log_wire_record(wire_log_tx_out.as_ref(), WireRecord::ObservationArc(obs));
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
    let mut line = Vec::with_capacity(4096);

    loop {
        let read = match read_bounded_line(&mut reader, &mut line).await {
            Ok(read) => read,
            Err(_) => {
                // Treat I/O errors the same as a disconnect for lifecycle cleanup purposes.
                // Some clients may terminate abruptly, and we must still release/promote controller.
                break;
            }
        };

        match read {
            BoundedLineRead::Eof | BoundedLineRead::TooLong => break,
            BoundedLineRead::Line => {}
        }

        let Ok(line) = std::str::from_utf8(&line) else {
            break;
        };
        let raw_line = line.trim_end_matches(['\n', '\r']);
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
                    send_client_error(
                        &tx,
                        hello.seq,
                        ErrorCode::InvalidCommand,
                        "hello seq must be 1",
                    );
                    continue;
                }

                // Sequencing: enforce monotonic seq per sender.
                if is_handshaken(&state, client_id).await
                    && !enforce_strict_seq(&state, &tx, client_id, hello.seq).await
                {
                    continue;
                }

                // Validate protocol version
                if !is_compatible_protocol_version(&hello.protocol_version) {
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
                        client.requested_role = hello.requested.role.unwrap_or(RequestedRole::Auto);
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
                    clear_stale_controller(&state, &mut controller).await;

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
                    sync_controller_flags(&mut clients, controller_id);
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
                if !enforce_handshake_and_seq(&state, &tx, client_id, cmd.seq, "command").await {
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
                    send_client_error(
                        &tx,
                        cmd.seq,
                        ErrorCode::NotController,
                        "Only controller may send commands",
                    );
                    continue;
                }

                // Map command into an inbound command for the game loop.
                let mapped = match map_command(&cmd) {
                    Ok(c) => c,
                    Err((code, message)) => {
                        send_client_error(&tx, cmd.seq, code, message);
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
                        let _ = tx.send(ClientOutbound::Error(create_backpressure_error(
                            cmd.seq,
                            "Command queue is full",
                            BACKPRESSURE_RETRY_AFTER_MS,
                        )));
                    }
                }
            }

            Ok(ParsedMessage::Control(ctrl)) => match ctrl.action {
                ControlAction::Claim => {
                    if !enforce_handshake_and_seq(&state, &tx, client_id, ctrl.seq, "control").await
                    {
                        continue;
                    }

                    let mut should_emit_status = false;
                    {
                        let mut controller = state.controller.write().await;
                        // Clear stale controller_id (e.g. if the controller client crashed/disconnected).
                        clear_stale_controller(&state, &mut controller).await;
                        if *controller == Some(client_id) {
                            // Idempotent self-claim: already controller.
                            let mut clients = state.clients.write().await;
                            sync_controller_flags(&mut clients, Some(client_id));
                            let ack = create_ack(ctrl.seq, ctrl.seq);
                            let _ = tx.send(ClientOutbound::Ack(ack));
                            should_emit_status = true;
                        } else if controller.is_none() {
                            *controller = Some(client_id);
                            let mut clients = state.clients.write().await;
                            sync_controller_flags(&mut clients, Some(client_id));
                            let ack = create_ack(ctrl.seq, ctrl.seq);
                            let _ = tx.send(ClientOutbound::Ack(ack));
                            should_emit_status = true;
                        } else {
                            send_client_error(
                                &tx,
                                ctrl.seq,
                                ErrorCode::ControllerActive,
                                "Controller already assigned",
                            );
                        }
                    }
                    if should_emit_status {
                        emit_status(&state).await;
                    }
                }
                ControlAction::Release => {
                    if !enforce_handshake_and_seq(&state, &tx, client_id, ctrl.seq, "control").await
                    {
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
                            send_client_error(
                                &tx,
                                ctrl.seq,
                                ErrorCode::NotController,
                                "Only controller may release",
                            );
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
                if is_handshaken(&state, client_id).await
                    && !enforce_strict_seq(&state, &tx, client_id, seq).await
                {
                    continue;
                }
                send_client_error(&tx, seq, ErrorCode::InvalidCommand, "Unknown message type");
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
            let next_id = clients
                .iter()
                .filter(|c| !c.tx.is_closed() && c.requested_role != RequestedRole::Observer)
                .map(|c| c.id)
                .min();
            *controller = next_id;
            sync_controller_flags(&mut clients, next_id);
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
            let mut saw_restart = false;
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
                    ActionName::Restart => {
                        saw_restart = true;
                        GameAction::Restart
                    }
                };
                actions
                    .try_push(ga)
                    .map_err(|_| (ErrorCode::InvalidCommand, "Too many actions".to_string()))?;
            }

            let restart_seed = match cmd.restart.as_ref() {
                None => None,
                Some(r) => {
                    if !saw_restart {
                        return Err((
                            ErrorCode::InvalidCommand,
                            "restart field requires actions to include restart".to_string(),
                        ));
                    }
                    if r.seed > u32::MAX as u64 {
                        return Err((
                            ErrorCode::InvalidCommand,
                            "restart.seed out of range".to_string(),
                        ));
                    }
                    Some(r.seed as u32)
                }
            };

            Ok(ClientCommand::Actions {
                actions,
                restart_seed,
            })
        }
        CommandMode::Place => {
            if cmd.restart.is_some() {
                return Err((
                    ErrorCode::InvalidCommand,
                    "restart is only valid in action mode".to_string(),
                ));
            }
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

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
