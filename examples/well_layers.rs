//! Dump the well layers as raw pixels so rendering can be inspected offline.
//!
//! `cargo run --release --example well_layers -- /tmp` writes `board.rgb` and
//! `motion.rgba` for a representative mid-game frame; composite motion over
//! board to see exactly what the terminal draws.

use std::fs;

use tetris_core::core::{ActiveSnapshot, GameState};
use tetris_core::types::{PieceKind, Rotation};
use tetris_terminal::term::kitty::{KittySupport, WellGeometry, WellRenderer};

fn main() {
    let out_dir = std::env::args()
        .nth(1)
        .unwrap_or_else(|| "/tmp".to_string());
    // 1x display: 8x17 cells, the frame hugging a 10x20 grid of 16px minos.
    let geom = WellGeometry::new((20, 19), (160, 323));

    let mut snap = GameState::new(7).snapshot();
    for (y, row) in snap.board.iter_mut().enumerate() {
        for (x, cell) in row.iter_mut().enumerate() {
            let height = 19usize.saturating_sub(y / 2);
            *cell = if y + height / 2 >= 9 && (x + y) % 7 != 3 {
                (x % 7 + 1) as u8
            } else {
                0
            };
        }
    }
    snap.active = Some(ActiveSnapshot {
        kind: PieceKind::T,
        rotation: Rotation::North,
        x: 3,
        y: 4,
    });
    snap.ghost_y = Some(12);

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
