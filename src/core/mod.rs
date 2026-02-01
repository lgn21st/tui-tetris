//! Core module - pure game logic with no external dependencies
//!
//! This module contains all the game rules, state management, and logic.
//! It has zero dependencies on UI, networking, or I/O.

pub mod board;
pub mod game_state;
pub mod pieces;
pub mod rng;
pub mod scoring;

// Re-export commonly used types
pub use board::Board;
pub use game_state::{GameState, Tetromino};
pub use pieces::{get_shape, try_rotate};
pub use rng::{PieceQueue, SimpleRng};
pub use scoring::{calculate_drop_score, calculate_score, ScoreResult};
