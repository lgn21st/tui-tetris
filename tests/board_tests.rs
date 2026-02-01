//! Board tests - TDD for Board module

use tui_tetris::core::Board;
use tui_tetris::types::{PieceKind, BOARD_HEIGHT, BOARD_WIDTH};

#[test]
fn test_board_new_empty() {
    let board = Board::new();
    assert_eq!(board.width(), BOARD_WIDTH);
    assert_eq!(board.height(), BOARD_HEIGHT);

    // All cells should be empty
    for y in 0..BOARD_HEIGHT as i8 {
        for x in 0..BOARD_WIDTH as i8 {
            assert!(board.is_valid(x, y), "Cell ({}, {}) should be valid", x, y);
            assert_eq!(board.get(x, y), Some(None));
        }
    }
}

#[test]
fn test_board_get_out_of_bounds() {
    let board = Board::new();

    // Negative coordinates
    assert_eq!(board.get(-1, 0), None);
    assert_eq!(board.get(0, -1), None);

    // Beyond bounds
    assert_eq!(board.get(BOARD_WIDTH as i8, 0), None);
    assert_eq!(board.get(0, BOARD_HEIGHT as i8), None);
}

#[test]
fn test_board_set_and_get() {
    let mut board = Board::new();

    // Set a cell
    assert!(board.set(5, 10, Some(PieceKind::T)));
    assert_eq!(board.get(5, 10), Some(Some(PieceKind::T)));

    // Set another cell
    assert!(board.set(0, 0, Some(PieceKind::I)));
    assert_eq!(board.get(0, 0), Some(Some(PieceKind::I)));

    // Clear a cell
    assert!(board.set(5, 10, None));
    assert_eq!(board.get(5, 10), Some(None));
}

#[test]
fn test_board_set_out_of_bounds() {
    let mut board = Board::new();

    // Should return false for out of bounds
    assert!(!board.set(-1, 0, Some(PieceKind::T)));
    assert!(!board.set(0, -1, Some(PieceKind::T)));
    assert!(!board.set(BOARD_WIDTH as i8, 0, Some(PieceKind::T)));
    assert!(!board.set(0, BOARD_HEIGHT as i8, Some(PieceKind::T)));
}

#[test]
fn test_board_is_valid() {
    let mut board = Board::new();

    // Empty cell should be valid
    assert!(board.is_valid(5, 10));

    // Occupied cell should not be valid
    board.set(5, 10, Some(PieceKind::T));
    assert!(!board.is_valid(5, 10));

    // Out of bounds should not be valid
    assert!(!board.is_valid(-1, 0));
    assert!(!board.is_valid(0, -1));
    assert!(!board.is_valid(BOARD_WIDTH as i8, 0));
}

#[test]
fn test_board_is_occupied() {
    let mut board = Board::new();

    // Empty cell should not be occupied
    assert!(!board.is_occupied(5, 10));

    // Occupied cell
    board.set(5, 10, Some(PieceKind::T));
    assert!(board.is_occupied(5, 10));

    // Out of bounds should not be occupied
    assert!(!board.is_occupied(-1, 0));
}

#[test]
fn test_board_lock_piece_success() {
    let mut board = Board::new();

    // Define a simple 2x2 shape (like O piece)
    let shape = vec![(0, 0), (1, 0), (0, 1), (1, 1)];

    // Lock the piece at position (3, 5)
    assert!(board.lock_piece(&shape, 3, 5, PieceKind::O));

    // Verify all cells are locked
    assert_eq!(board.get(3, 5), Some(Some(PieceKind::O)));
    assert_eq!(board.get(4, 5), Some(Some(PieceKind::O)));
    assert_eq!(board.get(3, 6), Some(Some(PieceKind::O)));
    assert_eq!(board.get(4, 6), Some(Some(PieceKind::O)));
}

#[test]
fn test_board_lock_piece_collision() {
    let mut board = Board::new();

    // Pre-occupy a cell
    board.set(4, 5, Some(PieceKind::T));

    // Try to lock piece that overlaps
    let shape = vec![(0, 0), (1, 0), (0, 1), (1, 1)];
    assert!(!board.lock_piece(&shape, 3, 5, PieceKind::O));

    // Cells should not be modified
    assert_eq!(board.get(3, 5), Some(None));
    assert_eq!(board.get(4, 5), Some(Some(PieceKind::T)));
}

#[test]
fn test_board_lock_piece_out_of_bounds() {
    let mut board = Board::new();

    // Shape that would go out of bounds
    let shape = vec![(0, 0), (1, 0), (2, 0)];

    // Try to lock too close to right edge
    assert!(!board.lock_piece(&shape, 8, 5, PieceKind::I));
}

