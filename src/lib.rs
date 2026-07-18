//! TUI Tetris - A high-performance terminal Tetris implementation with AI control support
//!
//! This crate implements a fully-featured Tetris game playable in the terminal,
//! designed with a clean architecture that separates game logic, rendering,
//! and external AI control.
//!
//! Gameplay, session, adapter, and terminal APIs live in their dedicated
//! workspace crates. This root library owns only application commands, replay
//! commands, and the observer client.
//!
//! # Quick Start
//!
//! ```no_run
//! use tetris_core::core::GameState;
//!
//! // Create a new game with a specific seed
//! let mut game = GameState::new(42);
//!
//! // Start the game
//! game.start();
//!
//! // Game is now running with first piece spawned
//! assert!(game.active().is_some());
//! ```
//!
//! # AI Control
//!
//! To enable AI control, set environment variables before running:
//!
//! ```bash
//! export TETRIS_AI_HOST=127.0.0.1
//! export TETRIS_AI_PORT=7777
//! cargo run
//! ```
//!
//! Or disable AI entirely:
//!
//! ```bash
//! export TETRIS_AI_DISABLED=1
//! cargo run
//! ```
//!
//! See `docs/adapter.md` and the `tetris-adapter` crate for protocol details.
//!
//! # Performance
//!
//! - Zero-allocation hot paths (tick, render)
//! - 16ms fixed timestep for consistent gameplay
//! - Diff-based terminal rendering (dirty-cell flush)

pub mod app_cli;
pub mod observe;
pub mod replay_cli;
