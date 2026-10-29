//! GameView: maps an immutable `GameSnapshot` into a terminal framebuffer.
//!
//! This module is pure (no I/O). It can be unit-tested.

use crate::term::fb::{CellStyle, FrameBuffer, Rgb};
use tetris_core::core::GameSnapshot;
use tetris_core::types::{BOARD_HEIGHT, BOARD_WIDTH, PieceKind};

const PAGE_BG: Rgb = Rgb::new(16, 16, 16);
const WELL_BG: Rgb = Rgb::new(28, 28, 28);
const RIM: Rgb = Rgb::new(46, 46, 46);
const RIM_CLEAR: Rgb = Rgb::new(72, 72, 72);
const LABEL: Rgb = Rgb::new(168, 168, 168);
const VALUE: Rgb = Rgb::new(230, 230, 230);
const ACCENT: Rgb = Rgb::new(79, 209, 196);
const OVERLAY_BG: Rgb = Rgb::new(12, 12, 12);

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
    pub pid: u32,
    pub listen_addr: Option<std::net::SocketAddr>,
}

impl AdapterStatusView {
    /// HUD status code:
    /// 0=adapter off, 1=listening(no clients), 2=clients(no controller),
    /// 3=controller active(no streaming), 4=streaming active.
    pub fn status_code(&self) -> u32 {
        if !self.enabled {
            return 0;
        }
        if self.client_count == 0 {
            return 1;
        }
        if self.controller_id.is_none() {
            return 2;
        }
        if self.streaming_count == 0 {
            return 3;
        }
        4
    }
}

const HUD_OVERLAY_ROWS: usize = 5;
const HUD_LABEL_MAX: usize = 6;
const HUD_TEXT_MAX: usize = 48;

/// Five-line top-left HUD chrome shared by local adapter status and observe attach.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct HudOverlay {
    rows: [HudOverlayRow; HUD_OVERLAY_ROWS],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct HudOverlayRow {
    label: [u8; HUD_LABEL_MAX],
    label_len: u8,
    value: HudOverlayValue,
}

/// Overlay value: a number, a dash, or a short ASCII string.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum HudOverlayValue {
    U32(u32),
    Dash,
    Text { bytes: [u8; HUD_TEXT_MAX], len: u8 },
}

impl HudOverlayValue {
    pub fn text(s: &str) -> Self {
        let mut bytes = [0u8; HUD_TEXT_MAX];
        let n = s.floor_char_boundary(HUD_TEXT_MAX);
        bytes[..n].copy_from_slice(&s.as_bytes()[..n]);
        Self::Text {
            bytes,
            len: n as u8,
        }
    }

    pub fn as_text(&self) -> Option<&str> {
        match self {
            Self::Text { bytes, len } => std::str::from_utf8(&bytes[..*len as usize]).ok(),
            _ => None,
        }
    }

    pub fn as_u32(&self) -> Option<u32> {
        match self {
            Self::U32(v) => Some(*v),
            _ => None,
        }
    }
}

impl HudOverlayRow {
    fn new(label: &str, value: HudOverlayValue) -> Self {
        let mut bytes = [0u8; HUD_LABEL_MAX];
        let n = label.len().min(HUD_LABEL_MAX);
        bytes[..n].copy_from_slice(&label.as_bytes()[..n]);
        Self {
            label: bytes,
            label_len: n as u8,
            value,
        }
    }

    fn label(&self) -> &str {
        std::str::from_utf8(&self.label[..self.label_len as usize]).unwrap_or("")
    }
}

impl HudOverlay {
    pub fn from_rows(rows: [(&str, HudOverlayValue); HUD_OVERLAY_ROWS]) -> Self {
        Self {
            rows: rows.map(|(label, value)| HudOverlayRow::new(label, value)),
        }
    }

    pub fn from_adapter(status: &AdapterStatusView) -> Self {
        Self::from_rows([
            ("CONN", HudOverlayValue::U32(status.client_count as u32)),
            ("ST", HudOverlayValue::U32(status.status_code())),
            (
                "CTRL",
                status
                    .controller_id
                    .map(|id| HudOverlayValue::U32(id as u32))
                    .unwrap_or(HudOverlayValue::Dash),
            ),
            (
                "PORT",
                status
                    .listen_addr
                    .map(|addr| HudOverlayValue::U32(u32::from(addr.port())))
                    .unwrap_or(HudOverlayValue::Dash),
            ),
            ("PID", HudOverlayValue::U32(status.pid)),
        ])
    }