#[test]
fn test_board_is_row_full() {
    let mut board = Board::new();

    // Empty row is not full
    assert!(!board.is_row_full(5));

    // Fill the entire row 5
    for x in 0..BOARD_WIDTH {
        board.set(x as i8, 5, Some(PieceKind::T));
    }

    assert!(board.is_row_full(5));

    // Leave one cell empty in row 6
    for x in 0..BOARD_WIDTH - 1 {
        board.set(x as i8, 6, Some(PieceKind::I));
    }
    assert!(!board.is_row_full(6));
}

#[test]
fn test_board_clear_row() {
    let mut board = Board::new();

    // Fill row 5
    for x in 0..BOARD_WIDTH {
        board.set(x as i8, 5, Some(PieceKind::T));
    }

    // Put something above it
    board.set(0, 3, Some(PieceKind::I));
    board.set(1, 4, Some(PieceKind::O));

    // Clear row 5
    let cleared = board.clear_row(5);
    assert_eq!(cleared, 1);

    // What was at row 4 should now be at row 5 (shifted down)
    assert_eq!(board.get(1, 5), Some(Some(PieceKind::O)));
    // What was at row 3 should now be at row 4
    assert_eq!(board.get(0, 4), Some(Some(PieceKind::I)));

    // Row 3 should now be empty (shifted down and cleared at top)
    assert_eq!(board.get(0, 3), Some(None));
}

#[test]
fn test_board_clear_full_rows() {
    let mut board = Board::new();

    // Fill rows 18 and 19 (bottom two)
    for x in 0..BOARD_WIDTH {
        board.set(x as i8, 18, Some(PieceKind::I));
        board.set(x as i8, 19, Some(PieceKind::O));
    }

    // Put something at row 17
    board.set(0, 17, Some(PieceKind::T));

    // Clear full rows
    let cleared = board.clear_full_rows();
    assert_eq!(cleared.len(), 2);
    assert!(cleared.contains(&18));
    assert!(cleared.contains(&19));

    // The T piece should have dropped by 2 rows (rows 18 and 19 cleared)
    assert_eq!(board.get(0, 19), Some(Some(PieceKind::T)));

    // Rows 18 and 19 were cleared, but T moved to row 19
    // The bottom should have T, rows above should be shifted
}

#[test]
fn test_board_clear_multiple_rows_order() {
    let mut board = Board::new();

    // Fill rows 5, 10, and 15
    for x in 0..BOARD_WIDTH {
        board.set(x as i8, 5, Some(PieceKind::T));
        board.set(x as i8, 10, Some(PieceKind::I));
        board.set(x as i8, 15, Some(PieceKind::O));
    }

    // Put marker pieces above each
    board.set(0, 4, Some(PieceKind::J)); // Above row 5
    board.set(0, 9, Some(PieceKind::L)); // Above row 10
    board.set(0, 14, Some(PieceKind::S)); // Above row 15

    let cleared = board.clear_full_rows();
    assert_eq!(cleared.len(), 3);

    // After clearing rows 5, 10, 15 (3 rows total):
    // All non-full rows above drop down by the number of full rows below them
    // - J was at 4, drops by 3 to row 7
    assert_eq!(board.get(0, 7), Some(Some(PieceKind::J)));
    // - L was at 9, drops by 2 (rows 10 and 15 cleared below) to row 11
    assert_eq!(board.get(0, 11), Some(Some(PieceKind::L)));
    // - S was at 14, drops by 1 (row 15 cleared below) to row 15
    assert_eq!(board.get(0, 15), Some(Some(PieceKind::S)));
}

#[test]
fn test_board_clear() {
    let mut board = Board::new();

    // Fill some cells
    for x in 0..BOARD_WIDTH {
        board.set(x as i8, 5, Some(PieceKind::T));
    }

    // Clear the board
    board.clear();

    // All cells should be empty
    for y in 0..BOARD_HEIGHT as i8 {
        for x in 0..BOARD_WIDTH as i8 {
            assert_eq!(board.get(x, y), Some(None));
        }
    }
}

#[test]
fn test_board_spawn_blocked() {
    let mut board = Board::new();

    // Empty board - spawn should not be blocked
    assert!(!board.is_spawn_blocked());

    // Block the spawn area
    board.set(3, 0, Some(PieceKind::T));
    assert!(board.is_spawn_blocked());

    // Clear and try blocking other cells
    board.clear();
    board.set(4, 0, Some(PieceKind::T));
    assert!(board.is_spawn_blocked());
}

#[test]
fn test_board_cells_reference() {
    let board = Board::new();
    let cells = board.cells();

    assert_eq!(cells.len(), BOARD_HEIGHT as usize);
    assert!(cells.iter().all(|row| row.len() == BOARD_WIDTH as usize));
}
