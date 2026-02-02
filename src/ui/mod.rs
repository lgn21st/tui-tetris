//! UI module - Terminal rendering and input handling
//!
//! This module provides the terminal user interface for TUI Tetris,
//! built on top of [ratatui](https://github.com/ratatui-org/ratatui)
//! for rendering and [crossterm](https://github.com/crossterm-rs/crossterm)
//! for cross-platform input handling.
//!
//! # Module Structure
//!
//! - [`incremental`]: High-performance incremental renderer
//!   - Tracks state changes to minimize redraws
//!   - Aspect ratio correction for square-looking pieces
//!   - Ghost piece and border rendering
//!
//! - [`input`]: Basic keyboard input mapping
//!   - Maps key events to game actions
//!   - Supports arrow keys, WASD, and vim-style (hjkl) controls
//!   - Quit detection (Q or Ctrl+C)
//!
//! - [`input_handler`]: Advanced input with DAS/ARR
//!   - Delayed Auto Shift (DAS): Time before auto-repeat starts
//!   - Auto Repeat Rate (ARR): Interval between repeats
//!   - Works on terminals with or without key release event support
//!
//! - [`widgets`]: Ratatui widget implementations
//!   - [`BoardWidget`]: Renders the game board
//!   - Side panel with score, level, lines, hold, and next piece
//!   - Pause and game over overlays
//!
//! # Input Controls
//!
//! | Key | Action |
//! |-----|--------|
//! | ← or H or A | Move left |
//! | → or L or D | Move right |
//! | ↓ or J or S | Soft drop |
//! | ↑ or K or W | Rotate clockwise |
//! | Z or Y | Rotate counter-clockwise |
//! | Space | Hard drop |
//! | C | Hold piece |
//! | P | Pause/Resume |
//! | R | Restart (when game over) |
//! | Q | Quit |
//! | Ctrl+C | Quit |
//!
//! # Rendering
//!
//! The UI supports two rendering modes:
//!
//! - **Full Render**: Renders the entire board every frame (current default)
//!   - More reliable, ensures locked pieces are always visible
//!   - Handles all edge cases correctly
//!
//! - **Incremental Render**: Only redraws changed cells (planned optimization)
//!   - Tracks previous board state and only updates differences
//!   - Better performance for high-frequency updates
//!
//! # Example
//!
//! ```no_run
//! use tui_tetris::core::GameState;
//! use tui_tetris::ui::BoardWidget;
//! use ratatui::{Terminal, backend::CrosstermBackend};
//!
//! // Create terminal and game
//! let mut terminal = Terminal::new(CrosstermBackend::new(std::io::stdout())).unwrap();
//! let mut game = GameState::new(42);
//! game.start();
//!
//! // Render the board
//! terminal.draw(|f| {
//!     let widget = BoardWidget::new(&game);
//!     f.render_widget(widget, f.area());
//! }).unwrap();
//! ```
//!
//! # DAS/ARR Timing
//!
//! Default timing values (Tetris Guideline standard):
//! - **DAS**: 167ms (time before auto-repeat starts)
//! - **ARR**: 33ms (interval between auto-repeats)
//!
//! These can be configured via [`InputHandler::with_config`](input_handler::InputHandler::with_config).

pub mod incremental;
pub mod input;
pub mod input_handler;
pub mod widgets;

// Re-export commonly used items
pub use incremental::IncrementalRenderer;
pub use input::{handle_key_event, should_quit};
pub use input_handler::InputHandler;
pub use widgets::{
    piece_style, render_game_over_overlay, render_pause_overlay, render_side_panel, BoardWidget,
};
