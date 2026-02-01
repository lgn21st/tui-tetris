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

        // Shift all rows above down by one using copy
        // Note: copy_within handles overlapping ranges safely
        for row in (1..=y).rev() {
            let src_start = (row - 1) * width;
            let dst_start = row * width;
            self.cells
                .copy_within(src_start..src_start + width, dst_start);
        }

        // Clear the top row
        let top_start = 0;
        let top_end = width;
        for cell in &mut self.cells[top_start..top_end] {
            *cell = None;
        }

        1
    }

    /// Clear all full rows and return the row indices that were cleared (sorted bottom to top)
    /// Uses a two-pointer algorithm with zero-allocation
    pub fn clear_full_rows(&mut self) -> ArrayVec<usize, 4> {
        let mut cleared_rows = ArrayVec::new();
        let width = BOARD_WIDTH as usize;
        let mut write_y = BOARD_HEIGHT as usize;

        // Scan from bottom to top
        for read_y in (0..BOARD_HEIGHT as usize).rev() {
            if self.is_row_full(read_y) {
                // This row is full, record it and skip
                cleared_rows.push(read_y);
            } else {
                // This row is not full, move it down to the write position
                write_y -= 1;
                if write_y != read_y {
                    // Copy row using copy_within (no allocation, handles overlap)
                    let src_start = read_y * width;
                    let dst_start = write_y * width;
                    self.cells
                        .copy_within(src_start..src_start + width, dst_start);
                }
            }
        }

        // Clear the remaining rows at the top
        for y in 0..write_y {
            let start = y * width;
            let end = start + width;
            for cell in &mut self.cells[start..end] {
                *cell = None;
            }
        }

        // Reverse to get bottom-to-top order
        cleared_rows.reverse();
        cleared_rows
    }

    /// Lock a piece onto the board at given position with given shape
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
            self.set(px, py, Some(kind));
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

    /// Create from a flat array for testing
    #[cfg(test)]
    pub fn from_flat(cells: [Cell; BOARD_SIZE]) -> Self {
        Self { cells }
    }

    /// Create from a 2D vector for testing (converts to flat array)
    #[cfg(test)]
    pub fn from_cells(cells_2d: Vec<Vec<Cell>>) -> Self {
        assert_eq!(cells_2d.len(), BOARD_HEIGHT as usize);
        assert!(cells_2d.iter().all(|row| row.len() == BOARD_WIDTH as usize));

        let mut flat = [None; BOARD_SIZE];
        for (y, row) in cells_2d.iter().enumerate() {
            for (x, cell) in row.iter().enumerate() {
                flat[y * BOARD_WIDTH as usize + x] = *cell;
            }
        }
        Self { cells: flat }
    }

    /// Convert to 2D vector for testing/display
    #[cfg(test)]
    pub fn to_cells(&self) -> Vec<Vec<Cell>> {
        let width = BOARD_WIDTH as usize;
        (0..BOARD_HEIGHT as usize)
            .map(|y| {
                let start = y * width;
                let end = start + width;
                self.cells[start..end].to_vec()
            })
            .collect()
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
    fn test_board_index_calculation() {
        assert_eq!(Board::index(0, 0), Some(0));
        assert_eq!(Board::index(9, 0), Some(9));
        assert_eq!(Board::index(0, 1), Some(10));
        assert_eq!(Board::index(9, 19), Some(199));
        assert_eq!(Board::index(-1, 0), None);
        assert_eq!(Board::index(10, 0), None);
        assert_eq!(Board::index(0, 20), None);
    }

    #[test]
    fn test_board_flat_array() {
        let mut board = Board::new();

        // Set some cells
        board.set(0, 0, Some(PieceKind::I));
        board.set(5, 10, Some(PieceKind::T));

        // Verify via get
        assert_eq!(board.get(0, 0), Some(Some(PieceKind::I)));
        assert_eq!(board.get(5, 10), Some(Some(PieceKind::T)));

        // Verify internal array
        assert_eq!(board.cells[0], Some(PieceKind::I));
        assert_eq!(board.cells[10 * 10 + 5], Some(PieceKind::T));
    }

    #[test]
    fn test_board_from_cells_roundtrip() {
        // Create board from 2D
        let mut cells_2d = vec![vec![None; 10]; 20];
        cells_2d[5][3] = Some(PieceKind::O);
        cells_2d[10][7] = Some(PieceKind::L);

        let board = Board::from_cells(cells_2d.clone());

        // Convert back to 2D
        let back_2d = board.to_cells();

        assert_eq!(cells_2d, back_2d);
    }
}
