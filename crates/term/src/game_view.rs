//! GameView: maps `core::GameState` into a terminal framebuffer.
//!
//! This module is pure (no I/O). It can be unit-tested.

use crate::core::{get_shape, GameSnapshot};
use crate::fb::{CellStyle, FrameBuffer, Rgb};
use crate::types::{PieceKind, BOARD_HEIGHT, BOARD_WIDTH};

/// Terminal viewport dimensions.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Viewport {
    pub width: u16,
    pub height: u16,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct AdapterStatusView {
    pub enabled: bool,
    pub client_count: u16,
    pub controller_id: Option<usize>,
    pub streaming_count: u16,
}

impl Viewport {
    pub fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

/// A lightweight terminal renderer for the Tetris game.
pub struct GameView {
    /// Board cell width in terminal columns.
    cell_w: u16,
    /// Board cell height in terminal rows.
    cell_h: u16,
    anchor_y: AnchorY,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AnchorY {
    Center,
    Top,
}

impl Default for GameView {
    fn default() -> Self {
        // 2x1 helps compensate for typical terminal glyph aspect ratio.
        Self {
            cell_w: 2,
            cell_h: 1,
            anchor_y: AnchorY::Center,
        }
    }
}

impl GameView {
    pub fn new(cell_w: u16, cell_h: u16) -> Self {
        Self {
            cell_w,
            cell_h,
            anchor_y: AnchorY::Center,
        }
    }

    pub fn with_anchor_y(mut self, anchor_y: AnchorY) -> Self {
        self.anchor_y = anchor_y;
        self
    }

    /// Render the current game state into an existing framebuffer.
    ///
    /// This is the allocation-free hot path. Callers can reuse a framebuffer
    /// across frames and only resize when the terminal size changes.
    pub fn render_into(&self, snap: &GameSnapshot, viewport: Viewport, fb: &mut FrameBuffer) {
        self.render_into_with_adapter(snap, None, viewport, fb);
    }

    pub fn render_into_with_adapter(
        &self,
        snap: &GameSnapshot,
        adapter: Option<&AdapterStatusView>,
        viewport: Viewport,
        fb: &mut FrameBuffer,
    ) {
        fb.resize(viewport.width, viewport.height);
        fb.clear(CellStyle::default().into_cell(' '));

        let board_px_w = (BOARD_WIDTH as u16) * self.cell_w;
        let board_px_h = (BOARD_HEIGHT as u16) * self.cell_h;
        let frame_w = board_px_w + 2;
        let frame_h = board_px_h + 2;

        let start_x = viewport.width.saturating_sub(frame_w) / 2;
        let start_y = match self.anchor_y {
            AnchorY::Center => viewport.height.saturating_sub(frame_h) / 2,
            AnchorY::Top => 0,
        };

        let bg = CellStyle {
            fg: Rgb::new(80, 80, 90),
            bg: Rgb::new(30, 30, 40),
            bold: false,
            dim: false,
        };
        let border = CellStyle {
            fg: Rgb::new(200, 200, 200),
            bg: Rgb::new(0, 0, 0),
            bold: false,
            dim: false,
        };

        // Background for play area.
        fb.fill_rect(start_x + 1, start_y + 1, board_px_w, board_px_h, ' ', bg);

        // Border.
        self.draw_border(fb, start_x, start_y, frame_w, frame_h, border);

        // Locked board cells.
        for y in 0..BOARD_HEIGHT as u16 {
            for x in 0..BOARD_WIDTH as u16 {
                let cell = snap.board[y as usize][x as usize];
                if let Some(kind) = piece_from_cell(cell) {
                    self.draw_board_cell(fb, start_x, start_y, x, y, kind, true);
                } else {
                    // Optional grid dot.
                    self.draw_empty_cell(fb, start_x, start_y, x, y);
                }
            }
        }

        // Ghost piece.
        if let (Some(active), Some(ghost_y)) = (snap.active, snap.ghost_y) {
            let ghost_style = CellStyle {
                fg: Rgb::new(140, 140, 140),
                bg: Rgb::new(30, 30, 40),
                bold: false,
                dim: true,
            };
            for &(dx, dy) in get_shape(active.kind, active.rotation).iter() {
                let x = active.x + dx;
                let y = ghost_y + dy;
                if x >= 0 && x < BOARD_WIDTH as i8 && y >= 0 && y < BOARD_HEIGHT as i8 {
                    self.fill_cell_rect(fb, start_x, start_y, x as u16, y as u16, '░', ghost_style);
                }
            }
        }

        // Active piece.
        if let Some(active) = snap.active {
            for &(dx, dy) in get_shape(active.kind, active.rotation).iter() {
                let x = active.x + dx;
                let y = active.y + dy;
                if x >= 0 && x < BOARD_WIDTH as i8 && y >= 0 && y < BOARD_HEIGHT as i8 {
                    self.draw_board_cell(
                        fb,
                        start_x,
                        start_y,
                        x as u16,
                        y as u16,
                        active.kind,
                        true,
                    );
                }
            }
        }

        // Side panel (score/next/hold).
        self.draw_side_panel(fb, snap, adapter, viewport, start_x, start_y, frame_w);

        // Overlays.
        if snap.paused {
            self.draw_overlay_text(fb, start_x, start_y, frame_w, frame_h, "PAUSED");
        } else if snap.game_over {
            self.draw_overlay_text(fb, start_x, start_y, frame_w, frame_h, "GAME OVER");
        }
    }

