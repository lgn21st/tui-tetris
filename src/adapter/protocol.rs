//! Protocol module - JSON message types for AI adapter
//!
//! Implements the line-delimited JSON protocol compatible with swiftui-tetris.
//! All messages have: type, seq (sequence number), ts (timestamp in ms)

use serde::{Deserialize, Serialize};

// ============== Client -> Game Messages ==============

/// Client hello message (first message to establish connection)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HelloMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub seq: u64,
    pub ts: u64,
    pub client: ClientInfo,
    pub protocol_version: String,
    pub formats: Vec<String>,
    pub requested: RequestedCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ClientInfo {
    pub name: String,
    pub version: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RequestedCapabilities {
    #[serde(rename = "stream_observations")]
    pub stream_observations: bool,
    #[serde(rename = "command_mode")]
    pub command_mode: String, // "action" or "place"
}

/// Command message (controller only)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CommandMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub seq: u64,
    pub ts: u64,
    pub mode: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub actions: Option<Vec<String>>, // For action mode
    #[serde(skip_serializing_if = "Option::is_none")]
    pub place: Option<PlaceCommand>, // For place mode
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PlaceCommand {
    pub x: i8,
    pub rotation: String,
    #[serde(rename = "useHold")]
    pub use_hold: bool,
}

/// Control message (claim/release controller status)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ControlMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub seq: u64,
    pub ts: u64,
    pub action: String, // "claim" or "release"
}

// ============== Game -> Client Messages ==============

/// Welcome message (response to hello)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WelcomeMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub seq: u64,
    pub ts: u64,
    pub protocol_version: String,
    pub game_id: String,
    pub capabilities: ServerCapabilities,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerCapabilities {
    pub formats: Vec<String>,
    #[serde(rename = "command_modes")]
    pub command_modes: Vec<String>,
    pub features: Vec<String>,
}

/// Acknowledgment for command receipt
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AckMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub seq: u64,
    pub ts: u64,
    pub status: String,
}

/// Error message
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ErrorMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub seq: u64,
    pub ts: u64,
    pub code: String,
    pub message: String,
}

