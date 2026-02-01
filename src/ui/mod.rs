//! UI module - Terminal rendering and input handling

pub mod input;
pub mod widgets;

pub use input::{handle_key_event, should_quit};
pub use widgets::{
    piece_style, render_game_over_overlay, render_pause_overlay, render_side_panel, BoardWidget,
};