    pub fn label(&self, index: usize) -> &str {
        self.rows.get(index).map(HudOverlayRow::label).unwrap_or("")
    }

    pub fn value(&self, index: usize) -> HudOverlayValue {
        self.rows
            .get(index)
            .map(|row| row.value)
            .unwrap_or(HudOverlayValue::Dash)
    }
}

/// Immutable terminal projection decoupled from the authoritative session.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GameViewModel {
    snapshot: GameSnapshot,
    overlay: Option<HudOverlay>,
}

impl GameViewModel {
    pub fn new(snapshot: GameSnapshot, overlay: Option<HudOverlay>) -> Self {
        Self { snapshot, overlay }
    }

    pub fn snapshot(&self) -> &GameSnapshot {
        &self.snapshot
    }

    pub fn overlay(&self) -> Option<&HudOverlay> {
        self.overlay.as_ref()
    }
}

impl Viewport {
    pub fn new(width: u16, height: u16) -> Self {
        Self { width, height }
    }
}

/// Cell anchors of the side panel's pixel previews.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct PanelLayout {
    /// Top row of the panel (the frame's first row).
    pub top: u16,
    /// Left column of the panel.
    pub x: u16,
    /// Top-left cell of the HOLD preview.
    pub hold: (u16, u16),
    /// Top-left cell of the NEXT strip.
    pub next: (u16, u16),
    /// Rows reserved for the NEXT strip.
    pub next_rows: u16,
}

/// A lightweight terminal renderer for the Tetris game.
pub struct GameView {
    /// Board cell width in terminal columns.
    cell_w: u16,
    /// Board cell height in terminal rows.
    cell_h: u16,
    /// Terminal cells reserved for the well interior.
    ///
    /// Usually `10 x 20` minos at `cell_w x cell_h` cells each, but the pixel
    /// backend sizes the frame so it hugs a *square* mino grid, which can shave
    /// a row off (an 8x17 terminal cell needs only 19 rows for 20 square minos).
    well_cells: (u16, u16),
    /// Cell boxes the HUD must leave free for the pixel previews.
    preview_hold: (u16, u16),
    preview_next: (u16, u16),
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
            well_cells: (BOARD_WIDTH as u16 * 2, BOARD_HEIGHT as u16),
            preview_hold: (4, 2),
            preview_next: (4, 7),
            anchor_y: AnchorY::Center,
        }
    }
}

impl GameView {
    pub fn new(cell_w: u16, cell_h: u16) -> Self {
        Self {
            cell_w,
            cell_h,
            well_cells: (BOARD_WIDTH as u16 * cell_w, BOARD_HEIGHT as u16 * cell_h),
            preview_hold: (4 * cell_w, 2 * cell_h),
            preview_next: (4 * cell_w, 7 * cell_h),
            anchor_y: AnchorY::Center,
        }
    }

    /// Cell boxes reserved for the pixel HOLD/NEXT previews.
    pub fn preview_cells(&self) -> ((u16, u16), (u16, u16)) {
        (self.preview_hold, self.preview_next)
    }

    /// Reserve the cell boxes the pixel HOLD/NEXT previews need.
    pub fn with_preview_cells(mut self, hold: (u16, u16), next: (u16, u16)) -> Self {
        self.preview_hold = (hold.0.max(1), hold.1.max(1));
        self.preview_next = (next.0.max(1), next.1.max(1));
        self
    }

    /// Reserve an explicit cell box for the well.
    ///
    /// The pixel backend calls this so the frame ends up as close as possible to
    /// `10 x 20` square minos instead of `10 x 20` tall ones.
    pub fn with_well_cells(mut self, well_cells: (u16, u16)) -> Self {
        self.well_cells = (well_cells.0.max(1), well_cells.1.max(1));
        self
    }

    /// Terminal cells reserved for the well interior.
    pub fn well_cells(&self) -> (u16, u16) {
        self.well_cells
    }

    pub fn with_anchor_y(mut self, anchor_y: AnchorY) -> Self {
        self.anchor_y = anchor_y;
        self
    }

    /// Size a mino from the pixel dimensions of one terminal cell.
    pub fn from_cell_pixels(cell_px_w: u16, cell_px_h: u16) -> Self {
        let (cell_w, cell_h) = crate::term::cell_metrics::squarest_cell_size(cell_px_w, cell_px_h);
        Self::new(cell_w, cell_h)
    }

