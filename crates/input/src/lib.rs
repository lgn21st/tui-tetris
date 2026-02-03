//! Terminal input module (engine-facing).
//!
//! This module is intentionally independent of any UI framework. It maps
//! `crossterm` key events into [`crate::types::GameAction`] and provides a
//! DAS/ARR input handler suitable for terminal environments (including terminals
//! without key-release events).

pub mod handler;
pub mod map;

pub use tui_tetris_types as types;

pub use handler::InputHandler;
pub use map::{handle_key_event, should_quit};
