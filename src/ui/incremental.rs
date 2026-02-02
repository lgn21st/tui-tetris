//! Incremental rendering for TUI Tetris
//!
//! NOTE: Currently using FULL RENDERING to ensure locked pieces are always visible.
//! Incremental rendering caused issues where clearing active piece positions
//! would overwrite locked pieces.

use ratatui::{
    buffer::Buffer,
    layout::Rect,
    style::{Color, Modifier, Style},
};

use crate::core::GameState;
use crate::types::{PieceKind, BOARD_HEIGHT, BOARD_WIDTH};

/// Cell dimensions type: (width_in_chars, height_in_chars)
pub type CellDims = (u16, u16);

/// Incremental renderer that tracks state changes to minimize redraws
///
/// CURRENTLY USING FULL RENDERING for reliability
pub struct IncrementalRenderer {
    /// Previous board state for comparison (incremental rendering disabled)
    #[allow(dead_code)]
    last_board: [Option<PieceKind>; (BOARD_WIDTH * BOARD_HEIGHT) as usize],
    /// Previous active piece position
    #[allow(dead_code)]
    last_active: Option<crate::core::Tetromino>,
    /// Previous ghost piece Y position
    #[allow(dead_code)]
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

    /// Calculate flat index from (x, y) coordinates
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

    /// Render the game board
    /// CURRENTLY: Full render every frame for reliability
    ///
    /// ASPECT RATIO FIX: Each board cell is 2 chars wide x 1 char tall
    /// This compensates for terminal characters being ~2:1 (tall:wide)
    pub fn render(&mut self, state: &GameState, area: Rect, buf: &mut Buffer) {
        let board_width = BOARD_WIDTH as u16;
        let board_height = BOARD_HEIGHT as u16;

        // Terminal characters are typically ~2:1 aspect ratio (tall:wide)
        // So each tetris cell should be 2 chars wide x 1 char tall to look square
        const ASPECT_RATIO_NUM: u16 = 2; // width multiplier
        const ASPECT_RATIO_DEN: u16 = 1; // height multiplier

        // Calculate cell size to fit in area
        // Each board cell takes up 2 terminal columns and 1 terminal row visually
        let available_width = area.width / ASPECT_RATIO_NUM;
        let available_height = area.height;
        let cell_width = available_width / board_width;
        let cell_height = available_height / board_height;
        let cell_size = cell_width.min(cell_height).max(1);

        // Each board cell is rendered as (cell_size * 2) wide x cell_size tall
        let cell_width_chars = cell_size * ASPECT_RATIO_NUM;
        let cell_height_chars = cell_size * ASPECT_RATIO_DEN;

        // Center the board in the area
        let total_width = board_width * cell_width_chars;
        let total_height = board_height * cell_height_chars;
        let start_x = area.x + (area.width - total_width) / 2;
        let start_y = area.y + (area.height - total_height) / 2;

        // Store cell dimensions for rendering
        let cell_dims: CellDims = (cell_width_chars, cell_height_chars);

        // Render board background
        self.render_board_background(start_x, start_y, total_width, total_height, buf);

        // Render border
        self.render_border(start_x, start_y, total_width, total_height, buf);

        // Render all board cells (locked pieces) - FULL RENDER
        self.render_all_board_cells(state, start_x, start_y, cell_dims, buf);

        // Render ghost piece
        self.render_ghost_piece(state, start_x, start_y, cell_dims, buf);

        // Render active piece
        self.render_active_piece(state, start_x, start_y, cell_dims, buf);

        // Update frame counter and state
        self.frame_count += 1;
        self.update_state(state);
    }

    /// Render board background (solid color for play area)
    fn render_board_background(
        &self,
        start_x: u16,
        start_y: u16,
        total_width: u16,
        total_height: u16,
        buf: &mut Buffer,
    ) {
        let bg_style = Style::default().bg(Color::Rgb(30, 30, 40)); // Dark blue-gray background

        for y in start_y..start_y + total_height {
            for x in start_x..start_x + total_width {
                if x < buf.area().width && y < buf.area().height {
                    let cell = &mut buf[(x, y)];
                    cell.set_char(' ');
                    cell.set_style(bg_style);
                }
            }
        }
    }