    pub fn cell_size(&self) -> (u16, u16) {
        (self.cell_w, self.cell_h)
    }

    /// Terminal cell of the well interior's top-left corner.
    ///
    /// Row/column offsets from this cell address individual board cells:
    /// `(bx, by)` maps to `(x + bx * cell_w, y + by * cell_h)`.
    pub fn well_origin(&self, viewport: Viewport) -> (u16, u16) {
        let frame_w = self.well_cells.0 + 2;
        let frame_h = self.well_cells.1 + 2;
        let start_x = viewport.width.saturating_sub(frame_w) / 2;
        let start_y = match self.anchor_y {
            AnchorY::Center => viewport.height.saturating_sub(frame_h) / 2,
            AnchorY::Top => 0,
        };
        (start_x + 1, start_y + 1)
    }

    /// Render the current game state into an existing framebuffer.
    ///
    /// This is the allocation-free hot path. Callers can reuse a framebuffer
    /// across frames and only resize when the terminal size changes.
    pub fn render_into(&self, snap: &GameSnapshot, viewport: Viewport, fb: &mut FrameBuffer) {
        self.render_into_with_overlay(snap, None, viewport, fb);
    }

    pub fn render_model_into(
        &self,
        model: &GameViewModel,
        viewport: Viewport,
        fb: &mut FrameBuffer,
    ) {
        self.render_into_with_overlay(model.snapshot(), model.overlay(), viewport, fb);
    }

    pub fn render_into_with_adapter(
        &self,
        snap: &GameSnapshot,
        adapter: Option<&AdapterStatusView>,
        viewport: Viewport,
        fb: &mut FrameBuffer,
    ) {
        let overlay = adapter.map(HudOverlay::from_adapter);
        self.render_into_with_overlay(snap, overlay.as_ref(), viewport, fb);
    }

