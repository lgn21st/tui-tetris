//! TUI Tetris (workspace facade crate).
//!
//! This package keeps the original `tui_tetris::{core,adapter,term,input,engine,types}` public
//! API stable while the implementation lives in dedicated crates under `crates/`.

pub use tui_tetris_adapter as adapter;
pub use tui_tetris_core as core;
pub use tui_tetris_engine as engine;
pub use tui_tetris_input as input;
pub use tui_tetris_term as term;
pub use tui_tetris_types as types;
