//! Terminal "game renderer" module.
//!
//! This is a small, game-oriented rendering layer for terminal gameplay.
//! It intentionally avoids ratatui widgets/layout and instead renders into a
//! simple framebuffer that can be flushed to a terminal backend.
//!
//! Goals:
//! - Keep `core` deterministic and testable
//! - Provide a rendering pipeline that feels closer to a game renderer
//! - Square minos: background-filled cells; width/height chosen from cell pixels

pub mod cell_metrics;
pub mod fb;
pub mod game_view;
pub mod render_throttle;
pub mod renderer;

pub use cell_metrics::{detect_cell_pixels, squarest_cell_size};
pub use fb::{Cell, CellStyle, FrameBuffer, Rgb};
pub use game_view::{
    AdapterStatusView, AnchorY, GameView, GameViewModel, HudOverlay, HudOverlayValue, Viewport,
};
pub use render_throttle::RenderThrottle;
pub use renderer::{TerminalRenderer, encode_diff_into, encode_full_into};
