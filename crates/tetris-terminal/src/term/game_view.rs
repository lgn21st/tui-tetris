//! GameView: maps an immutable `GameSnapshot` into a terminal framebuffer.
//!
//! This module is pure (no I/O). It can be unit-tested.

use crate::term::fb::{CellStyle, FrameBuffer, Rgb};
use tetris_core::core::{GameSnapshot, get_shape};
use tetris_core::types::{BOARD_HEIGHT, BOARD_WIDTH, PieceKind, Rotation};

const PAGE_BG: Rgb = Rgb::new(16, 16, 16);
const WELL_BG: Rgb = Rgb::new(28, 28, 28);
const RIM: Rgb = Rgb::new(46, 46, 46);
const RIM_CLEAR: Rgb = Rgb::new(72, 72, 72);
const GHOST: Rgb = Rgb::new(42, 42, 42);
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

    /// Size a mino from the pixel dimensions of one terminal cell.
    pub fn from_cell_pixels(cell_px_w: u16, cell_px_h: u16) -> Self {
        let (cell_w, cell_h) = crate::term::cell_metrics::squarest_cell_size(cell_px_w, cell_px_h);
        Self::new(cell_w, cell_h)
    }

    pub fn cell_size(&self) -> (u16, u16) {
        (self.cell_w, self.cell_h)
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

        let board_px_w = (BOARD_WIDTH as u16) * self.cell_w;
        let board_px_h = (BOARD_HEIGHT as u16) * self.cell_h;
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

        for y in 0..BOARD_HEIGHT as u16 {
            for x in 0..BOARD_WIDTH as u16 {
                let cell = snap.board[y as usize][x as usize];
                if let Some(kind) = piece_from_cell(cell) {
                    self.draw_board_cell(fb, start_x, start_y, x, y, kind, true);
                }
            }
        }

        if let (Some(active), Some(ghost_y)) = (snap.active, snap.ghost_y)
            && ghost_y != active.y
        {
            let ghost_style = CellStyle {
                fg: GHOST,
                bg: GHOST,
                bold: false,
                dim: false,
            };
            for &(dx, dy) in get_shape(active.kind, active.rotation).iter() {
                let x = active.x + dx;
                let y = ghost_y + dy;
                if x >= 0 && x < BOARD_WIDTH as i8 && y >= 0 && y < BOARD_HEIGHT as i8 {
                    self.fill_cell_rect(fb, start_x, start_y, x as u16, y as u16, ' ', ghost_style);
                }
            }
        }

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

        self.draw_side_panel(fb, snap, viewport, start_x, start_y, frame_w);
        if let Some(overlay) = overlay {
            self.draw_hud_overlay(fb, overlay, viewport);
        }

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

        if start_y.saturating_add(frame_h) < viewport.height {
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

    #[allow(clippy::too_many_arguments)] // Flat scalar arguments keep this render hot path allocation-free.
    fn draw_board_cell(
        &self,
        fb: &mut FrameBuffer,
        start_x: u16,
        start_y: u16,
        x: u16,
        y: u16,
        kind: PieceKind,
        _bold: bool,
    ) {
        // Paint with matching fg/bg spaces. Glyphs like '█' rarely fill a
        // terminal cell and may be fullwidth, which stretches every mino.
        let color = piece_color(kind);
        let style = CellStyle {
            fg: color,
            bg: color,
            bold: false,
            dim: false,
        };
        self.fill_cell_rect(fb, start_x, start_y, x, y, ' ', style);
    }

    #[allow(clippy::too_many_arguments)] // Flat scalar arguments keep this render hot path allocation-free.
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

    fn preview_cell_w(&self) -> u16 {
        (self.cell_w / 2).max(1)
    }

    fn preview_cell_h(&self) -> u16 {
        (self.cell_h / 2).max(1)
    }

    fn draw_side_panel(
        &self,
        fb: &mut FrameBuffer,
        snap: &GameSnapshot,
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

        let mut y = start_y;
        fb.put_str(panel_x, y, "SCORE", label);
        y = y.saturating_add(1);
        fb.put_u32(panel_x, y, snap.score, value);
        y = y.saturating_add(2);

        fb.put_str(panel_x, y, "LEVEL", label);
        fb.put_u32(panel_x + 6, y, snap.level.saturating_add(1), value);
        y = y.saturating_add(1);
        fb.put_str(panel_x, y, "LINES", label);
        fb.put_u32(panel_x + 6, y, snap.lines, value);
        y = y.saturating_add(2);

        if snap.combo > 0 {
            fb.put_str(panel_x, y, "COMBO", accent);
            fb.put_u32(panel_x + 6, y, snap.combo as u32, accent);
            y = y.saturating_add(1);
        }
        if snap.back_to_back {
            fb.put_str(panel_x, y, "B2B", accent);
            y = y.saturating_add(1);
        }
        if snap.combo > 0 || snap.back_to_back {
            y = y.saturating_add(1);
        }

        fb.put_str(panel_x, y, "HOLD", label);
        y = y.saturating_add(1);
        if let Some(kind) = snap.hold {
            y = y.saturating_add(self.draw_mini_piece(fb, panel_x, y, kind, !snap.can_hold));
        } else {
            fb.put_str(panel_x, y, "-", value);
            y = y.saturating_add(1);
        }
        y = y.saturating_add(1);

        fb.put_str(panel_x, y, "NEXT", label);
        y = y.saturating_add(1);
        for kind in snap.next_queue.iter() {
            let height = self.mini_term_height(*kind);
            if y.saturating_add(height) >= viewport.height {
                break;
            }
            y = y.saturating_add(self.draw_mini_piece(fb, panel_x, y, *kind, false));
            y = y.saturating_add(1);
        }
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

fn piece_color(kind: PieceKind) -> Rgb {
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

fn mix_rgb(a: Rgb, b: Rgb) -> Rgb {
    mix_rgb_n(a, b, 1, 1)
}

fn mix_rgb_n(a: Rgb, b: Rgb, a_w: u16, b_w: u16) -> Rgb {
    let t = a_w.saturating_add(b_w).max(1);
    Rgb::new(
        ((u16::from(a.r) * a_w + u16::from(b.r) * b_w) / t) as u8,
        ((u16::from(a.g) * a_w + u16::from(b.g) * b_w) / t) as u8,
        ((u16::from(a.b) * a_w + u16::from(b.b) * b_w) / t) as u8,
    )
}

fn mini_height(kind: PieceKind) -> u16 {
    let shape = get_shape(kind, Rotation::North);
    let mut min_y = i8::MAX;
    let mut max_y = i8::MIN;
    for &(_, dy) in &shape {
        min_y = min_y.min(dy);
        max_y = max_y.max(dy);
    }
    (max_y - min_y + 1) as u16
}

impl GameView {
    fn packs_preview_half_blocks(&self) -> bool {
        self.cell_h == 1 && self.cell_w >= 2
    }

    fn mini_term_height(&self, kind: PieceKind) -> u16 {
        let rows = mini_height(kind);
        if self.packs_preview_half_blocks() {
            rows.div_ceil(2)
        } else {
            rows.saturating_mul(self.preview_cell_h())
        }
    }

    fn draw_mini_piece(
        &self,
        fb: &mut FrameBuffer,
        origin_x: u16,
        origin_y: u16,
        kind: PieceKind,
        dim: bool,
    ) -> u16 {
        let shape = get_shape(kind, Rotation::North);
        let mut min_x = i8::MAX;
        let mut min_y = i8::MAX;
        let mut max_x = i8::MIN;
        let mut max_y = i8::MIN;
        for &(dx, dy) in &shape {
            min_x = min_x.min(dx);
            min_y = min_y.min(dy);
            max_x = max_x.max(dx);
            max_y = max_y.max(dy);
        }
        let mut color = piece_color(kind);
        if dim {
            color = mix_rgb(color, PAGE_BG);
        }
        let fill = CellStyle {
            fg: color,
            bg: color,
            bold: false,
            dim: false,
        };

        if self.packs_preview_half_blocks() {
            let width = (max_x - min_x + 1) as usize;
            let height = (max_y - min_y + 1) as usize;
            let mut occ = [[false; 4]; 4];
            for &(dx, dy) in &shape {
                occ[(dy - min_y) as usize][(dx - min_x) as usize] = true;
            }
            // Odd-height pieces sit on the lower half so they share a baseline with O.
            let pad = height % 2;
            let term_rows = ((height + pad) / 2) as u16;
            let half = CellStyle {
                fg: color,
                bg: PAGE_BG,
                bold: false,
                dim: false,
            };
            let row_at = |y: isize| -> [bool; 4] {
                if y < 0 {
                    return [false; 4];
                }
                let y = y as usize;
                if y >= height { [false; 4] } else { occ[y] }
            };
            for tr in 0..term_rows as usize {
                let ups = row_at((tr as isize) * 2 - pad as isize);
                let downs = row_at((tr as isize) * 2 - pad as isize + 1);
                for (mx, (&up, &down)) in ups.iter().zip(downs.iter()).take(width).enumerate() {
                    let px = origin_x.saturating_add(mx as u16);
                    let py = origin_y.saturating_add(tr as u16);
                    match (up, down) {
                        (true, true) => fb.put_char(px, py, ' ', fill),
                        (true, false) => fb.put_char(px, py, '▀', half),
                        (false, true) => fb.put_char(px, py, '▄', half),
                        (false, false) => {}
                    }
                }
            }
            return term_rows;
        }

        let cell_w = self.preview_cell_w();
        let cell_h = self.preview_cell_h();
        for &(dx, dy) in &shape {
            let px = origin_x.saturating_add((dx - min_x) as u16 * cell_w);
            let py = origin_y.saturating_add((dy - min_y) as u16 * cell_h);
            fb.fill_rect(px, py, cell_w, cell_h, ' ', fill);
        }
        ((max_y - min_y + 1) as u16).saturating_mul(cell_h)
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
