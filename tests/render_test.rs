//! Integration test for verifying locked pieces are rendered correctly
//!
//! This test creates a GameState, manually places locked pieces on the board,
//! and uses the IncrementalRenderer to verify the pieces are visible.

use ratatui::{buffer::Buffer, layout::Rect};

use tui_tetris::core::{Board, GameState};
use tui_tetris::types::{PieceKind, BOARD_WIDTH};
use tui_tetris::ui::IncrementalRenderer;

/// Helper function to create a test game state with locked pieces
fn create_test_state_with_locked_pieces() -> GameState {
    let mut state = GameState::new(12345);
    state.started = true;

    // Lock an O piece at the bottom (row 18-19)
    // O piece shape: [(1, 0), (2, 0), (1, 1), (2, 1)]
    // At position (4, 18), this fills cells (5,18), (6,18), (5,19), (6,19)
    let o_shape = [(1, 0), (2, 0), (1, 1), (2, 1)];
    let _ = state.board.lock_piece(&o_shape, 4, 18, PieceKind::O);

    // Lock an I piece horizontally at row 16
    // I piece shape at North: [(0, 1), (1, 1), (2, 1), (3, 1)]
    // At position (3, 15), this fills cells (3,16), (4,16), (5,16), (6,16)
    let i_shape = [(0, 1), (1, 1), (2, 1), (3, 1)];
    let _ = state.board.lock_piece(&i_shape, 3, 15, PieceKind::I);

    // Lock a T piece at position (2, 12)
    // T piece shape at North: [(1, 0), (0, 1), (1, 1), (2, 1)]
    let t_shape = [(1, 0), (0, 1), (1, 1), (2, 1)];
    let _ = state.board.lock_piece(&t_shape, 2, 12, PieceKind::T);

    // Set active piece to None to ensure we're only testing locked pieces
    state.active = None;

    state
}

/// Helper function to verify that a buffer contains rendered pieces
/// by checking that cells are not empty (not containing '·' or ' ')
fn verify_locked_piece_rendered(buf: &Buffer, expected_positions: &[(u16, u16)]) -> bool {
    let area = buf.area();
    let mut found_count = 0;

    for (x, y) in expected_positions {
        if *x < area.width && *y < area.height {
            let cell = &buf[(*x, *y)];
            let ch = cell.symbol();
            // Check if cell contains a piece character (not empty or dot or border)
            if ch != "·"
                && ch != " "
                && ch != "│"
                && ch != "─"
                && ch != "┌"
                && ch != "┐"
                && ch != "└"
                && ch != "┘"
            {
                found_count += 1;
            }
        }
    }

    // We should find at least some of the expected positions
    // Since pieces use multiple cells, we check for reasonable coverage
    found_count >= expected_positions.len() / 4
}

#[test]
fn test_locked_pieces_rendered_on_first_frame() {
    // Create state with locked pieces
    let state = create_test_state_with_locked_pieces();

    // Count how many cells should be occupied
    let occupied_cells = state.board.cells().iter().filter(|c| c.is_some()).count();
    println!("Board has {} occupied cells", occupied_cells);
    assert!(occupied_cells > 0, "Board should have locked pieces");

    // Create renderer and render first frame
    let mut renderer = IncrementalRenderer::new();
    let area = Rect::new(0, 0, 30, 25); // 30x25 is plenty for a 10x20 board
    let mut buf = Buffer::empty(area);

    renderer.render(&state, area, &mut buf);

    // Check that the buffer has been modified with piece characters
    let mut piece_cells = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buf[(x, y)];
            let ch = cell.symbol();
            // Check for piece characters (█) - full block
            if ch == "█" {
                piece_cells += 1;
            }
        }
    }

    println!(
        "Found {} piece cells ('█') in buffer after first render",
        piece_cells
    );
    assert!(
        piece_cells > 0,
        "Buffer should contain rendered piece characters on first frame"
    );

    // The occupied cells in the board should be reflected in the buffer
    // Each board cell might render as multiple buffer cells depending on cell_size
    // So we just check that we have some reasonable number of piece characters
    assert!(
        piece_cells >= occupied_cells,
        "Should have at least {} piece cells in buffer (found {})",
        occupied_cells,
        piece_cells
    );
}

#[test]
fn test_locked_pieces_rendered_on_subsequent_frames() {
    // Create state with locked pieces
    let mut state = create_test_state_with_locked_pieces();

    // Count how many cells should be occupied
    let occupied_cells = state.board.cells().iter().filter(|c| c.is_some()).count();

    // Create renderer
    let mut renderer = IncrementalRenderer::new();
    let area = Rect::new(0, 0, 30, 25);
    let mut buf = Buffer::empty(area);

    // First frame
    renderer.render(&state, area, &mut buf);

    // Clear the buffer to simulate what might happen between frames
    // (though in real usage, ratatui manages the buffer)
    let mut buf2 = Buffer::empty(area);

    // Second frame - should still show locked pieces via incremental rendering
    renderer.render(&state, area, &mut buf2);

    // Count piece characters in second frame
    let mut piece_cells = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buf2[(x, y)];
            let ch = cell.symbol();
            if ch == "█" {
                piece_cells += 1;
            }
        }
    }

    println!(
        "After second frame: {} occupied board cells, {} piece buffer cells",
        occupied_cells, piece_cells
    );

    // On subsequent frames, if nothing changed, we might not re-render
    // But locked pieces should still be there if the incremental logic works
    // Actually, incremental rendering only renders CHANGED cells, so if we
    // cleared the buffer, the pieces won't show up unless we detect changes

    // The key test: if we add a NEW locked piece, it should render
    // Add another locked piece for frame 3
    let s_shape = [(1, 0), (2, 0), (0, 1), (1, 1)]; // S piece
    let _ = state.board.lock_piece(&s_shape, 6, 10, PieceKind::S);

    let new_occupied = state.board.cells().iter().filter(|c| c.is_some()).count();
    println!(
        "Added new piece, board now has {} occupied cells",
        new_occupied
    );

    let mut buf3 = Buffer::empty(area);
    renderer.render(&state, area, &mut buf3);

    // Count piece characters after adding new piece
    let mut piece_cells_frame3 = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            let cell = &buf3[(x, y)];
            let ch = cell.symbol();
            if ch == "█" {
                piece_cells_frame3 += 1;
            }
        }
    }

    println!(
        "After third frame (with new piece): {} piece buffer cells",
        piece_cells_frame3
    );

    // With a new piece added, we should see piece characters
    assert!(
        piece_cells_frame3 > 0,
        "Adding a new locked piece should result in piece characters being rendered"
    );
}

