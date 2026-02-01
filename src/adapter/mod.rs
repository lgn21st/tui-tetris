//! Adapter module - AI protocol handling
//!
//! This module handles external AI control via TCP socket.
//! Implements the JSON line protocol compatible with swiftui-tetris.

pub mod protocol;

// Re-export protocol types
pub use protocol::*;
