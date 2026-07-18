//! TCP server for AI adapter
//!
//! Handles incoming connections and manages client lifecycle.
//! Uses tokio for async networking.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::io::BufWriter;
use tokio::io::{AsyncBufRead, AsyncBufReadExt, AsyncWrite, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, oneshot, watch, RwLock};
use tokio::time::{Duration, MissedTickBehavior};

use crate::adapter::client_mailbox::{
    client_outbound_channel, ClientOutbound, ClientOutboundSender,
};
use crate::adapter::protocol::*;
use crate::adapter::runtime::{
    AdapterStatus, ClientCommand, ClientResponder, InboundCommand, InboundPayload, OutboundMessage,
};
use crate::adapter::wire_log::{spawn_wire_logger, try_log as log_wire_record, WireRecord};
use crate::types::{GameAction, Rotation};

pub use crate::adapter::client_mailbox::CLIENT_RELIABLE_QUEUE_CAPACITY;
pub use crate::adapter::observation::build_observation;
pub use crate::adapter::server_config::ServerConfig;
pub use crate::adapter::wire_log::WIRE_LOG_QUEUE_CAPACITY;

use arrayvec::ArrayVec;

const BACKPRESSURE_RETRY_AFTER_MS: u64 = 50;
pub const MAX_INBOUND_LINE_BYTES: usize = 64 * 1024;
const CLIENT_WRITER_SHUTDOWN_TIMEOUT: Duration = Duration::from_millis(100);

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
    major == 3 && components.next().is_none()
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

fn send_client_error(
    outbound: &ClientOutboundSender,
    seq: u64,
    code: ErrorCode,
    message: impl AsRef<str>,
) {
    outbound.try_send_reliable(ClientOutbound::Error(create_error(
        seq,
        code,
        message.as_ref(),
    )));
}

fn encode_json_into_buf<T: serde::Serialize>(buf: &mut Vec<u8>, value: &T) -> bool {
    buf.clear();
    serde_json::to_writer(&mut *buf, value).is_ok()
}

