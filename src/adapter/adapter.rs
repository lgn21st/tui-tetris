//! Adapter integration module
//!
//! Provides a synchronous interface to the async TCP adapter.
//! This bridges the sync game loop with the async adapter.

use std::sync::mpsc::{channel, Receiver, Sender};
use std::sync::Arc;
use tokio::runtime::Runtime;
use tokio::sync::RwLock;

use crate::adapter::{server, ClientCommand, ServerConfig};
use crate::core::GameState;

/// Adapter handle for the game loop
pub struct Adapter {
    /// Runtime for async operations
    #[allow(dead_code)]
    runtime: Runtime,
    /// Command receiver - sync channel from adapter to game
    command_rx: Receiver<ClientCommand>,
    /// Observation sender - sends game state updates to adapter
    observation_tx: Sender<GameState>,
}

impl Adapter {
    /// Create and start the adapter
    pub fn new(game_state: GameState) -> Option<Self> {
        if server::ServerState::is_disabled() {
            println!("[Adapter] AI control disabled (TETRIS_AI_DISABLED)");
            return None;
        }
        
        // Create channels
        let (command_tx, command_rx) = channel::<ClientCommand>();
        let (observation_tx, observation_rx) = channel::<GameState>();
        
        // Create tokio runtime
        let runtime = Runtime::new().expect("Failed to create tokio runtime");
        
        let config = ServerConfig::from_env();
        let game_state = Arc::new(RwLock::new(game_state));
        
        // Spawn adapter task
        runtime.spawn(async move {
            adapter_task(config, game_state, command_tx, observation_rx).await;
        });
        
        Some(Self {
            runtime,
            command_rx,
            observation_tx,
        })
    }
    
    /// Poll for pending commands from AI clients
    pub fn poll_commands(&mut self) -> Vec<ClientCommand> {
        let mut commands = Vec::new();
        
        // Drain all available commands (non-blocking)
        while let Ok(cmd) = self.command_rx.try_recv() {
            commands.push(cmd);
        }
        
        commands
    }
    
    /// Send observation (game state) to all connected clients
    pub fn send_observation(&self, game_state: &GameState) {
        // Clone the game state and send
        let _ = self.observation_tx.send(game_state.clone());
    }
}

/// Background task that runs the TCP server
async fn adapter_task(
    config: ServerConfig,
    _game_state: Arc<RwLock<GameState>>,
    command_tx: Sender<ClientCommand>,
    mut observation_rx: Receiver<GameState>,
) {
    // Create channel for server to receive commands
    let (server_cmd_tx, mut server_cmd_rx) = tokio::sync::mpsc::channel(100);
    
    // Spawn command forwarding task
    let forward_task = tokio::spawn(async move {
        while let Some(cmd) = server_cmd_rx.recv().await {
            // Convert and send to game loop via sync channel
            if command_tx.send(cmd).is_err() {
                break;
            }
        }
    });
    
    // Spawn observation broadcasting task
    let _obs_task = tokio::spawn(async move {
        while let Ok(state) = observation_rx.recv() {
            // Build and broadcast observation to all clients
            // This would need access to the client list from server
            let _obs = server::build_observation(&state, 0);
            // TODO: Send to all connected clients
        }
    });
    
    // Run server
    let _ = server::run_server(config, _game_state, server_cmd_tx).await;
    
    // Cleanup
    let _ = forward_task.await;
}

#[cfg(test)]
mod tests {
    use super::*;
    
    #[test]
    fn test_adapter_disabled() {
        // Test that adapter returns None when disabled
        std::env::set_var("TETRIS_AI_DISABLED", "1");
        let adapter = Adapter::new(GameState::new(1));
        assert!(adapter.is_none());
        std::env::remove_var("TETRIS_AI_DISABLED");
    }
}
