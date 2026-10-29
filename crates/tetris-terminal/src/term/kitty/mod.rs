//! Kitty graphics protocol backend for the game well.
//!
//! Ghostty 1.4 implements 100% of the published Kitty graphics protocol, so a
//! Ghostty user can render the playfield as pixels instead of coloured cells.
//! The same wire format works on kitty, WezTerm and Rio.
//!
//! Scope: one opaque image that exactly covers the 10x20 well interior. It is
//! retransmitted only when the rendered pixels change, reusing a stable image
//! id and placement id so the terminal replaces the old placement flicker-free.
//! Inside tmux every escape is wrapped in a `DCS tmux;` passthrough envelope and
//! `allow-passthrough` must be `on`.
//!
//! The cell framebuffer remains the source of truth and stays fully functional:
//! the image is composited above cell backgrounds, so if the terminal refuses
//! graphics the existing cell renderer is still visible underneath.

mod apc;
mod canvas;
mod capability;
mod well;

pub use apc::{BASE64_CHUNK, KittyEscapeWriter};
pub use canvas::{PixelCanvas, PixelFormat};
pub use capability::{GraphicsChoice, KittySupport, detect};
pub use well::{
    Grid, NEXT_SLOTS, PREVIEW_COLS, PreviewRenderer, WELL_BG, WellGeometry, WellRenderer,
    board_hash, motion_hash,
};

pub use crate::term::fb::Rgb;
