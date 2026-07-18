//! Adapter server configuration and listen-address validation.

use std::net::SocketAddr;

use crate::adapter::protocol::PROTOCOL_VERSION;

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
            protocol_version: PROTOCOL_VERSION.to_string(),
            max_pending_commands: 10,
            log_path: None,
            log_every_n: 1,
            log_max_lines: None,
        }
    }
}

impl ServerConfig {
    /// Create from environment variables matching `docs/adapter-tui-tetris.md`.
    pub fn from_env() -> Self {
        let host = std::env::var("TETRIS_AI_HOST").unwrap_or_else(|_| "127.0.0.1".to_string());
        let port = std::env::var("TETRIS_AI_PORT")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(7777);
        let max_pending_commands = std::env::var("TETRIS_AI_MAX_PENDING")
            .ok()
            .and_then(|value| value.parse().ok())
            .unwrap_or(10);
        let log_path = std::env::var("TETRIS_AI_LOG_PATH")
            .ok()
            .map(|value| value.trim().to_string())
            .filter(|value| !value.is_empty());
        let log_every_n = std::env::var("TETRIS_AI_LOG_EVERY_N")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|&value| value >= 1)
            .unwrap_or(1);
        let log_max_lines = std::env::var("TETRIS_AI_LOG_MAX_LINES")
            .ok()
            .and_then(|value| value.parse::<u64>().ok())
            .filter(|&value| value >= 1);

        Self {
            host,
            port,
            protocol_version: PROTOCOL_VERSION.to_string(),
            max_pending_commands,
            log_path,
            log_every_n,
            log_max_lines,
        }
    }

    pub fn socket_addr(&self) -> anyhow::Result<SocketAddr> {
        format!("{}:{}", self.host, self.port)
            .parse()
            .map_err(|error| {
                anyhow::anyhow!(
                    "invalid adapter listen address {}:{}: {error}",
                    self.host,
                    self.port
                )
            })
    }
}
