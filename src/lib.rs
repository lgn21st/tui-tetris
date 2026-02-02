//! TUI Tetris - A high-performance terminal Tetris implementation with AI control support
//!
//! This crate implements a fully-featured Tetris game playable in the terminal,
//! designed with a clean architecture that separates game logic, rendering,
//! and external AI control.
//!
//! # Architecture
//!
//! The codebase is organized into three main modules:
//!
//! - [`core`]: Pure game logic with no external dependencies
//!   - Board state management, piece rotation, line clearing
//!   - Scoring, levels, timing mechanics
//!   - Deterministic and testable
//!
//! - [`adapter`]: AI protocol handling via TCP socket
//!   - JSON line protocol compatible with swiftui-tetris
//!   - Supports both "action" and "place" command modes
//!   - Allows external AI agents to control the game
//!
//! - [`ui`]: Terminal rendering and input handling
//!   - Uses ratatui for declarative TUI rendering
//!   - Uses crossterm for cross-platform input handling
//!   - Implements DAS/ARR for responsive controls
//!
//! - [`types`]: Core types and constants shared across modules
//!   - Game actions, piece kinds, rotation states
//!   - Board dimensions, timing constants
//!
//! # Quick Start
//!
//! ```no_run
//! use tui_tetris::core::GameState;
//!
//! // Create a new game with a specific seed
//! let mut game = GameState::new(42);
//!
//! // Start the game
//! game.start();
//!
//! // Game is now running with first piece spawned
//! assert!(game.active.is_some());
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
//! See [`adapter`] module documentation for protocol details.
//!
//! # Performance
//!
//! - Zero-allocation hot paths (tick, render)
//! - 16ms fixed timestep for consistent gameplay
//! - Diff-based rendering support (currently using full render for reliability)

pub mod adapter;
pub mod core;
pub mod types;
pub mod ui;
