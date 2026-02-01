//! Core module - pure game logic with no external dependencies
//!
//! This module contains all the game rules, state management, and logic.
//! It has zero dependencies on UI, networking, or I/O.

pub mod board;
pub mod pieces;

// Re-export commonly used types
pub use board::Board;
pub use pieces::{get_shape, try_rotate};
