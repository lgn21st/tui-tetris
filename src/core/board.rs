//! Board module - manages the game grid
//!
//! The board is a 10x20 grid where each cell can be empty or filled with a piece kind.
//! Uses a flat array for better cache locality and zero-allocation.
//! Coordinates: (x, y) where x ranges 0..9 (left to right), y ranges 0..19 (top to bottom)
//! Spawn position for new pieces is at (3, 0)

use arrayvec::ArrayVec;

use crate::types::{Cell, PieceKind, BOARD_HEIGHT, BOARD_WIDTH};

/// Total number of cells on the board
const BOARD_SIZE: usize = (BOARD_WIDTH * BOARD_HEIGHT) as usize;

/// The game board - 10 columns x 20 rows using flat array storage
#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    /// Flat array of cells, row-major order (y * WIDTH + x)
    cells: [Cell; BOARD_SIZE],
}

impl Board {
    /// Create a new empty board
    pub fn new() -> Self {
        Self {
            cells: [None; BOARD_SIZE],
        }
    }

    /// Calculate flat index from (x, y) coordinates
    #[inline(always)]
    fn index(x: i8, y: i8) -> Option<usize> {
        if x < 0 || x >= BOARD_WIDTH as i8 || y < 0 || y >= BOARD_HEIGHT as i8 {
            return None;
        }
        Some((y as usize) * (BOARD_WIDTH as usize) + (x as usize))
    }

    /// Get width of the board
    pub fn width(&self) -> u8 {
        BOARD_WIDTH
    }

    /// Get height of the board
    pub fn height(&self) -> u8 {
        BOARD_HEIGHT
    }

    /// Get cell at position (x, y)
    /// Returns None if out of bounds
    pub fn get(&self, x: i8, y: i8) -> Option<Cell> {
        Self::index(x, y).map(|idx| self.cells[idx])
    }

    /// Set cell at position (x, y)
    /// Returns false if out of bounds
    pub fn set(&mut self, x: i8, y: i8, cell: Cell) -> bool {
        match Self::index(x, y) {
            Some(idx) => {
                self.cells[idx] = cell;
                true
            }
            None => false,
        }
    }

    /// Check if position is valid (within bounds and empty)
    pub fn is_valid(&self, x: i8, y: i8) -> bool {
        matches!(self.get(x, y), Some(None))
    }

    /// Check if position is occupied (within bounds and filled)
    pub fn is_occupied(&self, x: i8, y: i8) -> bool {
        matches!(self.get(x, y), Some(Some(_)))
    }

    /// Check if position is out of bounds
    pub fn is_out_of_bounds(&self, x: i8, y: i8) -> bool {
        x < 0 || x >= BOARD_WIDTH as i8 || y < 0 || y >= BOARD_HEIGHT as i8
    }

    /// Check if a row is completely filled
    pub fn is_row_full(&self, y: usize) -> bool {
        if y >= BOARD_HEIGHT as usize {
            return false;
        }
        let start = y * BOARD_WIDTH as usize;
        let end = start + BOARD_WIDTH as usize;
        self.cells[start..end].iter().all(|cell| cell.is_some())
    }

    /// Clear a row and shift all rows above down
    /// Uses copy_nonoverlapping for efficient memory movement
    /// Returns the number of lines cleared (1 or 0)
    pub fn clear_row(&mut self, y: usize) -> usize {
        if y >= BOARD_HEIGHT as usize {
            return 0;
        }

        let width = BOARD_WIDTH as usize;
        let start = y * width;

        // Safety: We're copying within the same array, and the ranges don't overlap
        // because we're copying from [0..start] to [width..start+width]
        unsafe {
            let src = self.cells.as_ptr();
            let dst = self.cells.as_mut_ptr().add(width);

            // Copy rows 0..y down by one row
            std::ptr::copy(src, dst, start);
        }

        // Clear the top row
        for i in 0..width {
            self.cells[i] = None;
        }

        1
    }

    /// Clear all full rows and return which rows were cleared
    /// Uses ArrayVec for zero-allocation (max 4 rows can be cleared at once)
    pub fn clear_full_rows(&mut self) -> ArrayVec<usize, 4> {
        let mut cleared = ArrayVec::new();

        // First pass: find all full rows from top to bottom
        // We need to clear from TOP (smallest y) to BOTTOM (largest y)
        // This way, when we clear a row, the rows above it shift down,
        // but we've already processed those rows, so their new positions don't affect us.
        let mut full_rows = ArrayVec::<usize, 4>::new();
        for y in 0..BOARD_HEIGHT as usize {
            if self.is_row_full(y) {
                full_rows.push(y);
            }
        }

        // Clear rows from top to bottom (ascending y)
        for &y in &full_rows {
            self.clear_row(y);
            cleared.push(y);
        }

        cleared
    }