    /// Convenience helper that allocates a new framebuffer.
    pub fn render(&self, snap: &GameSnapshot, viewport: Viewport) -> FrameBuffer {
        let mut fb = FrameBuffer::new(viewport.width, viewport.height);
        self.render_into(snap, viewport, &mut fb);
        fb
    }

    pub fn render_with_adapter(
        &self,
        snap: &GameSnapshot,
        adapter: Option<&AdapterStatusView>,
        viewport: Viewport,
    ) -> FrameBuffer {
        let mut fb = FrameBuffer::new(viewport.width, viewport.height);
        self.render_into_with_adapter(snap, adapter, viewport, &mut fb);
        fb
    }

    fn draw_border(&self, fb: &mut FrameBuffer, x: u16, y: u16, w: u16, h: u16, style: CellStyle) {
        if w < 2 || h < 2 {
            return;
        }

        fb.put_char(x, y, '┌', style);
        fb.put_char(x + w - 1, y, '┐', style);
        fb.put_char(x, y + h - 1, '└', style);
        fb.put_char(x + w - 1, y + h - 1, '┘', style);

        for dx in 1..w - 1 {
            fb.put_char(x + dx, y, '─', style);
            fb.put_char(x + dx, y + h - 1, '─', style);
        }
        for dy in 1..h - 1 {
            fb.put_char(x, y + dy, '│', style);
            fb.put_char(x + w - 1, y + dy, '│', style);
        }
    }

    fn draw_empty_cell(&self, fb: &mut FrameBuffer, start_x: u16, start_y: u16, x: u16, y: u16) {
        let style = CellStyle {
            fg: Rgb::new(90, 90, 100),
            bg: Rgb::new(30, 30, 40),
            bold: false,
            dim: true,
        };
        self.fill_cell_rect(fb, start_x, start_y, x, y, '·', style);
    }

    fn draw_board_cell(
        &self,
        fb: &mut FrameBuffer,
        start_x: u16,
        start_y: u16,
        x: u16,
        y: u16,
        kind: PieceKind,
        bold: bool,
    ) {
        let (fg, ch) = match kind {
            PieceKind::I => (Rgb::new(80, 220, 220), '█'),
            PieceKind::O => (Rgb::new(240, 220, 80), '█'),
            PieceKind::T => (Rgb::new(200, 120, 220), '█'),
            PieceKind::S => (Rgb::new(100, 220, 120), '█'),
            PieceKind::Z => (Rgb::new(220, 80, 80), '█'),
            PieceKind::J => (Rgb::new(80, 120, 220), '█'),
            PieceKind::L => (Rgb::new(255, 165, 0), '█'),
        };
        let style = CellStyle {
            fg,
            bg: Rgb::new(30, 30, 40),
            bold,
            dim: false,
        };
        self.fill_cell_rect(fb, start_x, start_y, x, y, ch, style);
    }

    fn fill_cell_rect(
        &self,
        fb: &mut FrameBuffer,
        start_x: u16,
        start_y: u16,
        cell_x: u16,
        cell_y: u16,
        ch: char,
        style: CellStyle,
    ) {
        let px = start_x + 1 + cell_x * self.cell_w;
        let py = start_y + 1 + cell_y * self.cell_h;
        fb.fill_rect(px, py, self.cell_w, self.cell_h, ch, style);
    }

