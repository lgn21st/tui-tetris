//! TUI Tetris - High-performance terminal Tetris with AI control support
//!
//! Architecture:
//! - core: Pure game logic (board, pieces, rules, scoring)
//! - adapter: AI protocol handling (TCP, JSON)
//! - ui: Terminal rendering and input (ratatui, crossterm)

pub mod adapter;
pub mod core;
pub mod types;
