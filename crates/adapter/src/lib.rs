//! Adapter module - AI control via TCP socket with JSON protocol
//!
//! This module enables external AI agents to control the game through a
//! TCP socket connection. The protocol is compatible with swiftui-tetris,
//! allowing the same AI clients to work with both implementations.
//!
//! # Protocol Overview
//!
//! The adapter implements a **line-delimited JSON protocol** over TCP:
//!
//! 1. **Connection**: Client connects to TCP socket (default: 127.0.0.1:7777)
//! 2. **Handshake**: Client sends `hello`, server responds with `welcome`
//! 3. **Controller Assignment**: First client to hello becomes the controller
//! 4. **Observation Streaming**: Server periodically sends game state observations
//! 5. **Commanding**: Controller sends commands to execute game actions
//!
//! # Message Types
//!
//! ## Client → Server
//!
//! - **hello**: Initial handshake with client info and requested capabilities
//! - **command**: Execute game actions or place piece at specific position
//! - **control**: Claim or release controller status
//!
//! ## Server → Client
//!
//! - **welcome**: Response to hello with server capabilities
//! - **observation**: Full game state snapshot (board, active piece, score, etc.)
//! - **ack**: Command acknowledgment
//! - **error**: Error response with code and message
//!
//! # Command Modes
//!
//! The adapter supports two command modes:
//!
//! - **action**: Send individual game actions (moveLeft, rotateCw, hardDrop, etc.)
//! - **place**: Send target position, server calculates actions to reach it
//!
//! # Environment Variables
//!
//! Configure the adapter using environment variables:
//!
//! - `TETRIS_AI_HOST`: Bind address (default: "127.0.0.1")
//! - `TETRIS_AI_PORT`: Port number (default: 7777)
//! - `TETRIS_AI_DISABLED`: Set to "1" or "true" to disable adapter entirely
//!
//! # Example Protocol Flow
//!
//! ```text
//! Client -> Server: {"type":"hello","seq":1,"ts":1234567890,"client":{"name":"my-ai","version":"1.0.0"},...}
//! Server -> Client: {"type":"welcome","seq":1,"ts":1234567890,"protocol_version":"2.0.0",...}
//! Server -> Client: {"type":"observation","seq":2,"ts":1234567891,"board":{...},"active":{...},...}
//! Client -> Server: {"type":"command","seq":2,"ts":1234567892,"mode":"action","actions":["moveLeft","rotateCw","hardDrop"]}
//! Server -> Client: {"type":"ack","seq":3,"ts":1234567892,"status":"ok"}
//! ```
//!
//! # Implementation
//!
//! - Uses **tokio** for async networking
//! - Multiple clients can connect (only one controller at a time)
//! - Controller can release control for another client to take over
//! - See [`protocol`] for message structure definitions
//! - See [`server`] for TCP server implementation
//!
//! # Testing
//!
//! Connect to the adapter using netcat for manual testing:
//!
//! ```bash
//! nc 127.0.0.1 7777
//! {"type":"hello","seq":1,"ts":1234567890,"client":{"name":"test","version":"1.0.0"},"protocol_version":"2.0.0","formats":["json"],"requested":{"stream_observations":true,"command_mode":"action"}}
//! ```

pub mod protocol;
pub mod runtime;
pub mod server;

pub use tui_tetris_core as core;
pub use tui_tetris_types as types;

// Re-export protocol types for convenience
pub use protocol::*;
pub use runtime::{Adapter, ClientCommand, InboundCommand, OutboundMessage};
pub use server::*;