    fn draw_side_panel(
        &self,
        fb: &mut FrameBuffer,
        snap: &GameSnapshot,
        adapter: Option<&AdapterStatusView>,
        viewport: Viewport,
        start_x: u16,
        start_y: u16,
        frame_w: u16,
    ) {
        let panel_x = start_x.saturating_add(frame_w).saturating_add(2);
        if panel_x >= viewport.width {
            return;
        }
        let panel_w = viewport.width - panel_x;
        if panel_w < 12 {
            return;
        }

        let label = CellStyle {
            fg: Rgb::new(220, 220, 220),
            bg: Rgb::new(0, 0, 0),
            bold: true,
            dim: false,
        };
        let value = CellStyle {
            fg: Rgb::new(200, 200, 200),
            bg: Rgb::new(0, 0, 0),
            bold: false,
            dim: false,
        };

        let mut y = start_y;
        fb.put_str(panel_x, y, "SCORE", label);
        y = y.saturating_add(1);
        fb.put_u32(panel_x, y, snap.score, value);
        y = y.saturating_add(2);

        fb.put_str(panel_x, y, "LEVEL", label);
        y = y.saturating_add(1);
        fb.put_u32(panel_x, y, snap.level, value);
        y = y.saturating_add(2);

        fb.put_str(panel_x, y, "LINES", label);
        y = y.saturating_add(1);
        fb.put_u32(panel_x, y, snap.lines, value);
        y = y.saturating_add(2);

        fb.put_str(panel_x, y, "HOLD", label);
        y = y.saturating_add(1);
        fb.put_str(
            panel_x,
            y,
            snap.hold.map(piece_letter).unwrap_or("-"),
            value,
        );
        y = y.saturating_add(2);

        fb.put_str(panel_x, y, "NEXT", label);
        y = y.saturating_add(1);
        for (i, k) in snap.next_queue.iter().take(5).enumerate() {
            if y >= viewport.height {
                break;
            }
            fb.put_str(panel_x, y, piece_letter(*k), value);
            if panel_w >= 16 {
                let dim = CellStyle { dim: true, ..value };
                fb.put_char(panel_x + 2, y, '#', dim);
                fb.put_u32(panel_x + 3, y, (i as u32) + 1, dim);
            }
            y = y.saturating_add(1);
        }

        y = y.saturating_add(1);
        fb.put_str(panel_x, y, "AI", label);
        y = y.saturating_add(1);
        if let Some(st) = adapter {
            fb.put_str(panel_x, y, "ON", value);
            if panel_w >= 16 {
                let dim = CellStyle { dim: true, ..value };
                fb.put_str(panel_x + 3, y, "clients", dim);
            }
            y = y.saturating_add(1);
            fb.put_str(panel_x, y, "C", value);
            fb.put_u32(panel_x + 2, y, st.client_count as u32, value);
            y = y.saturating_add(1);
            fb.put_str(panel_x, y, "S", value);
            fb.put_u32(panel_x + 2, y, st.streaming_count as u32, value);
            y = y.saturating_add(1);
            fb.put_str(panel_x, y, "CTRL", value);
            if let Some(id) = st.controller_id {
                fb.put_u32(panel_x + 5, y, id as u32, value);
            } else {
                fb.put_str(panel_x + 5, y, "-", value);
            }
        } else {
            fb.put_str(panel_x, y, "OFF", value);
        }
    }

    fn draw_overlay_text(
        &self,
        fb: &mut FrameBuffer,
        start_x: u16,
        start_y: u16,
        frame_w: u16,
        frame_h: u16,
        text: &str,
    ) {
        let mid_y = start_y.saturating_add(frame_h / 2);
        let text_w = text.chars().count() as u16;
        let x = start_x.saturating_add(frame_w.saturating_sub(text_w) / 2);
        let style = CellStyle {
            fg: Rgb::new(255, 255, 255),
            bg: Rgb::new(0, 0, 0),
            bold: true,
            dim: false,
        };
        fb.put_str(x, mid_y, text, style);
    }
}

fn piece_from_cell(v: u8) -> Option<PieceKind> {
    match v {
        1 => Some(PieceKind::I),
        2 => Some(PieceKind::O),
        3 => Some(PieceKind::T),
        4 => Some(PieceKind::S),
        5 => Some(PieceKind::Z),
        6 => Some(PieceKind::J),
        7 => Some(PieceKind::L),
        _ => None,
    }
}

fn piece_letter(kind: PieceKind) -> &'static str {
    match kind {
        PieceKind::I => "I",
        PieceKind::O => "O",
        PieceKind::T => "T",
        PieceKind::S => "S",
        PieceKind::Z => "Z",
        PieceKind::J => "J",
        PieceKind::L => "L",
    }
}

trait IntoCell {
    fn into_cell(self, ch: char) -> crate::fb::Cell;
}

impl IntoCell for CellStyle {
    fn into_cell(self, ch: char) -> crate::fb::Cell {
        crate::fb::Cell { ch, style: self }
    }
}
