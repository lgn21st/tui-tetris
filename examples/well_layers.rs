//! Dump the well layers as raw pixels so they can be inspected offline.
//!
//! Usage: `cargo run --release --example well_layers [out-dir]`
//! Writes `board.rgb` (opaque RGB) and `motion.rgba` next to each other; composite
//! motion over board to see exactly what the terminal will draw.

use std::fs;

use tetris_core::core::{ActiveSnapshot, GameState};
use tetris_core::types::{PieceKind, Rotation};
use tetris_terminal::term::kitty::{KittySupport, WellGeometry, WellRenderer};

fn main() {
    let out_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp".to_string());
    // Same geometry the app derives for an 8x17 terminal cell: the frame hugs a
    // 10x20 grid of square 16px minos, which needs only 19 text rows.
    let geom = WellGeometry::new(2, 1, (8, 17))
        .with_box_cells((20, 19))
        .with_layer_pixels(160, 323);
    let mut state = GameState::new(7);
    state.start();
    let mut snap = state.snapshot();

    // A realistic stack with a hole, plus a falling piece and its ghost.
    snap.board = [[0u8; 10]; 20];
    for (y, row) in snap.board.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            let height = 19usize.saturating_sub(y / 2);
            let filled = y + height / 2 >= 9 && (x + y) % 7 != 3;
            *cell = if filled { (x % 7 + 1) as u8 } else { 0 };
        }
    }
    snap.board[19][3] = 0;
    snap.board[19][4] = 0;
    snap.active = Some(ActiveSnapshot {
        kind: PieceKind::T,
        rotation: Rotation::North,
        x: 3,
        y: 4,
    });
    snap.ghost_y = Some(12);
    snap.timers.lock_ms = 0;

    let mut well = WellRenderer::new(
        KittySupport {
            tmux_passthrough: false,
        },
        geom,
    );
    let mut out = Vec::new();
    well.sync(&snap, geom, (0, 0), &mut out).unwrap();

    fs::write(format!("{out_dir}/board.rgb"), well.board().data()).unwrap();
    fs::write(format!("{out_dir}/motion.rgba"), well.motion().data()).unwrap();
    println!(
        "board {}x{} rgb, motion {}x{} rgba ({} wire bytes)",
        well.board().width(),
        well.board().height(),
        well.motion().width(),
        well.motion().height(),
        out.len()
    );
}