async fn write_json_and_log<W, T, F>(
    writer: &mut BufWriter<W>,
    buf: &mut Vec<u8>,
    value: T,
    log_tx: Option<&mpsc::Sender<WireRecord>>,
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
    outbound: &ClientOutboundSender,
    client_id: usize,
    seq: u64,
) -> bool {
    if !check_and_update_seq(state, client_id, seq).await {
        send_client_error(
            outbound,
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
    outbound: &ClientOutboundSender,
    client_id: usize,
    seq: u64,
    noun: &str,
) -> bool {
    if !is_handshaken(state, client_id).await {
        send_client_error(
            outbound,
            seq,
            ErrorCode::HandshakeRequired,
            format!("Send hello before {}", noun),
        );
        return false;
    }
    enforce_strict_seq(state, outbound, client_id, seq).await
}

/// Shared server state
pub struct ServerState {
    config: ServerConfig,
    broker: Arc<RwLock<BrokerState>>,
    status_tx: Option<watch::Sender<AdapterStatus>>,
}

#[derive(Default)]
struct BrokerState {
    clients: Vec<ClientHandle>,
    controller_id: Option<usize>,
}

impl BrokerState {
    fn is_controller(&self, client_id: usize) -> bool {
        self.controller_id == Some(client_id)
    }

    fn clear_stale_controller(&mut self) {
        clear_stale_controller_id(&mut self.controller_id, |id| {
            self.clients
                .iter()
                .any(|client| client.id == id && client.outbound.is_live())
        });
    }

    fn remove_and_promote(&mut self, client_id: usize) {
        let was_controller = self.is_controller(client_id);
        self.clients.retain(|client| client.id != client_id);
        if was_controller {
            self.controller_id = self
                .clients
                .iter()
                .filter(|client| {
                    client.outbound.is_live() && client.requested_role != RequestedRole::Observer
                })
                .map(|client| client.id)
                .min();
        }
    }
}

impl ServerState {
    pub fn new(config: ServerConfig, status_tx: Option<watch::Sender<AdapterStatus>>) -> Self {
        Self {
            config,
            broker: Arc::new(RwLock::new(BrokerState::default())),
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
    let broker = state.broker.read().await;
    let live_client_count = broker
        .clients
        .iter()
        .filter(|c| c.outbound.is_live())
        .count()
        .min(u16::MAX as usize) as u16;
    let controller_id = broker.controller_id.and_then(|id| {
        broker
            .clients
            .iter()
            .any(|c| c.id == id && c.outbound.is_live())
            .then_some(id)
    });
    let streaming_count = broker
        .clients
        .iter()
        .filter(|c| c.stream_observations && c.outbound.is_live())
        .count()
        .min(u16::MAX as usize) as u16;
    tx.send_replace(AdapterStatus {
        client_count: live_client_count,
        controller_id,
        streaming_count,
    });
}

async fn is_handshaken(state: &Arc<ServerState>, client_id: usize) -> bool {
    let broker = state.broker.read().await;
    broker
        .clients
        .iter()
        .find(|c| c.id == client_id)
        .map(|c| c.handshaken)
        .unwrap_or(false)
}

async fn check_and_update_seq(state: &Arc<ServerState>, client_id: usize, seq: u64) -> bool {
    let mut broker = state.broker.write().await;
    let Some(client) = broker.clients.iter_mut().find(|c| c.id == client_id) else {
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
    pub requested_role: RequestedRole,
    pub command_mode: CommandMode,
    pub stream_observations: bool,
    pub handshaken: bool,
    pub last_seq: Option<u64>,
    outbound: ClientOutboundSender,
}

enum StartupNotifier {
    Address(Option<oneshot::Sender<SocketAddr>>),
    Result(oneshot::Sender<Result<SocketAddr, String>>),
}

enum OutboundReceiverKind {
    Channel(mpsc::UnboundedReceiver<OutboundMessage>),
    Latest(watch::Receiver<Option<Arc<ObservationMessage>>>),
}

/// Adapter server observation source. Production uses a latest-only slot;
/// the channel conversion supports transport tests and carries the same single
/// coalescible observation type.
pub struct OutboundReceiver(OutboundReceiverKind);

impl From<mpsc::UnboundedReceiver<OutboundMessage>> for OutboundReceiver {
    fn from(value: mpsc::UnboundedReceiver<OutboundMessage>) -> Self {
        Self(OutboundReceiverKind::Channel(value))
    }
}

impl OutboundReceiver {
    fn latest(observations: watch::Receiver<Option<Arc<ObservationMessage>>>) -> Self {
        Self(OutboundReceiverKind::Latest(observations))
    }

    async fn recv(&mut self) -> Option<OutboundMessage> {
        match &mut self.0 {
            OutboundReceiverKind::Channel(receiver) => receiver.recv().await,
            OutboundReceiverKind::Latest(observations) => {
                if observations.changed().await.is_err() {
                    None
                } else {
                    observations
                        .borrow_and_update()
                        .clone()
                        .map(|obs| OutboundMessage::BroadcastObservationArc { obs })
                }
            }
        }
    }
}

impl StartupNotifier {
    fn success(self, address: SocketAddr) {
        match self {
            Self::Address(Some(tx)) => {
                let _ = tx.send(address);
            }
            Self::Address(None) => {}
            Self::Result(tx) => {
                let _ = tx.send(Ok(address));
            }
        }
    }

    fn failure(self, error: &anyhow::Error) {
        if let Self::Result(tx) = self {
            let _ = tx.send(Err(error.to_string()));
        }
    }
}

/// Start the TCP server.
///
/// The optional readiness channel is retained for callers that only need the
/// bound address. [`run_server_with_startup`] is used by [`crate::adapter::Adapter`]
/// when startup errors must be propagated synchronously.
pub async fn run_server<R>(
    config: ServerConfig,
    command_tx: mpsc::Sender<InboundCommand>,
    out_rx: R,
    ready_tx: Option<oneshot::Sender<SocketAddr>>,
    status_tx: Option<watch::Sender<AdapterStatus>>,
) -> anyhow::Result<()>
where
    R: Into<OutboundReceiver>,
{
    run_server_inner(
        config,
        command_tx,
        out_rx.into(),
        StartupNotifier::Address(ready_tx),
        status_tx,
    )
    .await
}

pub(crate) async fn run_server_with_startup(
    config: ServerConfig,
    command_tx: mpsc::Sender<InboundCommand>,
    observation_rx: watch::Receiver<Option<Arc<ObservationMessage>>>,
    startup_tx: oneshot::Sender<Result<SocketAddr, String>>,
    status_tx: Option<watch::Sender<AdapterStatus>>,
) -> anyhow::Result<()> {
    run_server_inner(
        config,
        command_tx,
        OutboundReceiver::latest(observation_rx),
        StartupNotifier::Result(startup_tx),
        status_tx,
    )
    .await
}

async fn run_server_inner(
    config: ServerConfig,
    command_tx: mpsc::Sender<InboundCommand>,
    mut out_rx: OutboundReceiver,
    startup: StartupNotifier,
    status_tx: Option<watch::Sender<AdapterStatus>>,
) -> anyhow::Result<()> {
    if ServerState::is_disabled() {
        let error = anyhow::anyhow!("AI adapter is disabled");
        startup.failure(&error);
        return Err(error);
    }

    let addr = match config.socket_addr() {
        Ok(address) => address,
        Err(error) => {
            startup.failure(&error);
            return Err(error);
        }
    };
    let listener = match TcpListener::bind(&addr).await {
        Ok(listener) => listener,
        Err(source) => {
            let error = anyhow::anyhow!("failed to bind AI adapter at {addr}: {source}");
            startup.failure(&error);
            return Err(error);
        }
    };
    let bound = match listener.local_addr() {
        Ok(address) => address,
        Err(source) => {
            let error = anyhow::anyhow!("failed to read AI adapter address: {source}");
            startup.failure(&error);
            return Err(error);
        }
    };
    let wire_log_tx = config
        .log_path
        .clone()
        .map(|path| spawn_wire_logger(path, config.log_every_n, config.log_max_lines));
    // Emit initial status (0 clients).
    if let Some(tx) = status_tx.as_ref() {
        tx.send_replace(AdapterStatus {
            client_count: 0,
            controller_id: None,
            streaming_count: 0,
        });
    }
    startup.success(bound);

    let state = Arc::new(ServerState::new(config, status_tx));
    let mut client_id_counter = 0usize;

    // Outbound dispatcher.
    {
        let state = Arc::clone(&state);
        tokio::spawn(async move {
            while let Some(msg) = out_rx.recv().await {
                match msg {
                    OutboundMessage::BroadcastObservationArc { obs } => {
                        let broker = state.broker.read().await;
                        let clients = &broker.clients;
                        for c in clients.iter() {
                            if c.stream_observations {
                                c.outbound
                                    .publish_observation(ClientOutbound::ObservationArc(
                                        Arc::clone(&obs),
                                    ));
                            }
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
    wire_log_tx: Option<mpsc::Sender<WireRecord>>,
) -> anyhow::Result<()> {
    let (reader, writer) = tokio::io::split(socket);
    let mut writer = BufWriter::with_capacity(16 * 1024, writer);
    let mut reader = BufReader::new(reader);

    let (outbound, mut reliable_rx, mut observation_rx, mut shutdown_rx) =
        client_outbound_channel(CLIENT_RELIABLE_QUEUE_CAPACITY);

    // Add client to list
    let client_handle = ClientHandle {
        id: client_id,
        addr,
        requested_role: RequestedRole::Auto,
        command_mode: CommandMode::Action,
        stream_observations: false,
        handshaken: false,
        last_seq: None,
        outbound: outbound.clone(),
    };

    {
        let mut broker = state.broker.write().await;
        broker.clients.push(client_handle);
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
                biased;
                msg = reliable_rx.recv() => msg,
                changed = observation_rx.changed() => {
                    if changed.is_err() {
                        None
                    } else {
                        observation_rx.borrow_and_update().clone()
                    }
                }
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
        let read_result = tokio::select! {
            read = read_bounded_line(&mut reader, &mut line) => Some(read),
            changed = shutdown_rx.changed() => {
                let _ = changed;
                None
            }
        };
        let Some(read_result) = read_result else {
            break;
        };
        let read = match read_result {
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

        log_wire_record(
            wire_log_tx.as_ref(),
            WireRecord::LineArc(Arc::from(raw_line)),
        );

        // Parse the message
        match parse_message(trimmed) {
            Ok(ParsedMessage::Hello(hello)) => {
                // Require hello to start the per-sender sequence at 1.
                if hello.seq != 1 {
                    send_client_error(
                        &outbound,
                        hello.seq,
                        ErrorCode::InvalidCommand,
                        "hello seq must be 1",
                    );
                    continue;
                }

                // Sequencing: enforce monotonic seq per sender.
                if is_handshaken(&state, client_id).await
                    && !enforce_strict_seq(&state, &outbound, client_id, hello.seq).await
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
                    outbound.try_send_reliable(ClientOutbound::Error(error));
                    break;
                }

                if !hello.formats.json {
                    let error = create_error(
                        hello.seq,
                        ErrorCode::InvalidCommand,
                        "formats must include json",
                    );
                    outbound.try_send_reliable(ClientOutbound::Error(error));
                    continue;
                }

                // Mark client as handshaken and store requested capabilities.
                {
                    let mut broker = state.broker.write().await;
                    if let Some(client) = broker.clients.iter_mut().find(|c| c.id == client_id) {
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
                    let mut broker = state.broker.write().await;
                    broker.clear_stale_controller();

                    let requested_role = hello.requested.role.unwrap_or(RequestedRole::Auto);
                    let allow_auto_controller = requested_role != RequestedRole::Observer;

                    if broker.controller_id == Some(client_id) {
                        assigned_role = AssignedRole::Controller;
                    } else if broker.controller_id.is_none() && allow_auto_controller {
                        broker.controller_id = Some(client_id);
                        assigned_role = AssignedRole::Controller;
                    }

                    broker.controller_id
                };

                // Send welcome (with deterministic role/controller fields).
                let welcome = create_welcome(
                    hello.seq,
                    &state.config.protocol_version,
                    client_id as u64,
                    assigned_role,
                    controller_id.map(|id| id as u64),
                );
                outbound.try_send_reliable(ClientOutbound::Welcome(welcome));

                // Request an immediate snapshot for this client if desired.
                if hello.requested.stream_observations {
                    // This is a required handshake consequence, not a best-effort
                    // gameplay command. Waiting here backpressures only this client
                    // task and preserves the bounded game-loop queue.
                    if command_tx
                        .send(InboundCommand {
                            client_id,
                            seq: hello.seq,
                            payload: InboundPayload::SnapshotRequest,
                            responder: ClientResponder::new(outbound.clone()),
                        })
                        .await
                        .is_err()
                    {
                        break;
                    }
                }

                emit_status(&state).await;
            }

            Ok(ParsedMessage::Command(cmd)) => {
                // Handshake required.
                if !enforce_handshake_and_seq(&state, &outbound, client_id, cmd.seq, "command")
                    .await
                {
                    continue;
                }

                // Check if client is controller
                let is_controller = {
                    let broker = state.broker.read().await;
                    broker.is_controller(client_id)
                };

                if !is_controller {
                    send_client_error(
                        &outbound,
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
                        send_client_error(&outbound, cmd.seq, code, message);
                        continue;
                    }
                };

                // Backpressure: bounded queue.
                match command_tx.try_send(InboundCommand {
                    client_id,
                    seq: cmd.seq,
                    payload: InboundPayload::Command(mapped),
                    responder: ClientResponder::new(outbound.clone()),
                }) {
                    Ok(()) => {
                        // Ack will be sent by the game loop after the command is applied.
                    }
                    Err(_) => {
                        outbound.try_send_reliable(ClientOutbound::Error(
                            create_backpressure_error(
                                cmd.seq,
                                "Command queue is full",
                                BACKPRESSURE_RETRY_AFTER_MS,
                            ),
                        ));
                    }
                }
            }

            Ok(ParsedMessage::Control(ctrl)) => match ctrl.action {
                ControlAction::Claim => {
                    if !enforce_handshake_and_seq(&state, &outbound, client_id, ctrl.seq, "control")
                        .await
                    {
                        continue;
                    }

                    let mut should_emit_status = false;
                    {
                        let mut broker = state.broker.write().await;
                        broker.clear_stale_controller();
                        if broker.is_controller(client_id) {
                            // Idempotent self-claim: already controller.
                            let ack = create_ack(ctrl.seq, ctrl.seq);
                            outbound.try_send_reliable(ClientOutbound::Ack(ack));
                            should_emit_status = true;
                        } else if broker.controller_id.is_none() {
                            broker.controller_id = Some(client_id);
                            let ack = create_ack(ctrl.seq, ctrl.seq);
                            outbound.try_send_reliable(ClientOutbound::Ack(ack));
                            should_emit_status = true;
                        } else {
                            send_client_error(
                                &outbound,
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
                    if !enforce_handshake_and_seq(&state, &outbound, client_id, ctrl.seq, "control")
                        .await
                    {
                        continue;
                    }

                    let mut should_emit_status = false;
                    {
                        let mut broker = state.broker.write().await;
                        if broker.is_controller(client_id) {
                            broker.controller_id = None;
                            let ack = create_ack(ctrl.seq, ctrl.seq);
                            outbound.try_send_reliable(ClientOutbound::Ack(ack));
                            should_emit_status = true;
                        } else {
                            send_client_error(
                                &outbound,
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
                outbound.try_send_reliable(ClientOutbound::Error(error));
            }

            Ok(ParsedMessage::Unknown(unknown)) => {
                let seq = unknown.seq;
                if is_handshaken(&state, client_id).await
                    && !enforce_strict_seq(&state, &outbound, client_id, seq).await
                {
                    continue;
                }
                send_client_error(
                    &outbound,
                    seq,
                    ErrorCode::InvalidCommand,
                    "Unknown message type",
                );
            }
        }
    }

    // Clean up: remove client and release/promote controller if needed.
    {
        let mut broker = state.broker.write().await;
        broker.remove_and_promote(client_id);
    }

    emit_status(&state).await;

    // Cancel write task
    drop(outbound);
    let mut write_task = write_task;
    if tokio::time::timeout(CLIENT_WRITER_SHUTDOWN_TIMEOUT, &mut write_task)
        .await
        .is_err()
    {
        write_task.abort();
        let _ = write_task.await;
    }

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
