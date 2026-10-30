//! Terminal "game renderer" module.
//!
//! This is a small, game-oriented rendering layer for terminal gameplay.
//! It intentionally avoids ratatui widgets/layout and instead renders into a
//! simple framebuffer that can be flushed to a terminal backend.
//!
//! Text (HUD, labels) goes through the framebuffer diff; the playfield and the
//! HUD previews are pixels drawn by `kitty`. Both consume the same immutable
//! snapshot and never touch game rules.

pub mod cell_metrics;
pub mod fb;
pub mod game_view;
pub mod kitty;
pub mod render_throttle;
pub mod renderer;

pub use cell_metrics::{detect_window_metrics, squarest_cell_size};
pub use fb::{Cell, CellStyle, FrameBuffer, Rgb};
pub use game_view::{
    AdapterStatusView, AnchorY, GameView, GameViewModel, HudOverlay, HudOverlayValue, PanelLayout,
    Viewport,
};
pub use render_throttle::RenderThrottle;
pub use renderer::{TerminalRenderer, encode_diff_into, encode_full_into};
