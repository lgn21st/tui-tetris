//! Core game logic module - pure, deterministic, and testable
//!
//! This module contains all the game rules, state management, and simulation logic.
//! It has **zero dependencies** on UI, networking, or I/O, making it:
//!
//! - **Deterministic**: Same seed produces identical games (for AI training)
//! - **Testable**: Comprehensive unit tests for all game rules
//! - **Portable**: Can run in any environment (terminal, GUI, headless)
//! - **Fast**: Zero-allocation hot paths for game tick processing
//!
//! # Module Structure
//!
//! - [`board`]: 10x20 game board with collision detection and line clearing
//! - [`game_state`]: Complete game state including active piece, scoring, timing
//! - [`pieces`]: Tetromino shape definitions and SRS rotation with wall kicks
//! - [`rng`]: 7-bag random piece generation for fair distribution
//! - [`scoring`]: Score calculation with T-spins, combos, and back-to-back bonuses
//!
//! # Game Rules
//!
//! This implementation follows modern Tetris guidelines:
//!
//! - **7-Bag Randomizer**: Pieces are drawn from a bag of 7, ensuring all piece types appear regularly
//! - **SRS Rotation**: Super Rotation System with wall kicks for all pieces except O
//! - **Lock Delay**: 450ms before a grounded piece locks, with 15 move/rotate reset limit
//! - **Ghost Piece**: Shows where the current piece will land
//! - **Hold**: Store one piece for later use (once per piece)
//! - **T-Spin Detection**: Mini and full T-spins based on corner occupancy
//! - **Scoring**: Classic Nintendo scoring with modern bonuses
//!
//! # Example
//!
//! ```
//! use tui_tetris_core::GameState;
//! use tui_tetris_types::GameAction;
//!
//! // Create and start a game
//! let mut game = GameState::new(12345);
//! game.start();
//!
//! // Apply game actions
//! game.apply_action(GameAction::MoveRight);
//! game.apply_action(GameAction::RotateCw);
//! game.apply_action(GameAction::HardDrop);
//!
//! // Check game state
//! assert!(game.score() > 0); // Hard drop awards points
//! ```
//!
//! # Timing
//!
//! The game uses a fixed timestep system:
//! - **Tick Rate**: 16ms (approximately 60 FPS)
//! - **Gravity**: Depends on level (1000ms at level 0, decreases with level)
//! - **Soft Drop**: 10x faster than normal gravity
//! - **Lock Delay**: 450ms when piece is grounded
//!
//! Call [`GameState::tick`](game_state::GameState::tick) every frame with elapsed time.

pub mod board;
pub mod game_state;
pub mod pieces;
pub mod rng;
pub mod scoring;
pub mod snapshot;

pub use tui_tetris_types as types;

// Re-export commonly used types for convenience
pub use board::Board;
pub use game_state::{GameState, Tetromino};
pub use pieces::{get_shape, try_rotate};
pub use rng::{PieceQueue, SimpleRng};
pub use scoring::{calculate_drop_score, calculate_score, ScoreResult};
pub use snapshot::{ActiveSnapshot, GameSnapshot};