    fn render_into_with_overlay(
        &self,
        snap: &GameSnapshot,
        overlay: Option<&HudOverlay>,
        viewport: Viewport,
        fb: &mut FrameBuffer,
    ) {
        fb.resize(viewport.width, viewport.height);
        fb.clear(
            CellStyle {
                fg: VALUE,
                bg: PAGE_BG,
                bold: false,
                dim: false,
            }
            .into_cell(' '),
        );

        let board_px_w = self.well_cells.0;
        let board_px_h = self.well_cells.1;
        let frame_w = board_px_w + 2;
        let frame_h = board_px_h + 2;

        let start_x = viewport.width.saturating_sub(frame_w) / 2;
        let start_y = match self.anchor_y {
            AnchorY::Center => viewport.height.saturating_sub(frame_h) / 2,
            AnchorY::Top => 0,
        };

        let well = CellStyle {
            fg: WELL_BG,
            bg: WELL_BG,
            bold: false,
            dim: false,
        };
        let clearing = snap.timers.line_clear_ms > 0;
        let rim = CellStyle {
            fg: if clearing { RIM_CLEAR } else { RIM },
            bg: PAGE_BG,
            bold: false,
            dim: false,
        };

        fb.fill_rect(start_x + 1, start_y + 1, board_px_w, board_px_h, ' ', well);
        self.draw_border(fb, start_x, start_y, frame_w, frame_h, rim);

        if start_y >= 1 && frame_w >= 6 {
            let title = CellStyle {
                fg: ACCENT,
                bg: PAGE_BG,
                bold: true,
                dim: false,
            };
            fb.put_str(
                start_x + (frame_w.saturating_sub(6)) / 2,
                start_y - 1,
                "TETRIS",
                title,
            );
        }

        // The well interior is left empty: minos are pixels, not character
        // cells. Everything around it stays text so the HUD uses the user's
        // font at full fidelity.
        self.draw_side_panel(fb, snap, viewport);
        if let Some(overlay) = overlay {
            self.draw_hud_overlay(fb, overlay, viewport);
        }

        self.draw_state_banner(fb, snap, start_x, start_y, frame_w, frame_h);
        self.draw_help_line(fb, viewport, start_y, frame_h);
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_state_banner(
        &self,
        fb: &mut FrameBuffer,
        snap: &GameSnapshot,
        start_x: u16,
        start_y: u16,
        frame_w: u16,
        frame_h: u16,
    ) {
        if snap.paused {
            self.draw_banner(
                fb, start_x, start_y, frame_w, frame_h, "PAUSED", None, "P resume",
            );
        } else if snap.game_over {
            self.draw_banner(
                fb,
                start_x,
                start_y,
                frame_w,
                frame_h,
                "GAME OVER",
                Some(snap.score),
                "R retry",
            );
        }
    }

    fn draw_help_line(&self, fb: &mut FrameBuffer, viewport: Viewport, start_y: u16, frame_h: u16) {
        if start_y.saturating_add(frame_h) >= viewport.height {
            return;
        }
        let help = CellStyle {
            fg: LABEL,
            bg: PAGE_BG,
            bold: false,
            dim: true,
        };
        fb.put_str(
            1,
            viewport.height - 1,
            "move rotate drop  C hold  P pause  R retry  Q quit",
            help,
        );
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

    /// Where the side panel's dynamic parts land, shared by the text pass and
    /// the pixel preview layers so both agree on the anchors.
    ///
    /// The previews are anchored to the *bottom* of the frame, so the stats and
    /// the combo rows above them never move them (a moved anchor would force the
    /// sprite layers to re-place themselves).
    pub fn panel_layout(&self, _snap: &GameSnapshot, viewport: Viewport) -> Option<PanelLayout> {
        let frame_w = self.well_cells.0 + 2;
        let frame_h = self.well_cells.1 + 2;
        let start_x = viewport.width.saturating_sub(frame_w) / 2;
        let start_y = match self.anchor_y {
            AnchorY::Center => viewport.height.saturating_sub(frame_h) / 2,
            AnchorY::Top => 0,
        };
        let x = start_x.saturating_add(frame_w).saturating_add(2);
        if x >= viewport.width || viewport.width - x < self.preview_next.0.max(1) {
            return None;
        }

        let hold_rows = self.preview_hold.1.max(1);
        let next_rows = self.preview_next.1.max(1);
        let next_y = start_y.saturating_add(frame_h.saturating_sub(next_rows));
        let hold_y = next_y.saturating_sub(hold_rows).saturating_sub(1);

        Some(PanelLayout {
            top: start_y,
            x,
            hold: (x, hold_y),
            next: (x, next_y),
            next_rows,
        })
    }

    /// Text part of the panel: stats, labels and the empty-hold dash.
    ///
    /// This is the layout the cell renderer used: stats stacked in the right
    /// column with blank separators, then HOLD and NEXT underneath. The pieces
    /// themselves are pixel layers (see `PreviewRenderer`).
    fn draw_side_panel(&self, fb: &mut FrameBuffer, snap: &GameSnapshot, viewport: Viewport) {
        let Some(layout) = self.panel_layout(snap, viewport) else {
            return;
        };
        let label = CellStyle {
            fg: LABEL,
            bg: PAGE_BG,
            bold: true,
            dim: false,
        };
        let value = CellStyle {
            fg: VALUE,
            bg: PAGE_BG,
            bold: false,
            dim: false,
        };
        let accent = CellStyle {
            fg: ACCENT,
            bg: PAGE_BG,
            bold: true,
            dim: false,
        };
        let x = layout.x;
        // Everything above the HOLD label belongs to the stats block.
        let limit = layout.hold.1.saturating_sub(1);
        let row = |offset: u16| layout.top.saturating_add(offset);
        let put = |fb: &mut FrameBuffer, offset: u16, text: &str, number: u32, style: CellStyle| {
            let y = row(offset);
            if y < limit {
                fb.put_str(x, y, text, style);
                fb.put_u32(x + 6, y, number, style);
            }
        };

        // SCORE keeps the cell renderer's shape: label, then its value below.
        if row(0) < limit {
            fb.put_str(x, row(0), "SCORE", label);
        }
        if row(1) < limit {
            fb.put_u32(x, row(1), snap.score, value);
        }
        put(fb, 3, "LEVEL", snap.level.saturating_add(1), label);
        put(fb, 4, "LINES", snap.lines, label);
        if snap.combo > 0 {
            put(fb, 6, "COMBO", snap.combo as u32, accent);
        }
        if snap.back_to_back && row(7) < limit {
            fb.put_str(x, row(7), "B2B", accent);
        }

        fb.put_str(x, layout.hold.1.saturating_sub(1), "HOLD", label);
        if snap.hold.is_none() {
            fb.put_str(layout.hold.0, layout.hold.1, "-", value);
        }
        fb.put_str(x, layout.next.1.saturating_sub(1), "NEXT", label);
    }

    fn draw_hud_overlay(&self, fb: &mut FrameBuffer, overlay: &HudOverlay, viewport: Viewport) {
        if viewport.width < 10 || viewport.height == 0 {
            return;
        }
        let label = CellStyle {
            fg: LABEL,
            bg: PAGE_BG,
            bold: false,
            dim: true,
        };
        let value = CellStyle {
            fg: VALUE,
            bg: PAGE_BG,
            bold: false,
            dim: false,
        };
        let value_x = HUD_LABEL_MAX as u16 + 1;

        for (i, row) in overlay.rows.iter().enumerate() {
            let y = i as u16;
            if y >= viewport.height {
                return;
            }
            fb.put_str(0, y, row.label(), label);
            match row.value {
                HudOverlayValue::U32(n) => fb.put_u32(value_x, y, n, value),
                HudOverlayValue::Dash => fb.put_str(value_x, y, "-", value),
                HudOverlayValue::Text { bytes, len } => {
                    if let Ok(text) = std::str::from_utf8(&bytes[..len as usize]) {
                        fb.put_str(value_x, y, text, value);
                    }
                }
            }
        }
    }

    #[allow(clippy::too_many_arguments)]
    fn draw_banner(
        &self,
        fb: &mut FrameBuffer,
        start_x: u16,
        start_y: u16,
        frame_w: u16,
        frame_h: u16,
        title: &str,
        score: Option<u32>,
        hint: &str,
    ) {
        let title_w = title.len() as u16;
        let hint_w = hint.len() as u16;
        let score_w = score.map(digit_count).unwrap_or(0);
        let inner = title_w.max(hint_w).max(score_w);
        let box_w = inner.saturating_add(4).max(8);
        let box_h = if score.is_some() { 5 } else { 4 };
        let x = start_x.saturating_add(frame_w.saturating_sub(box_w) / 2);
        let y = start_y.saturating_add(frame_h.saturating_sub(box_h) / 2);
        let fill = CellStyle {
            fg: VALUE,
            bg: OVERLAY_BG,
            bold: false,
            dim: false,
        };
        let border = CellStyle {
            fg: ACCENT,
            bg: OVERLAY_BG,
            bold: true,
            dim: false,
        };
        let title_style = CellStyle {
            fg: VALUE,
            bg: OVERLAY_BG,
            bold: true,
            dim: false,
        };
        let hint_style = CellStyle {
            fg: LABEL,
            bg: OVERLAY_BG,
            bold: false,
            dim: false,
        };

        fb.fill_rect(x, y, box_w, box_h, ' ', fill);
        self.draw_border(fb, x, y, box_w, box_h, border);
        fb.put_str(
            x + (box_w.saturating_sub(title_w)) / 2,
            y + 1,
            title,
            title_style,
        );
        if let Some(score) = score {
            fb.put_u32(
                x + (box_w.saturating_sub(score_w)) / 2,
                y + 2,
                score,
                title_style,
            );
            fb.put_str(
                x + (box_w.saturating_sub(hint_w)) / 2,
                y + 3,
                hint,
                hint_style,
            );
        } else {
            fb.put_str(
                x + (box_w.saturating_sub(hint_w)) / 2,
                y + 2,
                hint,
                hint_style,
            );
        }
    }
}

pub(crate) fn piece_from_cell(v: u8) -> Option<PieceKind> {
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

pub(crate) fn piece_color(kind: PieceKind) -> Rgb {
    match kind {
        PieceKind::I => Rgb::new(79, 209, 196),
        PieceKind::O => Rgb::new(245, 224, 94),
        PieceKind::T => Rgb::new(158, 122, 235),
        PieceKind::S => Rgb::new(105, 212, 145),
        PieceKind::Z => Rgb::new(252, 130, 130),
        PieceKind::J => Rgb::new(99, 178, 237),
        PieceKind::L => Rgb::new(245, 173, 84),
    }
}

fn digit_count(mut v: u32) -> u16 {
    if v == 0 {
        return 1;
    }
    let mut n = 0;
    while v != 0 {
        n += 1;
        v /= 10;
    }
    n
}

trait IntoCell {
    fn into_cell(self, ch: char) -> crate::term::fb::Cell;
}

impl IntoCell for CellStyle {
    fn into_cell(self, ch: char) -> crate::term::fb::Cell {
        crate::term::fb::Cell { ch, style: self }
    }
}
