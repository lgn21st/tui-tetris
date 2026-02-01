//! Pieces module tests - TDD for SRS rotation system

use tui_tetris::core::pieces::{get_shape, get_spawn_shape, try_rotate, SPAWN_POSITION};
use tui_tetris::types::{PieceKind, Rotation};

// ============== Shape Tests ==============

#[test]
fn test_i_piece_shapes() {
    let north = get_shape(PieceKind::I, Rotation::North);
    assert_eq!(north, [(0, 1), (1, 1), (2, 1), (3, 1)]);

    let east = get_shape(PieceKind::I, Rotation::East);
    assert_eq!(east, [(2, 0), (2, 1), (2, 2), (2, 3)]);

    let south = get_shape(PieceKind::I, Rotation::South);
    assert_eq!(south, [(0, 2), (1, 2), (2, 2), (3, 2)]);

    let west = get_shape(PieceKind::I, Rotation::West);
    assert_eq!(west, [(1, 0), (1, 1), (1, 2), (1, 3)]);
}

#[test]
fn test_o_piece_shapes() {
    // O piece is the same for all rotations
    let north = get_shape(PieceKind::O, Rotation::North);
    let east = get_shape(PieceKind::O, Rotation::East);
    let south = get_shape(PieceKind::O, Rotation::South);
    let west = get_shape(PieceKind::O, Rotation::West);

    assert_eq!(north, [(1, 0), (2, 0), (1, 1), (2, 1)]);
    assert_eq!(east, north);
    assert_eq!(south, north);
    assert_eq!(west, north);
}

#[test]
fn test_t_piece_shapes() {
    let north = get_shape(PieceKind::T, Rotation::North);
    assert_eq!(north, [(1, 0), (0, 1), (1, 1), (2, 1)]);

    let east = get_shape(PieceKind::T, Rotation::East);
    assert_eq!(east, [(1, 0), (1, 1), (2, 1), (1, 2)]);

    let south = get_shape(PieceKind::T, Rotation::South);
    assert_eq!(south, [(0, 1), (1, 1), (2, 1), (1, 2)]);

    let west = get_shape(PieceKind::T, Rotation::West);
    assert_eq!(west, [(1, 0), (0, 1), (1, 1), (1, 2)]);
}

#[test]
fn test_s_piece_shapes() {
    let north = get_shape(PieceKind::S, Rotation::North);
    assert_eq!(north, [(1, 0), (2, 0), (0, 1), (1, 1)]);

    let east = get_shape(PieceKind::S, Rotation::East);
    assert_eq!(east, [(1, 0), (1, 1), (2, 1), (2, 2)]);
}

#[test]
fn test_z_piece_shapes() {
    let north = get_shape(PieceKind::Z, Rotation::North);
    assert_eq!(north, [(0, 0), (1, 0), (1, 1), (2, 1)]);

    let east = get_shape(PieceKind::Z, Rotation::East);
    assert_eq!(east, [(2, 0), (1, 1), (2, 1), (1, 2)]);
}

#[test]
fn test_j_piece_shapes() {
    let north = get_shape(PieceKind::J, Rotation::North);
    assert_eq!(north, [(0, 0), (0, 1), (1, 1), (2, 1)]);

    let east = get_shape(PieceKind::J, Rotation::East);
    assert_eq!(east, [(1, 0), (2, 0), (1, 1), (1, 2)]);
}

#[test]
fn test_l_piece_shapes() {
    let north = get_shape(PieceKind::L, Rotation::North);
    assert_eq!(north, [(2, 0), (0, 1), (1, 1), (2, 1)]);

    let east = get_shape(PieceKind::L, Rotation::East);
    assert_eq!(east, [(1, 0), (1, 1), (1, 2), (2, 2)]);
}

#[test]
fn test_spawn_shape() {
    let i_spawn = get_spawn_shape(PieceKind::I);
    assert_eq!(i_spawn, get_shape(PieceKind::I, Rotation::North));

    let t_spawn = get_spawn_shape(PieceKind::T);
    assert_eq!(t_spawn, get_shape(PieceKind::T, Rotation::North));
}

#[test]
fn test_spawn_position() {
    assert_eq!(SPAWN_POSITION, (3, 0));
}

// ============== SRS Rotation Tests ==============

#[test]
fn test_t_rotation_success() {
    // Empty board - all positions valid
    let is_valid = |_x: i8, _y: i8| true;

    // T piece at spawn, rotate CW
    let result = try_rotate(PieceKind::T, Rotation::North, 3, 0, true, is_valid);
    assert!(result.is_some());

    let (shape, rotation, kick) = result.unwrap();
    assert_eq!(rotation, Rotation::East);
    assert_eq!(shape, get_shape(PieceKind::T, Rotation::East));
    // Should succeed without kick on empty board
    assert_eq!(kick, (0, 0));
}