/// Game state observation (sent to all clients)
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ObservationMessage {
    #[serde(rename = "type")]
    pub msg_type: String,
    pub seq: u64,
    pub ts: u64,
    pub playable: bool,
    pub paused: bool,
    #[serde(rename = "game_over")]
    pub game_over: bool,
    #[serde(rename = "episode_id")]
    pub episode_id: u32,
    pub seed: u32,
    #[serde(rename = "piece_id")]
    pub piece_id: u32,
    #[serde(rename = "step_in_piece")]
    pub step_in_piece: u32,
    pub board: BoardSnapshot,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub active: Option<ActivePieceSnapshot>,
    pub next: String, // Single next piece (for compatibility)
    #[serde(rename = "next_queue")]
    pub next_queue: Vec<String>, // Full next queue
    #[serde(skip_serializing_if = "Option::is_none")]
    pub hold: Option<String>,
    #[serde(rename = "can_hold")]
    pub can_hold: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    #[serde(rename = "last_event")]
    pub last_event: Option<LastEvent>,
    #[serde(rename = "state_hash")]
    pub state_hash: String,
    pub score: u32,
    pub level: u32,
    pub lines: u32,
    pub timers: TimersSnapshot,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoardSnapshot {
    pub width: u8,
    pub height: u8,
    pub cells: Vec<Vec<u8>>, // 0 = empty, 1-7 = piece kind
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ActivePieceSnapshot {
    pub kind: String,
    pub rotation: String,
    pub x: i8,
    pub y: i8,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LastEvent {
    pub locked: bool,
    #[serde(rename = "lines_cleared")]
    pub lines_cleared: u32,
    #[serde(rename = "line_clear_score")]
    pub line_clear_score: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub tspin: Option<String>,
    pub combo: u32,
    #[serde(rename = "back_to_back")]
    pub back_to_back: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TimersSnapshot {
    #[serde(rename = "drop_ms")]
    pub drop_ms: u32,
    #[serde(rename = "lock_ms")]
    pub lock_ms: u32,
    #[serde(rename = "line_clear_ms")]
    pub line_clear_ms: u32,
}

// ============== Message Parsing ==============

/// Parse a JSON message from a string
pub fn parse_message(json: &str) -> Result<ParsedMessage, serde_json::Error> {
    // Try to extract type field first
    let value: serde_json::Value = serde_json::from_str(json)?;
    let msg_type = value
        .get("type")
        .and_then(|v| v.as_str())
        .unwrap_or("unknown");

    match msg_type {
        "hello" => Ok(ParsedMessage::Hello(serde_json::from_value(value)?)),
        "command" => Ok(ParsedMessage::Command(serde_json::from_value(value)?)),
        "control" => Ok(ParsedMessage::Control(serde_json::from_value(value)?)),
        _ => Ok(ParsedMessage::Unknown(value)),
    }
}

/// Parsed incoming message
#[derive(Debug, Clone)]
pub enum ParsedMessage {
    Hello(HelloMessage),
    Command(CommandMessage),
    Control(ControlMessage),
    Unknown(serde_json::Value),
}

// ============== Utility Functions ==============

/// Create a hello message
pub fn create_hello(seq: u64, client_name: &str, protocol_version: &str) -> HelloMessage {
    HelloMessage {
        msg_type: "hello".to_string(),
        seq,
        ts: current_timestamp_ms(),
        client: ClientInfo {
            name: client_name.to_string(),
            version: env!("CARGO_PKG_VERSION").to_string(),
        },
        protocol_version: protocol_version.to_string(),
        formats: vec!["json".to_string()],
        requested: RequestedCapabilities {
            stream_observations: true,
            command_mode: "action".to_string(),
        },
    }
}

/// Create a welcome message
pub fn create_welcome(seq: u64, protocol_version: &str) -> WelcomeMessage {
    WelcomeMessage {
        msg_type: "welcome".to_string(),
        seq,
        ts: current_timestamp_ms(),
        protocol_version: protocol_version.to_string(),
        game_id: "tui-tetris".to_string(),
        capabilities: ServerCapabilities {
            formats: vec!["json".to_string()],
            command_modes: vec!["action".to_string(), "place".to_string()],
            features: vec![
                "hold".to_string(),
                "next".to_string(),
                "next_queue".to_string(),
                "can_hold".to_string(),
                "score".to_string(),
                "timers".to_string(),
            ],
        },
    }
}

/// Create an acknowledgment
pub fn create_ack(seq: u64, command_seq: u64) -> AckMessage {
    AckMessage {
        msg_type: "ack".to_string(),
        seq,
        ts: current_timestamp_ms(),
        status: "ok".to_string(),
    }
}

/// Create an error message
pub fn create_error(seq: u64, code: &str, message: &str) -> ErrorMessage {
    ErrorMessage {
        msg_type: "error".to_string(),
        seq,
        ts: current_timestamp_ms(),
        code: code.to_string(),
        message: message.to_string(),
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
    fn test_parse_hello() {
        let json = r#"{"type":"hello","seq":1,"ts":1234567890,"client":{"name":"test-ai","version":"1.0.0"},"protocol_version":"2.0.0","formats":["json"],"requested":{"stream_observations":true,"command_mode":"action"}}"#;

        let result = parse_message(json).unwrap();
        match result {
            ParsedMessage::Hello(msg) => {
                assert_eq!(msg.msg_type, "hello");
                assert_eq!(msg.seq, 1);
                assert_eq!(msg.client.name, "test-ai");
                assert_eq!(msg.protocol_version, "2.0.0");
            }
            _ => panic!("Expected Hello message"),
        }
    }

    #[test]
    fn test_parse_command_action() {
        let json = r#"{"type":"command","seq":2,"ts":1234567900,"mode":"action","actions":["moveLeft","rotateCw","hardDrop"]}"#;

        let result = parse_message(json).unwrap();
        match result {
            ParsedMessage::Command(msg) => {
                assert_eq!(msg.mode, "action");
                assert_eq!(msg.actions.unwrap().len(), 3);
            }
            _ => panic!("Expected Command message"),
        }
    }

    #[test]
    fn test_parse_control() {
        let json = r#"{"type":"control","seq":3,"ts":1234567910,"action":"claim"}"#;

        let result = parse_message(json).unwrap();
        match result {
            ParsedMessage::Control(msg) => {
                assert_eq!(msg.action, "claim");
            }
            _ => panic!("Expected Control message"),
        }
    }

    #[test]
    fn test_create_welcome() {
        let welcome = create_welcome(1, "2.0.0");
        assert_eq!(welcome.msg_type, "welcome");
        assert_eq!(welcome.seq, 1);
        assert_eq!(welcome.protocol_version, "2.0.0");
        assert_eq!(welcome.game_id, "tui-tetris");
    }

    #[test]
    fn test_create_error() {
        let error = create_error(5, "not_controller", "Only controller may send commands");
        assert_eq!(error.msg_type, "error");
        assert_eq!(error.code, "not_controller");
    }

    #[test]
    fn test_serde_roundtrip() {
        let ack = create_ack(10, 5);
        let json = serde_json::to_string(&ack).unwrap();
        let parsed: AckMessage = serde_json::from_str(&json).unwrap();
        assert_eq!(parsed.seq, ack.seq);
        assert_eq!(parsed.status, ack.status);
    }
}
