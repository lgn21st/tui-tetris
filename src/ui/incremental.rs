//! Incremental rendering - only re-render changed cells
//!
//! This module provides efficient rendering by tracking changes between frames
//! and only updating cells that have actually changed.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::core::{GameState, Tetromino};
use crate::types::{PieceKind, BOARD_HEIGHT, BOARD_WIDTH};

/// Tracks board state changes for incremental rendering
pub struct IncrementalRenderer {
    /// Last rendered board state (flat array)
    last_board: [Option<PieceKind>; (BOARD_WIDTH * BOARD_HEIGHT) as usize],
    /// Last rendered active piece position
    last_active: Option<Tetromino>,
    /// Last rendered ghost Y position
    last_ghost_y: Option<i8>,
    /// Frame counter for first-frame detection
    frame_count: u32,
}

impl IncrementalRenderer {
    /// Create a new incremental renderer
    pub fn new() -> Self {
        Self {
            last_board: [None; (BOARD_WIDTH * BOARD_HEIGHT) as usize],
            last_active: None,
            last_ghost_y: None,
            frame_count: 0,
        }
    }

    /// Calculate flat index from (x, y)
    #[inline(always)]
    fn index(x: u8, y: u8) -> usize {
        (y as usize) * (BOARD_WIDTH as usize) + (x as usize)
    }

    /// Get piece style (character and color)
    fn piece_style(kind: PieceKind) -> (char, Color) {
        let color = match kind {
            PieceKind::I => Color::Cyan,
            PieceKind::O => Color::Yellow,
            PieceKind::T => Color::Magenta,
            PieceKind::S => Color::Green,
            PieceKind::Z => Color::Red,
            PieceKind::J => Color::Blue,
            PieceKind::L => Color::Rgb(255, 165, 0), // Orange
        };
        ('█', color)
    }

    /// Render the game board incrementally
    /// Only updates cells that have changed since last render
    pub fn render(&mut self, state: &GameState, area: Rect, buf: &mut Buffer) {
        let board_width = BOARD_WIDTH as u16;
        let board_height = BOARD_HEIGHT as u16;

        // Calculate cell size to fit in area
        let cell_width = area.width / board_width;
        let cell_height = area.height / board_height;
        let cell_size = cell_width.min(cell_height).max(1);

        // Center the board in the area
        let total_width = board_width * cell_size;
        let total_height = board_height * cell_size;
        let start_x = area.x + (area.width - total_width) / 2;
        let start_y = area.y + (area.height - total_height) / 2;

        // On first frame, do a full render
        let is_first_frame = self.frame_count == 0;

        // Render board cells
        if is_first_frame {
            // Full render on first frame
            self.render_all_board_cells(state, start_x, start_y, cell_size, buf);
        } else {
            // Incremental render on subsequent frames
            self.render_changed_board_cells(state, start_x, start_y, cell_size, buf);
        }

        // Render ghost piece (clear old if moved)
        self.render_ghost_piece(state, start_x, start_y, cell_size, buf);

        // Render active piece (clear old if moved)
        self.render_active_piece(state, start_x, start_y, cell_size, buf);

        // Render border (static, only once)
        if is_first_frame {
            self.render_border(start_x, start_y, total_width, total_height, buf);
        }

        // Update state for next frame
        self.update_state(state);
        self.frame_count += 1;
    }

    /// Render all board cells (for first frame)
    fn render_all_board_cells(
        &self,
        state: &GameState,
        start_x: u16,
        start_y: u16,
        cell_size: u16,
        buf: &mut Buffer,
    ) {
        let cells = state.board.cells();

        for y in 0..BOARD_HEIGHT {
            for x in 0..BOARD_WIDTH {
                let idx = Self::index(x, y);
                let cell = cells.get(idx as usize).copied().flatten();

                let screen_x = start_x + (x as u16) * cell_size;
                let screen_y = start_y + (y as u16) * cell_size;

                if let Some(kind) = cell {
                    // Cell is occupied
                    let (ch, color) = Self::piece_style(kind);
                    let style = Style::default().fg(color);
                    self.fill_cell(buf, screen_x, screen_y, cell_size, ch, style);
                } else {
                    // Cell is empty
                    let style = Style::default().fg(Color::DarkGray);
                    self.fill_cell(buf, screen_x, screen_y, cell_size, '·', style);
                }
            }
        }
    }

