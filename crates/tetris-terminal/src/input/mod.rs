//! Terminal input module (engine-facing).
//!
//! This module is intentionally independent of any UI framework. It maps
//! `crossterm` key events into [`tetris_core::types::GameAction`] and provides a
//! DAS/ARR input handler suitable for terminal environments (including terminals
//! without key-release events).

pub mod handler;
pub mod map;

pub use handler::InputHandler;
pub use map::{InputCommand, handle_key_event, map_input_command, should_quit};
