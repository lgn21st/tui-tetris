//! Pieces module - Tetromino shapes and SRS rotation system
//!
//! Implements Standard Rotation System (SRS) with wall kick tables.
//! Reference: https://tetris.wiki/SRS

use crate::types::{PieceKind, Rotation};

/// Offset of a single mino relative to piece origin
pub type MinoOffset = (i8, i8);

/// Shape of a piece - 4 mino offsets from piece origin
pub type PieceShape = [MinoOffset; 4];

/// Get the shape (mino offsets) for a piece kind and rotation
pub fn get_shape(kind: PieceKind, rotation: Rotation) -> PieceShape {
    match kind {
        PieceKind::I => get_i_shape(rotation),
        PieceKind::O => get_o_shape(rotation),
        PieceKind::T => get_t_shape(rotation),
        PieceKind::S => get_s_shape(rotation),
        PieceKind::Z => get_z_shape(rotation),
        PieceKind::J => get_j_shape(rotation),
        PieceKind::L => get_l_shape(rotation),
    }
}

/// I piece shapes
fn get_i_shape(rotation: Rotation) -> PieceShape {
    match rotation {
        // N: horizontal, centered on row 1
        Rotation::North => [(0, 1), (1, 1), (2, 1), (3, 1)],
        // E: vertical, right-aligned
        Rotation::East => [(2, 0), (2, 1), (2, 2), (2, 3)],
        // S: horizontal, centered on row 2
        Rotation::South => [(0, 2), (1, 2), (2, 2), (3, 2)],
        // W: vertical, left-aligned
        Rotation::West => [(1, 0), (1, 1), (1, 2), (1, 3)],
    }
}

/// O piece shapes (same for all rotations)
fn get_o_shape(_rotation: Rotation) -> PieceShape {
    [(1, 0), (2, 0), (1, 1), (2, 1)]
}

/// T piece shapes
fn get_t_shape(rotation: Rotation) -> PieceShape {
    match rotation {
        Rotation::North => [(1, 0), (0, 1), (1, 1), (2, 1)],
        Rotation::East => [(1, 0), (1, 1), (2, 1), (1, 2)],
        Rotation::South => [(0, 1), (1, 1), (2, 1), (1, 2)],
        Rotation::West => [(1, 0), (0, 1), (1, 1), (1, 2)],
    }
}

/// S piece shapes
fn get_s_shape(rotation: Rotation) -> PieceShape {
    match rotation {
        Rotation::North => [(1, 0), (2, 0), (0, 1), (1, 1)],
        Rotation::East => [(1, 0), (1, 1), (2, 1), (2, 2)],
        Rotation::South => [(1, 1), (2, 1), (0, 2), (1, 2)],
        Rotation::West => [(0, 0), (0, 1), (1, 1), (1, 2)],
    }
}

/// Z piece shapes
fn get_z_shape(rotation: Rotation) -> PieceShape {
    match rotation {
        Rotation::North => [(0, 0), (1, 0), (1, 1), (2, 1)],
        Rotation::East => [(2, 0), (1, 1), (2, 1), (1, 2)],
        Rotation::South => [(0, 1), (1, 1), (1, 2), (2, 2)],
        Rotation::West => [(1, 0), (0, 1), (1, 1), (0, 2)],
    }
}

/// J piece shapes
fn get_j_shape(rotation: Rotation) -> PieceShape {
    match rotation {
        Rotation::North => [(0, 0), (0, 1), (1, 1), (2, 1)],
        Rotation::East => [(1, 0), (2, 0), (1, 1), (1, 2)],
        Rotation::South => [(0, 1), (1, 1), (2, 1), (2, 2)],
        Rotation::West => [(1, 0), (1, 1), (0, 2), (1, 2)],
    }
}

/// L piece shapes
fn get_l_shape(rotation: Rotation) -> PieceShape {
    match rotation {
        Rotation::North => [(2, 0), (0, 1), (1, 1), (2, 1)],
        Rotation::East => [(1, 0), (1, 1), (1, 2), (2, 2)],
        Rotation::South => [(0, 1), (1, 1), (2, 1), (0, 2)],
        Rotation::West => [(0, 0), (1, 0), (1, 1), (1, 2)],
    }
}

/// SRS wall kick data
/// Each entry is (dx, dy) offset to try when rotation fails
/// Order: 0=initial rotation, 1-4=wall kicks
pub type KickTable = [[(i8, i8); 5]; 8];