    /// Render all board cells (locked pieces)
    fn render_all_board_cells(
        &self,
        state: &GameState,
        start_x: u16,
        start_y: u16,
        cell_dims: CellDims,
        buf: &mut Buffer,
    ) {
        let cells = state.board.cells();
        let (cell_width, cell_height) = cell_dims;

        for y in 0..BOARD_HEIGHT {
            for x in 0..BOARD_WIDTH {
                let idx = Self::index(x, y);
                let cell = cells.get(idx as usize).copied().flatten();

                let screen_x = start_x + (x as u16) * cell_width;
                let screen_y = start_y + (y as u16) * cell_height;

                if let Some(kind) = cell {
                    // Cell has a locked piece
                    let (ch, color) = Self::piece_style(kind);
                    let style = Style::default().fg(color);
                    self.fill_cell(buf, screen_x, screen_y, cell_dims, ch, style);
                } else {
                    // Cell is empty
                    let style = Style::default().fg(Color::DarkGray);
                    self.fill_cell(buf, screen_x, screen_y, cell_dims, '·', style);
                }
            }
        }
    }

    /// Render ghost piece
    fn render_ghost_piece(
        &self,
        state: &GameState,
        start_x: u16,
        start_y: u16,
        cell_dims: CellDims,
        buf: &mut Buffer,
    ) {
        if let (Some(active), Some(ghost_y)) = (state.active, state.ghost_y()) {
            let (cell_width, cell_height) = cell_dims;
            let style = Style::default().fg(Color::Gray);
            for &(dx, dy) in active.shape().iter() {
                let x = (active.x + dx) as u16;
                let y = (ghost_y + dy) as u16;
                if x < BOARD_WIDTH as u16 && y < BOARD_HEIGHT as u16 {
                    let screen_x = start_x + x * cell_width;
                    let screen_y = start_y + y * cell_height;
                    self.fill_cell(buf, screen_x, screen_y, cell_dims, '░', style);
                }
            }
        }
    }

    /// Render active piece
    fn render_active_piece(
        &self,
        state: &GameState,
        start_x: u16,
        start_y: u16,
        cell_dims: CellDims,
        buf: &mut Buffer,
    ) {
        if let Some(active) = state.active {
            let (cell_width, cell_height) = cell_dims;
            let (ch, color) = Self::piece_style(active.kind);
            let style = Style::default().fg(color).add_modifier(Modifier::BOLD);

            for &(dx, dy) in active.shape().iter() {
                let x = (active.x + dx) as u16;
                let y = (active.y + dy) as u16;
                if x < BOARD_WIDTH as u16 && y < BOARD_HEIGHT as u16 {
                    let screen_x = start_x + x * cell_width;
                    let screen_y = start_y + y * cell_height;
                    self.fill_cell(buf, screen_x, screen_y, cell_dims, ch, style);
                }
            }
        }
    }

    /// Render border around the board
    fn render_border(
        &self,
        start_x: u16,
        start_y: u16,
        total_width: u16,
        total_height: u16,
        buf: &mut Buffer,
    ) {
        let border_style = Style::default().fg(Color::Rgb(200, 200, 200));

        // Top-left corner
        if start_x > 0 && start_y > 0 {
            let corner = &mut buf[(start_x - 1, start_y - 1)];
            corner.set_char('┌');
            corner.set_style(border_style);
        }

        // Top-right corner
        if start_x + total_width < buf.area().width && start_y > 0 {
            let corner = &mut buf[(start_x + total_width, start_y - 1)];
            corner.set_char('┐');
            corner.set_style(border_style);
        }

        // Bottom-left corner
        if start_x > 0 && start_y + total_height < buf.area().height {
            let corner = &mut buf[(start_x - 1, start_y + total_height)];
            corner.set_char('└');
            corner.set_style(border_style);
        }

        // Bottom-right corner
        if start_x + total_width < buf.area().width && start_y + total_height < buf.area().height {
            let corner = &mut buf[(start_x + total_width, start_y + total_height)];
            corner.set_char('┘');
            corner.set_style(border_style);
        }

        // Top and bottom borders
        for x in 0..total_width {
            if start_x + x < buf.area().width {
                if start_y > 0 {
                    let top = &mut buf[(start_x + x, start_y - 1)];
                    top.set_char('─');
                    top.set_style(border_style);
                }
                if start_y + total_height < buf.area().height {
                    let bottom = &mut buf[(start_x + x, start_y + total_height)];
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
    }

    /// Fill a cell area with a character and style
    /// cell_dims: (width_in_chars, height_in_chars)
    fn fill_cell(
        &self,
        buf: &mut Buffer,
        x: u16,
        y: u16,
        cell_dims: CellDims,
        ch: char,
        style: Style,
    ) {
        let (width, height) = cell_dims;
        for dy in 0..height {
            for dx in 0..width {
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
}
