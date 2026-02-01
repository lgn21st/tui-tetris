//! Board module - manages the game grid
//!
//! The board is a 10x20 grid where each cell can be empty or filled with a piece kind.
//! Coordinates: (x, y) where x ranges 0..9 (left to right), y ranges 0..19 (top to bottom)
//! Spawn position for new pieces is at (3, 0)

use arrayvec::ArrayVec;

use crate::types::{Cell, PieceKind, BOARD_HEIGHT, BOARD_WIDTH};

/// The game board - 10 columns x 20 rows
#[derive(Debug, Clone, PartialEq)]
pub struct Board {
    /// Grid of cells, indexed as [y][x] for row-major order
    cells: Vec<Vec<Cell>>,
}

impl Board {
    /// Create a new empty board
    pub fn new() -> Self {
        let cells = vec![vec![None; BOARD_WIDTH as usize]; BOARD_HEIGHT as usize];
        Self { cells }
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
        if x < 0 || x >= BOARD_WIDTH as i8 || y < 0 || y >= BOARD_HEIGHT as i8 {
            return None;
        }
        Some(self.cells[y as usize][x as usize])
    }

    /// Set cell at position (x, y)
    /// Returns false if out of bounds
    pub fn set(&mut self, x: i8, y: i8, cell: Cell) -> bool {
        if x < 0 || x >= BOARD_WIDTH as i8 || y < 0 || y >= BOARD_HEIGHT as i8 {
            return false;
        }
        self.cells[y as usize][x as usize] = cell;
        true
    }

    /// Check if position is valid (within bounds and empty)
    pub fn is_valid(&self, x: i8, y: i8) -> bool {
        match self.get(x, y) {
            Some(None) => true, // Within bounds and empty
            _ => false,         // Out of bounds or occupied
        }
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
        self.cells[y].iter().all(|cell| cell.is_some())
    }

    /// Clear a row and shift all rows above down
    /// Returns the number of lines cleared (1 or 0)
    pub fn clear_row(&mut self, y: usize) -> usize {
        if y >= BOARD_HEIGHT as usize {
            return 0;
        }

        // Shift all rows above down by one
        for row in (1..=y).rev() {
            self.cells[row] = self.cells[row - 1].clone();
        }

        // Clear the top row using fill (no allocation)
        self.cells[0].fill(None);

        1
    }

    /// Clear all full rows and return the row indices that were cleared (sorted bottom to top)
    /// Uses a two-pointer algorithm for efficiency
    ///
    /// Uses ArrayVec for zero-allocation (max 4 lines can be cleared at once)
    pub fn clear_full_rows(&mut self) -> ArrayVec<usize, 4> {
        let mut cleared_rows = ArrayVec::new();
        let mut write_y = BOARD_HEIGHT as usize; // Points to where we should write the next non-full row

        // Scan from bottom to top
        for read_y in (0..BOARD_HEIGHT as usize).rev() {
            if self.is_row_full(read_y) {
                // This row is full, record it and skip
                cleared_rows.push(read_y);
            } else {
                // This row is not full, move it down to the write position
                write_y -= 1;
                if write_y != read_y {
                    // Clone the row (this allocates but only happens during line clear)
                    // TODO: Optimize with flat array + copy_nonoverlapping for zero-allocation
                    self.cells[write_y] = self.cells[read_y].clone();
                }
            }
        }

        // Clear the remaining rows at the top
        for y in 0..write_y {
            // Use fill instead of creating new vec
            self.cells[y].fill(None);
        }

        // ArrayVec maintains insertion order, reverse to get bottom-to-top
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
        // Spawn position is (3, 0), check if any of the typical spawn cells are occupied
        // For a standard piece shape, we'd check specific cells
        // For simplicity, check if the spawn area has any collisions
        // This is a simplified check - actual implementation depends on piece shape
        !self.is_valid(3, 0) || !self.is_valid(4, 0) || !self.is_valid(5, 0) || !self.is_valid(6, 0)
    }

    /// Get a reference to the internal cells grid
    pub fn cells(&self) -> &Vec<Vec<Cell>> {
        &self.cells
    }

    /// Clear the entire board
    pub fn clear(&mut self) {
        for row in &mut self.cells {
            for cell in row.iter_mut() {
                *cell = None;
            }
        }
    }

    /// Create from a 2D vector for testing
    #[cfg(test)]
    pub fn from_cells(cells: Vec<Vec<Cell>>) -> Self {
        assert_eq!(cells.len(), BOARD_HEIGHT as usize);
        assert!(cells.iter().all(|row| row.len() == BOARD_WIDTH as usize));
        Self { cells }
    }
}

impl Default for Board {
    fn default() -> Self {
        Self::new()
    }
}