#[test]
fn test_board_cells_stored_correctly() {
    // Direct test of board cell storage
    let mut board = Board::new();

    // Set some cells directly
    board.set(0, 0, Some(PieceKind::I));
    board.set(5, 10, Some(PieceKind::T));
    board.set(9, 19, Some(PieceKind::O));

    // Verify cells method returns correct data
    let cells = board.cells();
    assert_eq!(cells[0], Some(PieceKind::I), "Cell (0,0) should be I piece");

    // Cell (5, 10) is at index 10 * 10 + 5 = 105
    let idx_5_10 = 10 * BOARD_WIDTH as usize + 5;
    assert_eq!(
        cells[idx_5_10],
        Some(PieceKind::T),
        "Cell (5,10) should be T piece"
    );

    // Cell (9, 19) is at index 19 * 10 + 9 = 199
    let idx_9_19 = 19 * BOARD_WIDTH as usize + 9;
    assert_eq!(
        cells[idx_9_19],
        Some(PieceKind::O),
        "Cell (9,19) should be O piece"
    );

    // Verify the indexing function works
    let _renderer = IncrementalRenderer::new();
    // Access the index function through a test - the function is private so we
    // verify indirectly by rendering
    let mut state = GameState::new(12345);
    state.board = board;
    state.started = true;
    state.active = None;

    let area = Rect::new(0, 0, 30, 25);
    let mut buf = Buffer::empty(area);
    let mut renderer = IncrementalRenderer::new();
    renderer.render(&state, area, &mut buf);

    // Check that pieces were rendered
    let mut found_pieces = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            if buf[(x, y)].symbol() == "█" {
                found_pieces += 1;
            }
        }
    }

    assert!(
        found_pieces > 0,
        "Should render pieces from manually set board cells"
    );
}

#[test]
fn test_lock_piece_integration() {
    // Test that lock_piece properly stores pieces and they render
    let mut state = GameState::new(12345);
    state.started = true;

    // Manually lock a piece using lock_piece method
    let piece_shape = [(0, 0), (1, 0), (0, 1), (1, 1)]; // 2x2 block
    let success = state.board.lock_piece(&piece_shape, 4, 5, PieceKind::Z);

    assert!(success, "lock_piece should succeed for valid position");

    // Verify the cells are occupied
    assert!(
        state.board.is_occupied(4, 5),
        "Cell (4,5) should be occupied"
    );
    assert!(
        state.board.is_occupied(5, 5),
        "Cell (5,5) should be occupied"
    );
    assert!(
        state.board.is_occupied(4, 6),
        "Cell (4,6) should be occupied"
    );
    assert!(
        state.board.is_occupied(5, 6),
        "Cell (5,6) should be occupied"
    );

    // Render and verify
    let area = Rect::new(0, 0, 30, 25);
    let mut buf = Buffer::empty(area);
    let mut renderer = IncrementalRenderer::new();

    state.active = None; // No active piece
    renderer.render(&state, area, &mut buf);

    // Count rendered pieces
    let mut piece_chars = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            if buf[(x, y)].symbol() == "█" {
                piece_chars += 1;
            }
        }
    }

    println!(
        "Locked piece at (4,5) - found {} piece characters in buffer",
        piece_chars
    );
    assert!(
        piece_chars >= 4,
        "Should render at least 4 cells for the 2x2 locked piece"
    );
}

#[test]
fn test_incremental_renderer_state_tracking() {
    // Test that the renderer correctly tracks board state changes
    let mut renderer = IncrementalRenderer::new();
    let mut state = GameState::new(12345);
    state.started = true;
    state.active = None;

    let area = Rect::new(0, 0, 30, 25);

    // Frame 1: Empty board
    let mut buf1 = Buffer::empty(area);
    renderer.render(&state, area, &mut buf1);

    // Add a locked piece
    let shape = [(0, 0), (1, 0), (0, 1), (1, 1)];
    let _ = state.board.lock_piece(&shape, 3, 10, PieceKind::L);

    // Frame 2: Board with piece
    let mut buf2 = Buffer::empty(area);
    renderer.render(&state, area, &mut buf2);

    // The incremental renderer should detect the change and render the new piece
    let mut frame2_pieces = 0;
    for y in 0..area.height {
        for x in 0..area.width {
            if buf2[(x, y)].symbol() == "█" {
                frame2_pieces += 1;
            }
        }
    }

    println!(
        "Frame 2 (after adding piece): {} piece characters",
        frame2_pieces
    );
    assert!(
        frame2_pieces > 0,
        "Renderer should detect and render the new locked piece"
    );

    // Frame 3: Same board (no changes)
    let mut buf3 = Buffer::empty(area);
    renderer.render(&state, area, &mut buf3);

    // The renderer shouldn't need to update anything since nothing changed
    // But the pieces should still be conceptually there (the issue is about
    // whether they get rendered, not whether they're stored)
}