/// Get kick table for a piece kind
/// Returns table indexed by [from_rotation * 2 + to_rotation_index]
/// where to_rotation_index is 0 for CW, 1 for CCW
pub fn get_kick_table(kind: PieceKind) -> &'static KickTable {
    match kind {
        PieceKind::O => &O_KICKS,
        PieceKind::I => &I_KICKS,
        _ => &JLSTZ_KICKS,
    }
}

/// O piece has no kicks (always returns 0,0)
const O_KICKS: KickTable = [[(0, 0); 5]; 8];

/// JLSTZ kick table (shared by J, L, S, T, Z)
const JLSTZ_KICKS: KickTable = [
    // 0->1 (N->E, clockwise)
    [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
    // 0->3 (N->W, counter-clockwise)
    [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
    // 1->0 (E->N, counter-clockwise)
    [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
    // 1->2 (E->S, clockwise)
    [(0, 0), (1, 0), (1, -1), (0, 2), (1, 2)],
    // 2->1 (S->E, counter-clockwise)
    [(0, 0), (-1, 0), (-1, 1), (0, -2), (-1, -2)],
    // 2->3 (S->W, clockwise)
    [(0, 0), (1, 0), (1, 1), (0, -2), (1, -2)],
    // 3->2 (W->S, counter-clockwise)
    [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
    // 3->0 (W->N, clockwise)
    [(0, 0), (-1, 0), (-1, -1), (0, 2), (-1, 2)],
];

/// I piece kick table (different from JLSTZ)
const I_KICKS: KickTable = [
    // 0->1 (N->E)
    [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
    // 0->3 (N->W)
    [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
    // 1->0 (E->N)
    [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
    // 1->2 (E->S)
    [(0, 0), (-1, 0), (2, 0), (-1, 2), (2, -1)],
    // 2->1 (S->E)
    [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
    // 2->3 (S->W)
    [(0, 0), (2, 0), (-1, 0), (2, 1), (-1, -2)],
    // 3->2 (W->S)
    [(0, 0), (-2, 0), (1, 0), (-2, -1), (1, 2)],
    // 3->0 (W->N)
    [(0, 0), (1, 0), (-2, 0), (1, -2), (-2, 1)],
];

/// Get the kick index for a rotation transition
fn get_kick_index(from: Rotation, clockwise: bool) -> usize {
    match (from, clockwise) {
        (Rotation::North, true) => 0,  // N->E
        (Rotation::North, false) => 1, // N->W
        (Rotation::East, false) => 2,  // E->N
        (Rotation::East, true) => 3,   // E->S
        (Rotation::South, false) => 4, // S->E
        (Rotation::South, true) => 5,  // S->W
        (Rotation::West, false) => 6,  // W->S
        (Rotation::West, true) => 7,   // W->N
    }
}

/// Try to rotate a piece with wall kicks
/// Returns Some(new_shape, new_rotation, kick_offset) if successful, None if all kicks fail
pub fn try_rotate(
    kind: PieceKind,
    rotation: Rotation,
    x: i8,
    y: i8,
    clockwise: bool,
    is_valid: impl Fn(i8, i8) -> bool,
) -> Option<(PieceShape, Rotation, (i8, i8))> {
    let new_rotation = if clockwise {
        rotation.rotate_cw()
    } else {
        rotation.rotate_ccw()
    };

    let new_shape = get_shape(kind, new_rotation);
    let kick_table = get_kick_table(kind);
    let kick_index = get_kick_index(rotation, clockwise);
    let kicks = &kick_table[kick_index];

    // Try each kick offset
    for &(dx, dy) in kicks.iter() {
        let new_x = x + dx;
        let new_y = y + dy;

        // Check if all minos are valid at the kicked position
        let valid = new_shape
            .iter()
            .all(|&(mx, my)| is_valid(new_x + mx, new_y + my));

        if valid {
            return Some((new_shape, new_rotation, (dx, dy)));
        }
    }

    None
}

/// Spawn position for new pieces (x, y)
pub const SPAWN_POSITION: (i8, i8) = (3, 0);

/// Get initial shape for a new piece at spawn position
pub fn get_spawn_shape(kind: PieceKind) -> PieceShape {
    get_shape(kind, Rotation::North)
}