    /// Render only board cells that have changed
    fn render_changed_board_cells(
        &mut self,
        state: &GameState,
        start_x: u16,
        start_y: u16,
        cell_size: u16,
        buf: &mut Buffer,
    ) {
        let cells = state.board.cells();

        for y in 0..BOARD_HEIGHT {
            for x in 0..BOARD_WIDTH {
                let idx = Self::index(x, y);
                let current = cells.get(idx as usize).copied().flatten();
                let previous = self.last_board[idx as usize];

                // Only render if changed
                if current != previous {
                    let screen_x = start_x + (x as u16) * cell_size;
                    let screen_y = start_y + (y as u16) * cell_size;

                    if let Some(kind) = current {
                        // Cell is now occupied
                        let (ch, color) = Self::piece_style(kind);
                        let style = Style::default().fg(color);
                        self.fill_cell(buf, screen_x, screen_y, cell_size, ch, style);
                    } else {
                        // Cell is now empty
                        let style = Style::default().fg(Color::DarkGray);
                        self.fill_cell(buf, screen_x, screen_y, cell_size, '·', style);
                    }
                }
            }
        }
    }

    /// Render ghost piece, clearing old position if moved
    fn render_ghost_piece(
        &mut self,
        state: &GameState,
        start_x: u16,
        start_y: u16,
        cell_size: u16,
        buf: &mut Buffer,
    ) {
        if let (Some(active), Some(ghost_y)) = (state.active, state.ghost_y()) {
            // On first frame, always draw ghost (no clearing needed)
            if self.frame_count > 0 {
                // Clear old ghost if position changed
                if let Some(last_y) = self.last_ghost_y {
                    if last_y != ghost_y {
                        self.clear_piece_at(
                            state.active.unwrap(),
                            last_y,
                            start_x,
                            start_y,
                            cell_size,
                            buf,
                        );
                    }
                }
            }

            // Draw new ghost
            let style = Style::default().fg(Color::Gray);
            for &(dx, dy) in active.shape().iter() {
                let x = (active.x + dx) as u16;
                let y = (ghost_y + dy) as u16;
                if x < BOARD_WIDTH as u16 && y < BOARD_HEIGHT as u16 {
                    let screen_x = start_x + x * cell_size;
                    let screen_y = start_y + y * cell_size;
                    self.fill_cell(buf, screen_x, screen_y, cell_size, '░', style);
                }
            }
        }
    }

    /// Render active piece, clearing old position if moved
    fn render_active_piece(
        &mut self,
        state: &GameState,
        start_x: u16,
        start_y: u16,
        cell_size: u16,
        buf: &mut Buffer,
    ) {
        if let Some(active) = state.active {
            // On first frame, always draw piece (no clearing needed)
            if self.frame_count > 0 {
                // Clear old active piece if position changed
                if let Some(last) = self.last_active {
                    if last.x != active.x || last.y != active.y || last.rotation != active.rotation
                    {
                        self.clear_piece_at(last, last.y, start_x, start_y, cell_size, buf);
                    }
                }
            }

            // Draw new active piece
            let (ch, color) = Self::piece_style(active.kind);
            let style = Style::default().fg(color).add_modifier(Modifier::BOLD);

            for &(dx, dy) in active.shape().iter() {
                let x = (active.x + dx) as u16;
                let y = (active.y + dy) as u16;
                if x < BOARD_WIDTH as u16 && y < BOARD_HEIGHT as u16 {
                    let screen_x = start_x + x * cell_size;
                    let screen_y = start_y + y * cell_size;
                    self.fill_cell(buf, screen_x, screen_y, cell_size, ch, style);
                }
            }
        }
    }

    /// Clear a piece at a specific position (restore background)
    fn clear_piece_at(
        &self,
        piece: Tetromino,
        y_pos: i8,
        start_x: u16,
        start_y: u16,
        cell_size: u16,
        buf: &mut Buffer,
    ) {
        let style = Style::default().fg(Color::DarkGray);

        for &(dx, dy) in piece.shape().iter() {
            let x = (piece.x + dx) as u16;
            let y = (y_pos + dy) as u16;
            if x < BOARD_WIDTH as u16 && y < BOARD_HEIGHT as u16 {
                let screen_x = start_x + x * cell_size;
                let screen_y = start_y + y * cell_size;
                self.fill_cell(buf, screen_x, screen_y, cell_size, '·', style);
            }
        }
    }

