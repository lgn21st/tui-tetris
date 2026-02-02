//! Terminal "game renderer" module.
//!
//! This is a small, game-oriented rendering layer for terminal gameplay.
//! It intentionally avoids ratatui widgets/layout and instead renders into a
//! simple framebuffer that can be flushed to a terminal backend.
//!
//! Goals:
//! - Keep `core` deterministic and testable
//! - Provide a rendering pipeline that feels closer to a game renderer
//! - Allow precise control over aspect ratio (e.g. 2 chars wide per cell)

pub mod fb;
pub mod game_view;
pub mod renderer;

pub use fb::{Cell, CellStyle, FrameBuffer, Rgb};
pub use game_view::{GameView, Viewport};
pub use renderer::TerminalRenderer;
