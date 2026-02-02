//! TCP server for AI adapter
//!
//! Handles incoming connections and manages client lifecycle.
//! Uses tokio for async networking.

use std::net::SocketAddr;
use std::sync::Arc;
use tokio::net::{TcpListener, TcpStream};
use tokio::sync::{mpsc, Mutex, RwLock};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};

use crate::adapter::protocol::*;
use crate::core::GameState;
use crate::types::{GameAction, PieceKind, Rotation};

/// Server configuration
#[derive(Debug, Clone)]
pub struct ServerConfig {
    pub host: String,
    pub port: u16,
    pub protocol_version: String,
    pub max_pending_commands: usize,
}

impl Default for ServerConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            port: 7777,
            protocol_version: "2.0.0".to_string(),
            max_pending_commands: 10,
        }
    }
}

impl ServerConfig {
    /// Create from environment variables (matching swiftui-tetris)
    pub fn from_env() -> Self {
        use std::env;
        
        let host = env::var("TETRIS_AI_HOST")
            .unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = env::var("TETRIS_AI_PORT")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(7777);
        
        Self {
            host,
            port,
            protocol_version: "2.0.0".to_string(),
            max_pending_commands: 10,
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
    #[allow(dead_code)]
    game_state: Arc<RwLock<GameState>>,
    #[allow(dead_code)]
    observation_seq: Arc<Mutex<u64>>,
}

impl ServerState {
    pub fn new(config: ServerConfig, game_state: GameState) -> Self {
        Self {
            config,
            clients: Arc::new(RwLock::new(Vec::new())),
            controller: Arc::new(RwLock::new(None)),
            game_state: Arc::new(RwLock::new(game_state)),
            observation_seq: Arc::new(Mutex::new(0)),
        }
    }
    
    /// Check if AI is disabled via environment
    pub fn is_disabled() -> bool {
        std::env::var("TETRIS_AI_DISABLED")
            .map(|v| v == "1" || v.to_lowercase() == "true")
            .unwrap_or(false)
    }
}

/// Handle to a connected client
pub struct ClientHandle {
    pub id: usize,
    pub addr: SocketAddr,
    pub is_controller: bool,
    pub command_mode: String, // "action" or "place"
    pub tx: mpsc::UnboundedSender<String>, // Channel to send messages to client
}

/// Command from a client to be executed
#[derive(Debug, Clone)]
pub enum ClientCommand {
    Actions(Vec<GameAction>),
    Place { x: i8, rotation: Rotation, use_hold: bool },
}

/// Start the TCP server
pub async fn run_server(
    config: ServerConfig,
    game_state: Arc<RwLock<GameState>>,
    _command_rx: mpsc::Receiver<ClientCommand>,
) -> anyhow::Result<()> {
    if ServerState::is_disabled() {
        println!("[Adapter] AI control disabled via TETRIS_AI_DISABLED");
        // Just drain the command channel to prevent blocking
        loop {
            tokio::time::sleep(tokio::time::Duration::from_secs(1)).await;
        }
    }
    
    let addr = config.socket_addr();
    let listener = TcpListener::bind(&addr).await?;
    println!("[Adapter] TCP server listening on {}", addr);
    
    let state = Arc::new(ServerState::new(config, GameState::new(1)));
    let mut client_id_counter = 0usize;
    
    // Accept incoming connections
    loop {
        let (socket, addr) = listener.accept().await?;
        client_id_counter += 1;
        let client_id = client_id_counter;
        
        println!("[Adapter] Client {} connected from {}", client_id, addr);
        
        let state_clone = Arc::clone(&state);
        let game_state_clone = Arc::clone(&game_state);
        
        // Spawn task to handle this client
        tokio::spawn(async move {
            if let Err(e) = handle_client(socket, addr, client_id, state_clone, game_state_clone).await {
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
    game_state: Arc<RwLock<GameState>>,
) -> anyhow::Result<()> {
    let (reader, mut writer) = tokio::io::split(socket);
    let mut reader = BufReader::new(reader);
    
    // Channel to send messages to this client
    let (tx, mut rx) = mpsc::unbounded_channel::<String>();
    
    // Add client to list
    let client_handle = ClientHandle {
        id: client_id,
        addr,
        is_controller: false,
        command_mode: "action".to_string(),
        tx: tx.clone(),
    };
    
    {
        let mut clients = state.clients.write().await;
        clients.push(client_handle);
    }
    
    // Spawn task to write messages to client
    let write_task = tokio::spawn(async move {
        while let Some(msg) = rx.recv().await {
            if writer.write_all(msg.as_bytes()).await.is_err() {
                break;
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
    let mut seq_counter = 0u64;
    
    loop {
        line.clear();
        let bytes_read = reader.read_line(&mut line).await?;
        
        if bytes_read == 0 {
            // Client disconnected
            break;
        }
        
        let line = line.trim();
        if line.is_empty() {
            continue;
        }
        
        seq_counter += 1;
        
        // Parse the message
        match parse_message(line) {
            Ok(ParsedMessage::Hello(hello)) => {
                // Validate protocol version
                if !hello.protocol_version.starts_with("2.") {
                    let error = create_error(
                        seq_counter,
                        "protocol_mismatch",
                        &format!("Protocol version {} not supported", hello.protocol_version),
                    );
                    let json = serde_json::to_string(&error)?;
                    let _ = tx.send(json);
                    break;
                }
                
                _client_hello = Some(hello.clone());
                
                // Send welcome
                let welcome = create_welcome(seq_counter, &state.config.protocol_version);
                let json = serde_json::to_string(&welcome)?;
                let _ = tx.send(json);
                
                // First client to hello becomes controller
                let mut controller = state.controller.write().await;
                if controller.is_none() {
                    *controller = Some(client_id);
                    let mut clients = state.clients.write().await;
                    if let Some(client) = clients.iter_mut().find(|c| c.id == client_id) {
                        client.is_controller = true;
                        client.command_mode = hello.requested.command_mode.clone();
                    }
                    println!("[Adapter] Client {} is now controller", client_id);
                }
            }
            
            Ok(ParsedMessage::Command(cmd)) => {
                // Check if client is controller
                let is_controller = {
                    let clients = state.clients.read().await;
                    clients.iter().find(|c| c.id == client_id)
                        .map(|c| c.is_controller)
                        .unwrap_or(false)
                };
                
                if !is_controller {
                    let error = create_error(
                        seq_counter,
                        "not_controller",
                        "Only controller may send commands",
                    );
                    let json = serde_json::to_string(&error)?;
                    let _ = tx.send(json);
                    continue;
                }
                
                // Process command
                let game_state_guard = game_state.read().await;
                let _actions = process_command(&cmd, &game_state_guard);
                drop(game_state_guard);
                
                // Send acknowledgment
                let ack = create_ack(seq_counter, cmd.seq);
                let json = serde_json::to_string(&ack)?;
                let _ = tx.send(json);
            }
            
            Ok(ParsedMessage::Control(ctrl)) => {
                match ctrl.action.as_str() {
                    "claim" => {
                        let mut controller = state.controller.write().await;
                        if controller.is_none() {
                            *controller = Some(client_id);
                            let mut clients = state.clients.write().await;
                            if let Some(client) = clients.iter_mut().find(|c| c.id == client_id) {
                                client.is_controller = true;
                            }
                            let ack = create_ack(seq_counter, ctrl.seq);
                            let json = serde_json::to_string(&ack)?;
                            let _ = tx.send(json);
                        } else {
                            let error = create_error(
                                seq_counter,
                                "controller_active",
                                "Controller already assigned",
                            );
                            let json = serde_json::to_string(&error)?;
                            let _ = tx.send(json);
                        }
                    }
                    "release" => {
                        let mut controller = state.controller.write().await;
                        if *controller == Some(client_id) {
                            *controller = None;
                            let mut clients = state.clients.write().await;
                            if let Some(client) = clients.iter_mut().find(|c| c.id == client_id) {
                                client.is_controller = false;
                            }
                            let ack = create_ack(seq_counter, ctrl.seq);
                            let json = serde_json::to_string(&ack)?;
                            let _ = tx.send(json);
                        } else {
                            let error = create_error(
                                seq_counter,
                                "not_controller",
                                "Only controller may release",
                            );
                            let json = serde_json::to_string(&error)?;
                            let _ = tx.send(json);
                        }
                    }
                    _ => {
                        let error = create_error(
                            seq_counter,
                            "invalid_command",
                            &format!("Unknown control action: {}", ctrl.action),
                        );
                        let json = serde_json::to_string(&error)?;
                        let _ = tx.send(json);
                    }
                }
            }
            
            Err(e) => {
                let error = create_error(
                    seq_counter,
                    "invalid_command",
                    &format!("JSON parse error: {}", e),
                );
                let json = serde_json::to_string(&error)?;
                let _ = tx.send(json);
            }
            
            _ => {
                let error = create_error(
                    seq_counter,
                    "invalid_command",
                    "Unknown message type",
                );
                let json = serde_json::to_string(&error)?;
                let _ = tx.send(json);
            }
        }
    }
    
    // Clean up: remove client and release controller if needed
    {
        let mut controller = state.controller.write().await;
        if *controller == Some(client_id) {
            *controller = None;
            println!("[Adapter] Controller {} released", client_id);
        }
    }
    
    {
        let mut clients = state.clients.write().await;
        clients.retain(|c| c.id != client_id);
    }
    
    // Cancel write task
    drop(tx);
    let _ = write_task.await;
    
    Ok(())
}

/// Process a command and return game actions
fn process_command(cmd: &CommandMessage, _game_state: &GameState) -> Vec<GameAction> {
    let mut actions = Vec::new();
    
    match cmd.mode.as_str() {
        "action" => {
            if let Some(ref action_strings) = cmd.actions {
                for action_str in action_strings {
                    if let Some(action) = parse_action(action_str) {
                        actions.push(action);
                    }
                }
            }
        }
        "place" => {
            // TODO: Implement place mode - calculate actions to reach target
            // For now, just use the actions if provided
            if let Some(ref action_strings) = cmd.actions {
                for action_str in action_strings {
                    if let Some(action) = parse_action(action_str) {
                        actions.push(action);
                    }
                }
            }
        }
        _ => {}
    }
    
    actions
}

/// Parse action string to GameAction
fn parse_action(action: &str) -> Option<GameAction> {
    match action.to_lowercase().as_str() {
        "moveleft" => Some(GameAction::MoveLeft),
        "moveright" => Some(GameAction::MoveRight),
        "softdrop" => Some(GameAction::SoftDrop),
        "harddrop" => Some(GameAction::HardDrop),
        "rotatecw" => Some(GameAction::RotateCw),
        "rotateccw" => Some(GameAction::RotateCcw),
        "hold" => Some(GameAction::Hold),
        "pause" => Some(GameAction::Pause),
        "restart" => Some(GameAction::Restart),
        _ => None,
    }
}

/// Build observation message from game state
pub fn build_observation(
    game_state: &GameState,
    seq: u64,
) -> ObservationMessage {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};

    // Build board snapshot
    let cells: Vec<Vec<u8>> = (0..20)
        .map(|y| {
            (0..10)
                .map(|x| {
                    game_state.board.get(x, y)
                        .map(|cell| {
                            cell.map(|kind| match kind {
                                PieceKind::I => 1,
                                PieceKind::O => 2,
                                PieceKind::T => 3,
                                PieceKind::S => 4,
                                PieceKind::Z => 5,
                                PieceKind::J => 6,
                                PieceKind::L => 7,
                            }).unwrap_or(0)
                        })
                        .unwrap_or(0)
                })
                .collect()
        })
        .collect();

    // Build state hash
    let mut hasher = DefaultHasher::new();
    game_state.board.cells().hash(&mut hasher);
    if let Some(active) = game_state.active {
        active.hash(&mut hasher);
    }
    game_state.score.hash(&mut hasher);
    let state_hash = format!("{:x}", hasher.finish());

    // Build next queue
    let next_queue: Vec<String> = game_state.next_queue.iter()
        .take(5)
        .map(|kind| kind.as_str().to_lowercase())
        .collect();

    let next = next_queue.first()
        .cloned()
        .unwrap_or_default();

    // Build active piece
    let active = game_state.active.map(|piece| ActivePieceSnapshot {
        kind: piece.kind.as_str().to_lowercase(),
        rotation: match piece.rotation {
            Rotation::North => "north",
            Rotation::East => "east",
            Rotation::South => "south",
            Rotation::West => "west",
        }.to_string(),
        x: piece.x,
        y: piece.y,
    });

    // Build hold
    let hold = game_state.hold.map(|kind| kind.as_str().to_lowercase());

    ObservationMessage {
        msg_type: "observation".to_string(),
        seq,
        ts: current_timestamp_ms(),
        playable: !game_state.game_over && !game_state.paused,
        paused: game_state.paused,
        game_over: game_state.game_over,
        episode_id: 0, // TODO: Track episodes
        seed: 0, // TODO: Store seed in GameState
        piece_id: 0, // TODO: Track piece count
        step_in_piece: 0, // TODO: Track steps
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
        last_event: None, // TODO: Track last event
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
}