    /// Fill a cell area with a character and style
    fn fill_cell(&self, buf: &mut Buffer, x: u16, y: u16, size: u16, ch: char, style: Style) {
        for dy in 0..size {
            for dx in 0..size {
                let cell_x = x + dx;
                let cell_y = y + dy;
                if cell_x < buf.area().width && cell_y < buf.area().height {
                    let cell = &mut buf[(cell_x, cell_y)];
                    cell.set_char(ch);
                    cell.set_style(style);
                }
            }
        }
    }

    /// Render border around the board (static)
    fn render_border(
        &self,
        start_x: u16,
        start_y: u16,
        total_width: u16,
        total_height: u16,
        buf: &mut Buffer,
    ) {
        let border_style = Style::default().fg(Color::White);

        // Top and bottom borders
        for x in 0..total_width + 2 {
            if start_x + x > 0 && start_x + x < buf.area().width {
                if start_y > 0 {
                    let top = &mut buf[(start_x + x - 1, start_y - 1)];
                    top.set_char('─');
                    top.set_style(border_style);
                }
                if start_y + total_height < buf.area().height {
                    let bottom = &mut buf[(start_x + x - 1, start_y + total_height)];
                    bottom.set_char('─');
                    bottom.set_style(border_style);
                }
            }
        }

        // Side borders
        for y in 0..total_height {
            if start_y + y < buf.area().height {
                if start_x > 0 {
                    let left = &mut buf[(start_x - 1, start_y + y)];
                    left.set_char('│');
                    left.set_style(border_style);
                }
                if start_x + total_width < buf.area().width {
                    let right = &mut buf[(start_x + total_width, start_y + y)];
                    right.set_char('│');
                    right.set_style(border_style);
                }
            }
        }

        // Corners
        if start_x > 0 && start_y > 0 {
            let corner = &mut buf[(start_x - 1, start_y - 1)];
            corner.set_char('┌');
            corner.set_style(border_style);
        }
        if start_x + total_width < buf.area().width && start_y > 0 {
            let corner = &mut buf[(start_x + total_width, start_y - 1)];
            corner.set_char('┐');
            corner.set_style(border_style);
        }
        if start_x > 0 && start_y + total_height < buf.area().height {
            let corner = &mut buf[(start_x - 1, start_y + total_height)];
            corner.set_char('└');
            corner.set_style(border_style);
        }
        if start_x + total_width < buf.area().width && start_y + total_height < buf.area().height {
            let corner = &mut buf[(start_x + total_width, start_y + total_height)];
            corner.set_char('┘');
            corner.set_style(border_style);
        }
    }

    /// Update internal state for next frame
    fn update_state(&mut self, state: &GameState) {
        // Copy current board state
        let cells = state.board.cells();
        for (i, cell) in cells.iter().enumerate() {
            if i < self.last_board.len() {
                self.last_board[i] = *cell;
            }
        }

        // Update active piece and ghost
        self.last_active = state.active;
        self.last_ghost_y = state.ghost_y();
    }
}

impl Default for IncrementalRenderer {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::core::GameState;

    #[test]
    fn test_incremental_renderer_new() {
        let renderer = IncrementalRenderer::new();
        assert!(renderer.last_active.is_none());
        assert!(renderer.last_ghost_y.is_none());
        assert_eq!(renderer.frame_count, 0);
    }

    #[test]
    fn test_index_calculation() {
        assert_eq!(IncrementalRenderer::index(0, 0), 0);
        assert_eq!(IncrementalRenderer::index(9, 0), 9);
        assert_eq!(IncrementalRenderer::index(0, 1), 10);
        assert_eq!(IncrementalRenderer::index(9, 19), 199);
    }

    #[test]
    fn test_first_frame_detection() {
        let mut renderer = IncrementalRenderer::new();
        assert_eq!(renderer.frame_count, 0);

        // Simulate one frame
        renderer.frame_count += 1;
        assert_eq!(renderer.frame_count, 1);
    }
}