#[test]
fn test_t_rotation_with_kick() {
    // Simulate a wall blocking the direct rotation position
    // T piece at (3,5), rotating to East would place blocks at:
    // - (4,5), (4,6), (5,6), (4,7)
    // Let's block x=4,y=6 which is part of the East rotation
    let is_valid = |x: i8, y: i8| {
        let blocked = x == 4 && y == 6; // Block one cell of East rotation
        x >= 0 && x <= 9 && y >= 0 && y <= 19 && !blocked
    };

    // T piece, rotate CW (needs kick because direct rotation is blocked)
    let result = try_rotate(PieceKind::T, Rotation::North, 3, 5, true, is_valid);
    assert!(result.is_some());

    let (_shape, rotation, kick) = result.unwrap();
    assert_eq!(rotation, Rotation::East);
    // Should have applied a non-zero kick to avoid blocked position
    assert_ne!(kick, (0, 0), "Expected a kick but got none");
}

#[test]
fn test_t_rotation_failure() {
    // No positions valid - completely blocked
    let is_valid = |_x: i8, _y: i8| false;

    let result = try_rotate(PieceKind::T, Rotation::North, 3, 0, true, is_valid);
    assert!(result.is_none());
}

#[test]
fn test_o_rotation_no_kick() {
    // O piece has no kicks
    let is_valid = |_x: i8, _y: i8| true;

    let result = try_rotate(PieceKind::O, Rotation::North, 3, 0, true, is_valid);
    assert!(result.is_some());

    let (_shape, rotation, kick) = result.unwrap();
    assert_eq!(rotation, Rotation::East);
    // O piece always uses (0,0) kick
    assert_eq!(kick, (0, 0));
}

#[test]
fn test_i_rotation_offsets() {
    // I piece has different kick table than JLSTZ
    let is_valid = |_x: i8, _y: i8| true;

    // I piece rotate CW from North
    let result = try_rotate(PieceKind::I, Rotation::North, 3, 0, true, is_valid);
    assert!(result.is_some());

    let (shape, rotation, _kick) = result.unwrap();
    assert_eq!(rotation, Rotation::East);
    assert_eq!(shape, get_shape(PieceKind::I, Rotation::East));
}

#[test]
fn test_ccw_rotation() {
    let is_valid = |_x: i8, _y: i8| true;

    // Rotate CCW
    let result = try_rotate(PieceKind::T, Rotation::North, 3, 0, false, is_valid);
    assert!(result.is_some());

    let (_shape, rotation, _kick) = result.unwrap();
    assert_eq!(rotation, Rotation::West);
}

#[test]
fn test_kick_table_consistency() {
    use tui_tetris::core::pieces::get_kick_table;

    // O piece kick table should be all zeros
    let o_kicks = get_kick_table(PieceKind::O);
    for kicks in o_kicks.iter() {
        for &(dx, dy) in kicks.iter() {
            assert_eq!(dx, 0);
            assert_eq!(dy, 0);
        }
    }

    // JLSTZ should have same kick table
    let j_kicks = get_kick_table(PieceKind::J);
    let l_kicks = get_kick_table(PieceKind::L);
    let s_kicks = get_kick_table(PieceKind::S);
    let t_kicks = get_kick_table(PieceKind::T);
    let z_kicks = get_kick_table(PieceKind::Z);

    assert_eq!(j_kicks, l_kicks);
    assert_eq!(j_kicks, s_kicks);
    assert_eq!(j_kicks, t_kicks);
    assert_eq!(j_kicks, z_kicks);

    // I piece should have different kicks
    let i_kicks = get_kick_table(PieceKind::I);
    assert_ne!(i_kicks, j_kicks);
}

// ============== Shape Consistency Tests ==============

#[test]
fn test_all_shapes_have_4_minos() {
    for kind in [
        PieceKind::I,
        PieceKind::O,
        PieceKind::T,
        PieceKind::S,
        PieceKind::Z,
        PieceKind::J,
        PieceKind::L,
    ] {
        for rotation in [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ] {
            let shape = get_shape(kind, rotation);
            assert_eq!(
                shape.len(),
                4,
                "{:?} {:?} should have 4 minos",
                kind,
                rotation
            );
        }
    }
}

#[test]
fn test_shape_bounds_reasonable() {
    // All shape coordinates should be within reasonable bounds (0-3 for most pieces)
    for kind in [
        PieceKind::I,
        PieceKind::O,
        PieceKind::T,
        PieceKind::S,
        PieceKind::Z,
        PieceKind::J,
        PieceKind::L,
    ] {
        for rotation in [
            Rotation::North,
            Rotation::East,
            Rotation::South,
            Rotation::West,
        ] {
            let shape = get_shape(kind, rotation);
            for (x, y) in shape.iter() {
                assert!(*x >= 0 && *x <= 3, "Shape coordinate out of bounds");
                assert!(*y >= 0 && *y <= 3, "Shape coordinate out of bounds");
            }
        }
    }
}