    /// Lock a piece onto the board at position (x, y) with given shape and kind
    /// Returns true if successful, false if any cell is out of bounds or occupied
    pub fn lock_piece(&mut self, shape: &[(i8, i8)], x: i8, y: i8, kind: PieceKind) -> bool {
        // First check if all positions are valid
        for &(dx, dy) in shape {
            let px = x + dx;
            let py = y + dy;
            if !self.is_valid(px, py) {
                return false;
            }
        }

        // Then lock all cells
        for &(dx, dy) in shape {
            let px = x + dx;
            let py = y + dy;
            let success = self.set(px, py, Some(kind));
            if !success {
                return false;
            }
        }

        true
    }

    /// Check if spawn position is blocked (game over condition)
    pub fn is_spawn_blocked(&self) -> bool {
        // Check if the standard spawn area is blocked
        // For most pieces, this is a reasonable approximation
        !self.is_valid(3, 0) || !self.is_valid(4, 0) || !self.is_valid(5, 0)
    }

    /// Get a reference to the internal cells array
    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    /// Get a mutable reference to the internal cells array (for testing)
    #[cfg(test)]
    pub fn cells_mut(&mut self) -> &mut [Cell] {
        &mut self.cells
    }

    /// Clear the entire board
    pub fn clear(&mut self) {
        for cell in &mut self.cells {
            *cell = None;
        }
    }

    /// Count the number of filled cells on the board
    #[cfg(test)]
    pub fn filled_count(&self) -> usize {
        self.cells.iter().filter(|c| c.is_some()).count()
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_board_new() {
        let board = Board::new();
        assert_eq!(board.width(), BOARD_WIDTH);
        assert_eq!(board.height(), BOARD_HEIGHT);

        // All cells should be empty
        for y in 0..BOARD_HEIGHT {
            for x in 0..BOARD_WIDTH {
                assert!(board.is_valid(x as i8, y as i8));
            }
        }
    }

    #[test]
    fn test_board_set_and_get() {
        let mut board = Board::new();

        // Set a cell
        assert!(board.set(0, 0, Some(PieceKind::I)));
        assert_eq!(board.get(0, 0), Some(Some(PieceKind::I)));

        // Out of bounds
        assert!(!board.set(-1, 0, Some(PieceKind::I)));
        assert!(!board.set(10, 0, Some(PieceKind::I)));
        assert!(!board.set(0, -1, Some(PieceKind::I)));
        assert!(!board.set(0, 20, Some(PieceKind::I)));
    }

    #[test]
    fn test_board_is_valid() {
        let mut board = Board::new();

        // Empty cell should be valid
        assert!(board.is_valid(0, 0));

        // Occupied cell should not be valid
        board.set(0, 0, Some(PieceKind::I));
        assert!(!board.is_valid(0, 0));

        // Out of bounds should not be valid (is_valid returns false via matches)
        // Note: is_valid checks for Some(None), out of bounds returns None
        // So out of bounds returns false which is correct
    }

    #[test]
    fn test_board_is_occupied() {
        let mut board = Board::new();

        // Empty cell should not be occupied
        assert!(!board.is_occupied(0, 0));

        // Occupied cell should be occupied
        board.set(0, 0, Some(PieceKind::I));
        assert!(board.is_occupied(0, 0));
    }

    #[test]
    fn test_board_clear_full_rows() {
        let mut board = Board::new();

        // Fill a row
        for x in 0..BOARD_WIDTH {
            board.set(x as i8, 19, Some(PieceKind::I));
        }

        // Clear full rows
        let cleared = board.clear_full_rows();
        assert_eq!(cleared.len(), 1);
        assert_eq!(cleared[0], 19);

        // Row should now be empty
        for x in 0..BOARD_WIDTH {
            assert!(board.is_valid(x as i8, 19));
        }
    }

    #[test]
    fn test_board_lock_piece() {
        let mut board = Board::new();

        // Lock an O piece at (4, 0)
        let shape = &[(0, 0), (1, 0), (0, 1), (1, 1)];
        assert!(board.lock_piece(shape, 4, 0, PieceKind::O));

        // Check all 4 cells are occupied
        assert!(board.is_occupied(4, 0));
        assert!(board.is_occupied(5, 0));
        assert!(board.is_occupied(4, 1));
        assert!(board.is_occupied(5, 1));

        // Should fail to lock overlapping
        assert!(!board.lock_piece(shape, 4, 0, PieceKind::I));
    }

    #[test]
    fn test_board_clear() {
        let mut board = Board::new();

        // Fill some cells
        board.set(0, 0, Some(PieceKind::I));
        board.set(5, 10, Some(PieceKind::T));

        // Clear board
        board.clear();

        // All cells should be empty
        for y in 0..BOARD_HEIGHT {
            for x in 0..BOARD_WIDTH {
                assert!(board.is_valid(x as i8, y as i8));
            }
        }
    }

    #[test]
    fn test_board_index() {
        // Test index calculation
        assert_eq!(Board::index(0, 0), Some(0));
        assert_eq!(Board::index(9, 0), Some(9));
        assert_eq!(Board::index(0, 1), Some(10));
        assert_eq!(Board::index(9, 19), Some(199));

        // Out of bounds
        assert_eq!(Board::index(-1, 0), None);
        assert_eq!(Board::index(10, 0), None);
        assert_eq!(Board::index(0, -1), None);
        assert_eq!(Board::index(0, 20), None);
    }
}
