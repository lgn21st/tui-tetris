//! Layered pixel renderer for the well.
//!
//! The playfield is pixel-native: no board cell is ever drawn as a character.
//! Two images, both sized to the well's device-pixel box and anchored at the
//! same cell, compose the picture:
//!
//! | layer | z | format | content | retransmitted when |
//! |-------|---|--------|---------|--------------------|
//! | board | 0 | RGB | well background + settled minos | the settled board changes |
//! | motion | 1 | RGBA | falling piece, drop shadow, ghost outline, lock tint, line-clear flash, pause dim | any of those changes |
//!
//! Splitting by change frequency is what makes per-frame animation affordable:
//! the opaque board is re-sent only on a lock or a clear, while the mostly
//! transparent motion layer deflates to a few hundred bytes per frame.
//!
//! Both images are placed with `c=`/`r=` set to the well's cell box, and their
//! pixel size is derived from the window's real pixel size, so the scale factor
//! is ~1 and minos stay exactly square.

use anyhow::Result;
use arrayvec::ArrayVec;

use tetris_core::core::{GameSnapshot, get_shape};
use tetris_core::types::{BOARD_HEIGHT, BOARD_WIDTH, PieceKind, Rotation};

use crate::term::fb::Rgb;
use crate::term::game_view::{piece_color, piece_from_cell};
use crate::term::kitty::apc::KittyEscapeWriter;
use crate::term::kitty::canvas::{PixelCanvas, PixelFormat};
use crate::term::kitty::capability::KittySupport;

/// Background of the well interior (top of the gradient). Kept public because
/// the cell renderer still paints the well backdrop with it before the first
/// image arrives.
pub const WELL_BG: Rgb = Rgb::new(28, 28, 28);
const WELL_BG_BOTTOM: Rgb = Rgb::new(34, 34, 36);
const WELL_INNER_EDGE: Rgb = Rgb::new(18, 18, 18);
const WHITE: Rgb = Rgb::new(255, 255, 255);
const BLACK: Rgb = Rgb::new(0, 0, 0);

// One visual language for everything on the board: a falling piece and the same
// piece after it locks must be pixel identical, so there is a single pair of
// bevel weights and it is only ever drawn on a *region* boundary.
const BEVEL_LIGHT: u8 = 82;
const BEVEL_DARK: u8 = 96;
/// Soft enough to stay a shadow: at 118 a black edge reads as depth instead of
/// a black outline, and it no longer scars the stack below the piece.
const SHADOW_ALPHA: u8 = 118;
const SHADOW_OFFSET: (u32, u32) = (1, 2);
/// The ghost is a faint silhouette with a slightly denser rim, so it reads as a
/// hint rather than another block.
const GHOST_FILL_ALPHA: u8 = 30;
const GHOST_EDGE_ALPHA: u8 = 86;
/// How much of the piece colour survives in the ghost tint (`blend` weights its
/// first argument by this value).
const GHOST_COLOR_WEIGHT: u8 = 215;
const DIM_ALPHA: u8 = 150;
const FLASH_ALPHA: u8 = 96;
const LOCK_TINT_ALPHA: u8 = 84;
const FLASH_BUCKETS: u32 = 4;

/// How many NEXT previews the HUD shows.
const NEXT_SLOTS: usize = 5;

const IMAGE_BOARD: u32 = 0x7454_5253;
const IMAGE_MOTION: u32 = 0x7454_5254;
const PLACEMENT: u32 = 1;

/// A grid of square mino cells inside one layer image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Grid {
    /// Top-left pixel of the grid inside the layer.
    pub(crate) origin: (u32, u32),
    /// Pixel size of one cell band.
    pub(crate) band: (u32, u32),
}

impl Grid {
    pub(crate) const fn new(origin: (u32, u32), band: (u32, u32)) -> Self {
        Self { origin, band }
    }

    /// Top-left pixel of one cell.
    pub(crate) fn cell(&self, x: u16, y: u16) -> (u32, u32) {
        (
            self.origin.0 + u32::from(x) * self.band.0,
            self.origin.1 + u32::from(y) * self.band.1,
        )
    }
}

/// Geometry of the well: the terminal cell box the frame reserved and the
/// device-pixel box the layer images fill.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct WellGeometry {
    box_cells: (u16, u16),
    box_px: (u32, u32),
}

impl WellGeometry {
    pub fn new(box_cells: (u16, u16), box_px: (u32, u32)) -> Self {
        Self {
            box_cells: (box_cells.0.max(1), box_cells.1.max(1)),
            box_px: (box_px.0.max(1), box_px.1.max(1)),
        }
    }

    /// Terminal cell box covered by the well, used for `c=`/`r=`.
    pub fn cells(&self) -> (u16, u16) {
        self.box_cells
    }

    /// Layer image size in device pixels.
    pub fn pixels(&self) -> (u32, u32) {
        self.box_px
    }

    /// Pixel rectangle `(x, y, width, height)` of one board cell.
    ///
    /// Bands tile the box, so the grid spans it exactly and the bottom row
    /// touches the frame.
    pub fn cell_rect(&self, x: u16, y: u16) -> (u32, u32, u32, u32) {
        let grid = self.grid();
        let (x0, y0) = grid.cell(x, y);
        (x0, y0, grid.band.0, grid.band.1)
    }

    fn grid(&self) -> Grid {
        let band = (
            self.box_px.0 / BOARD_WIDTH as u32,
            self.box_px.1 / BOARD_HEIGHT as u32,
        );
        let origin = (
            (self.box_px.0 - band.0 * BOARD_WIDTH as u32) / 2,
            (self.box_px.1 - band.1 * BOARD_HEIGHT as u32) / 2,
        );
        Grid::new(origin, band)
    }

    fn salt(&self) -> u64 {
        let mut hash = FNV_OFFSET;
        for value in [
            u64::from(self.box_cells.0),
            u64::from(self.box_cells.1),
            u64::from(self.box_px.0),
            u64::from(self.box_px.1),
        ] {
            hash = fnv_step(hash, value);
        }
        hash
    }
}

/// Hash of everything in the settled board layer.
pub fn board_hash(snap: &GameSnapshot) -> u64 {
    let mut hash = FNV_OFFSET;
    for row in snap.board.iter() {
        for cell in row.iter() {
            hash = fnv_step(hash, u64::from(*cell));
        }
    }
    hash
}

/// Hash of everything in the motion layer.
pub fn motion_hash(snap: &GameSnapshot, geometry: WellGeometry) -> u64 {
    let mut hash = FNV_OFFSET ^ geometry.salt();
    match snap.active {
        Some(active) => {
            hash = fnv_step(hash, 1);
            hash = fnv_step(hash, u64::from(kind_code(active.kind)));
            hash = fnv_step(hash, u64::from(rotation_code(active.rotation)));
            hash = fnv_step(hash, u64::from(active.x as u8));
            hash = fnv_step(hash, u64::from(active.y as u8));
        }
        None => hash = fnv_step(hash, 0),
    }
    match snap.ghost_y {
        Some(y) => hash = fnv_step(hash, u64::from(y as u8) + 1),
        None => hash = fnv_step(hash, 0),
    }
    // The lock timer drives the tint, but one step per 64 ms is plenty.
    hash = fnv_step(hash, u64::from(snap.timers.lock_ms / 64));
    hash = fnv_step(hash, u64::from(flash_bucket(snap.timers.line_clear_ms)));
    hash = fnv_step(hash, u64::from(snap.paused));
    hash = fnv_step(hash, u64::from(snap.game_over));
    hash
}

fn flash_bucket(line_clear_ms: u32) -> u32 {
    if line_clear_ms == 0 {
        0
    } else {
        (line_clear_ms.saturating_sub(1) / (180 / FLASH_BUCKETS) + 1).min(FLASH_BUCKETS)
    }
}

fn lock_tint(lock_ms: u32) -> u8 {
    // 0..=500 ms of lock delay maps onto 0..=LOCK_TINT_ALPHA.
    (u32::from(LOCK_TINT_ALPHA) * lock_ms.min(500) / 500) as u8
}

fn kind_code(kind: PieceKind) -> u8 {
    match kind {
        PieceKind::I => 1,
        PieceKind::O => 2,
        PieceKind::T => 3,
        PieceKind::S => 4,
        PieceKind::Z => 5,
        PieceKind::J => 6,
        PieceKind::L => 7,
    }
}

fn rotation_code(rotation: tetris_core::types::Rotation) -> u8 {
    match rotation {
        tetris_core::types::Rotation::North => 0,
        tetris_core::types::Rotation::East => 1,
        tetris_core::types::Rotation::South => 2,
        tetris_core::types::Rotation::West => 3,
    }
}

const FNV_OFFSET: u64 = 0xcbf2_9ce4_8422_2325;
const FNV_PRIME: u64 = 0x0000_0100_0000_01b3;

#[inline]
fn fnv_step(hash: u64, value: u64) -> u64 {
    let mut hash = hash;
    for byte in value.to_le_bytes() {
        hash ^= u64::from(byte);
        hash = hash.wrapping_mul(FNV_PRIME);
    }
    hash
}

/// One image layer: pixels, wire ids, and what it was last rendered for.
struct Layer {
    image_id: u32,
    z: i32,
    canvas: PixelCanvas,
    /// Content hash plus a layout key (geometry salt or cell anchor).
    drawn: Option<(u64, u64)>,
}

impl Layer {
    fn new(image_id: u32, z: i32, width: u32, height: u32, format: PixelFormat) -> Self {
        Self {
            image_id,
            z,
            canvas: PixelCanvas::new(width, height, format),
            drawn: None,
        }
    }

    fn needs(&self, hash: u64, layout: u64) -> bool {
        self.drawn != Some((hash, layout))
    }

    fn mark(&mut self, hash: u64, layout: u64) {
        self.drawn = Some((hash, layout));
    }

    fn resize(&mut self, width: u32, height: u32) {
        self.canvas.resize(width, height);
    }
}

/// Owns the well layers and the wire encoder.
pub struct WellRenderer {
    writer: KittyEscapeWriter,
    board: Layer,
    motion: Layer,
    geometry: WellGeometry,
    /// Whether the terminal may still hold our images. Kept separate from each
    /// layer's `drawn` marker, which `invalidate` clears.
    live: bool,
}

impl WellRenderer {
    pub fn new(support: KittySupport, geometry: WellGeometry) -> Self {
        Self {
            writer: KittyEscapeWriter::new(support),
            board: Layer::new(
                IMAGE_BOARD,
                0,
                geometry.pixels().0,
                geometry.pixels().1,
                PixelFormat::Rgb,
            ),
            motion: Layer::new(
                IMAGE_MOTION,
                1,
                geometry.pixels().0,
                geometry.pixels().1,
                PixelFormat::Rgba,
            ),
            geometry,
            live: false,
        }
    }

    pub fn geometry(&self) -> WellGeometry {
        self.geometry
    }

    pub fn support(&self) -> KittySupport {
        self.writer.support()
    }

    /// Board layer pixels (tests and diagnostics).
    pub fn board(&self) -> &PixelCanvas {
        &self.board.canvas
    }

    /// Motion layer pixels (tests and diagnostics).
    pub fn motion(&self) -> &PixelCanvas {
        &self.motion.canvas
    }

    /// Repaint and emit whichever layers changed. Returns whether bytes were
    /// emitted into `out`.
    pub fn sync(
        &mut self,
        snap: &GameSnapshot,
        geometry: WellGeometry,
        cursor: (u16, u16),
        out: &mut Vec<u8>,
    ) -> Result<bool> {
        if geometry != self.geometry {
            let (width, height) = geometry.pixels();
            self.board.resize(width, height);
            self.motion.resize(width, height);
            self.board.drawn = None;
            self.motion.drawn = None;
            self.geometry = geometry;
        }
        let layout = geometry.salt();

        let mut emitted = false;

        let hash = board_hash(snap);
        if self.board.needs(hash, layout) {
            paint_board(&mut self.board.canvas, snap, geometry);
            self.writer.transmit_and_place(
                &self.board.canvas,
                self.board.image_id,
                PLACEMENT,
                self.board.z,
                geometry,
                cursor,
                out,
            )?;
            self.board.mark(hash, layout);
            emitted = true;
        }

        let hash = motion_hash(snap, geometry);
        if self.motion.needs(hash, layout) {
            paint_motion(&mut self.motion.canvas, snap, geometry);
            self.writer.transmit_and_place(
                &self.motion.canvas,
                self.motion.image_id,
                PLACEMENT,
                self.motion.z,
                geometry,
                cursor,
                out,
            )?;
            self.motion.mark(hash, layout);
            emitted = true;
        }

        if emitted {
            self.live = true;
        }
        Ok(emitted)
    }

    /// Force both layers to be retransmitted on the next `sync`.
    ///
    /// A full-screen text redraw emits `CSI 2J`, which also clears images.
    pub fn invalidate(&mut self) {
        self.board.drawn = None;
        self.motion.drawn = None;
    }

    /// Remove both images from the terminal. Safe to call more than once.
    pub fn shutdown(&mut self, out: &mut Vec<u8>) {
        if !self.live {
            return;
        }
        self.writer.delete_all(out);
        self.live = false;
        self.board.drawn = None;
        self.motion.drawn = None;
    }
}

fn paint_board(canvas: &mut PixelCanvas, snap: &GameSnapshot, geometry: WellGeometry) {
    paint_backdrop(canvas, geometry);

    let mut cells = ArrayVec::<_, 200>::new();
    for y in 0..BOARD_HEIGHT {
        for x in 0..BOARD_WIDTH {
            if piece_from_cell(snap.board[y as usize][x as usize]).is_some() {
                let _ = cells.try_push((x as i8, y as i8));
            }
        }
    }
    if cells.is_empty() {
        return;
    }
    let color_at = |x: i8, y: i8| {
        if !in_bounds(x, y) {
            return None;
        }
        piece_from_cell(snap.board[y as usize][x as usize]).map(piece_color)
    };
    paint_regions(canvas, geometry.grid(), &cells, color_at, 255, 255);
}

/// Well interior: a barely-there vertical gradient plus an inset edge line.
///
/// A flat fill looked like dead space next to the beveled blocks. Each row is a
/// constant colour, so the gradient still deflates to almost nothing.
fn paint_backdrop(canvas: &mut PixelCanvas, geometry: WellGeometry) {
    let (width, height) = geometry.pixels();
    for y in 0..height {
        let t = if height > 1 {
            (y * 255 / (height - 1)) as u16
        } else {
            0
        };
        let mix = |a: u8, b: u8| ((u16::from(a) * (255 - t) + u16::from(b) * t) / 255) as u8;
        let row = Rgb::new(
            mix(WELL_BG.r, WELL_BG_BOTTOM.r),
            mix(WELL_BG.g, WELL_BG_BOTTOM.g),
            mix(WELL_BG.b, WELL_BG_BOTTOM.b),
        );
        canvas.fill_rect(0, y, width, 1, row);
    }
    canvas.fill_rect(0, 0, width, 1, WELL_INNER_EDGE);
    canvas.fill_rect(0, 0, 1, height, WELL_INNER_EDGE);
    if width > 1 {
        canvas.fill_rect(width - 1, 0, 1, height, WELL_INNER_EDGE);
    }
    if height > 1 {
        canvas.fill_rect(0, height - 1, width, 1, WELL_INNER_EDGE);
    }
}

fn paint_motion(canvas: &mut PixelCanvas, snap: &GameSnapshot, geometry: WellGeometry) {
    canvas.clear();
    let grid = geometry.grid();
    if grid.band.0 == 0 || grid.band.1 == 0 {
        return;
    }

    let mut cells = ArrayVec::<_, 4>::new();
    if let Some(active) = snap.active {
        for &(dx, dy) in get_shape(active.kind, active.rotation).iter() {
            let (x, y) = (active.x + dx, active.y + dy);
            if in_bounds(x, y) {
                let _ = cells.try_push((x, y));
            }
        }
    }
    let inside = |x: i8, y: i8| cells.contains(&(x, y));

    if let (Some(active), Some(ghost_y)) = (snap.active, snap.ghost_y)
        && ghost_y != active.y
    {
        let mut ghost = ArrayVec::<_, 4>::new();
        for &(dx, dy) in get_shape(active.kind, active.rotation).iter() {
            let (x, y) = (active.x + dx, ghost_y + dy);
            if in_bounds(x, y) {
                let _ = ghost.try_push((x, y));
            }
        }
        let ghost_color = blend(piece_color(active.kind), WHITE, GHOST_COLOR_WEIGHT);
        paint_regions(
            canvas,
            geometry.grid(),
            &ghost,
            |x, y| ghost.contains(&(x, y)).then_some(ghost_color),
            GHOST_FILL_ALPHA,
            GHOST_EDGE_ALPHA,
        );
    }

    if let Some(active) = snap.active {
        let color = piece_color(active.kind);
        // An offset drop shadow sells the piece as a physical block above the
        // board instead of a flat fill.
        for &(x, y) in cells.iter() {
            let (mx, my) = grid.cell(x as u16, y as u16);
            canvas.blend_rect(
                mx + SHADOW_OFFSET.0,
                my + SHADOW_OFFSET.1,
                grid.band.0,
                grid.band.1,
                BLACK,
                SHADOW_ALPHA,
            );
        }
        // The falling piece goes through exactly the same painter as a locked
        // one, so nothing changes visually when it settles.
        paint_regions(
            canvas,
            geometry.grid(),
            &cells,
            |x, y| inside(x, y).then_some(color),
            255,
            255,
        );
        // Lock delay becomes visible: the piece whitens as it settles.
        let tint = lock_tint(snap.timers.lock_ms);
        if tint > 0 {
            for &(x, y) in cells.iter() {
                let (mx, my) = grid.cell(x as u16, y as u16);
                canvas.blend_rect(mx, my, grid.band.0, grid.band.1, WHITE, tint);
            }
        }
    }

    if snap.timers.line_clear_ms > 0 {
        let alpha =
            u32::from(FLASH_ALPHA) * flash_bucket(snap.timers.line_clear_ms) / FLASH_BUCKETS;
        for (y, row) in snap.board.iter().enumerate() {
            if row.iter().all(|cell| *cell != 0) {
                // Flash the whole band so a cleared row reads as one line.
                let (x0, y0) = grid.cell(0, y as u16);
                canvas.blend_rect(
                    x0,
                    y0,
                    grid.band.0 * BOARD_WIDTH as u32,
                    grid.band.1,
                    WHITE,
                    alpha as u8,
                );
            }
        }
    }

    if snap.paused || snap.game_over {
        canvas.blend_all(BLACK, DIM_ALPHA);
    }
}

/// Paint a region of cells: fill every cell edge to edge, then draw the two-tone
/// rim on the region's outline only.
///
/// Inside a region there is no line at all, so a tetromino reads as one block
/// instead of four tiles, and a locked piece looks exactly like it did while
/// falling.
pub(crate) fn paint_regions(
    canvas: &mut PixelCanvas,
    grid: Grid,
    cells: &[(i8, i8)],
    color_at: impl Fn(i8, i8) -> Option<Rgb>,
    fill_alpha: u8,
    edge_alpha: u8,
) {
    let (band_w, band_h) = grid.band;
    for &(x, y) in cells {
        let Some(color) = color_at(x, y) else {
            continue;
        };
        let (mx, my) = grid.cell(x as u16, y as u16);
        canvas.blend_rect(mx, my, band_w, band_h, color, fill_alpha);
    }
    for &(x, y) in cells {
        let Some(color) = color_at(x, y) else {
            continue;
        };
        let (mx, my) = grid.cell(x as u16, y as u16);
        let light = blend(color, WHITE, BEVEL_LIGHT);
        let dark = blend(color, BLACK, BEVEL_DARK);
        if color_at(x, y - 1) != Some(color) {
            canvas.blend_rect(mx, my, band_w, 1, light, edge_alpha);
        }
        if color_at(x - 1, y) != Some(color) {
            canvas.blend_rect(mx, my, 1, band_h, light, edge_alpha);
        }
        if color_at(x, y + 1) != Some(color) {
            canvas.blend_rect(mx, my + band_h - 1, band_w, 1, dark, edge_alpha);
        }
        if color_at(x + 1, y) != Some(color) {
            canvas.blend_rect(mx + band_w - 1, my, 1, band_h, dark, edge_alpha);
        }
    }
}

fn in_bounds(x: i8, y: i8) -> bool {
    x >= 0 && x < BOARD_WIDTH as i8 && y >= 0 && y < BOARD_HEIGHT as i8
}

fn blend(from: Rgb, to: Rgb, alpha: u8) -> Rgb {
    let a = u16::from(alpha);
    let inv = 255 - a;
    let mix = |x: u8, y: u8| ((u16::from(x) * a + u16::from(y) * inv) / 255) as u8;
    Rgb::new(mix(from.r, to.r), mix(from.g, to.g), mix(from.b, to.b))
}

/// Widest tetromino in minos: the preview strip is this many minos across.
pub const PREVIEW_COLS: u32 = 4;
/// Tallest tetromino in minos.
const PREVIEW_ROWS: u32 = 2;

const IMAGE_HOLD: u32 = 0x7454_5255;
const IMAGE_NEXT: u32 = 0x7454_5256;

/// Renders the HOLD and NEXT previews as pixel layers.
///
/// A preview mino is **half a board mino** - the same proportion the half-block
/// character previews had - but derived from the terminal's device pixel size, so
/// the previews scale with the display instead of shrinking on a HiDPI screen.
/// A blank half-slot between pieces keeps the airy rhythm of the cell renderer.
pub struct PreviewRenderer {
    writer: KittyEscapeWriter,
    /// Device pixels per preview mino.
    mino: u32,
    /// Device pixels between two preview slots.
    gap: u32,
    hold: Layer,
    next: Layer,
    live: bool,
}

impl PreviewRenderer {
    /// `board_mino` is the device-pixel side of one playfield mino.
    pub fn new(support: KittySupport, board_mino: u32) -> Self {
        let mino = (board_mino / 2).max(4);
        // One blank half slot between pieces: the cell renderer used a whole
        // text row, which is exactly this at typical cell aspect ratios.
        let gap = mino * 2;
        let hold_size = (PREVIEW_COLS * mino, PREVIEW_ROWS * mino);
        let next_size = (
            PREVIEW_COLS * mino,
            NEXT_SLOTS as u32 * (PREVIEW_ROWS * mino + gap) - gap,
        );
        Self {
            writer: KittyEscapeWriter::new(support),
            mino,
            gap,
            hold: Layer::new(IMAGE_HOLD, 1, hold_size.0, hold_size.1, PixelFormat::Rgba),
            next: Layer::new(IMAGE_NEXT, 1, next_size.0, next_size.1, PixelFormat::Rgba),
            live: false,
        }
    }

    /// Device pixels per preview mino.
    pub fn mino(&self) -> u32 {
        self.mino
    }

    /// Cell box the HUD must reserve for the HOLD preview.
    pub fn hold_cell_box(&self, cell_px: (u16, u16)) -> (u16, u16) {
        cell_box(self.hold.canvas.width(), self.hold.canvas.height(), cell_px)
    }

    /// Cell box the HUD must reserve for the NEXT strip.
    pub fn next_cell_box(&self, cell_px: (u16, u16)) -> (u16, u16) {
        cell_box(self.next.canvas.width(), self.next.canvas.height(), cell_px)
    }

    pub fn hold(&self) -> &PixelCanvas {
        &self.hold.canvas
    }

    pub fn next(&self) -> &PixelCanvas {
        &self.next.canvas
    }

    /// Repaint whichever preview changed. `hold`/`next` are the panel's cell
    /// anchors; returns whether anything was emitted.
    pub fn sync(
        &mut self,
        snap: &GameSnapshot,
        hold_anchor: (u16, u16),
        next_anchor: (u16, u16),
        out: &mut Vec<u8>,
    ) -> Result<bool> {
        let mut emitted = false;
        let mino = self.mino;
        let gap = self.gap;

        let hash = hold_hash(snap);
        if self.hold.needs(hash, anchor_key(hold_anchor)) {
            paint_preview(
                &mut self.hold.canvas,
                mino,
                gap,
                snap.hold.map(|kind| [kind]).into_iter().flatten(),
                snap.can_hold,
            );
            self.writer.transmit_native(
                &self.hold.canvas,
                self.hold.image_id,
                PLACEMENT,
                1,
                hold_anchor,
                out,
            )?;
            self.hold.mark(hash, anchor_key(hold_anchor));
            emitted = true;
        }

        let hash = next_hash(snap);
        if self.next.needs(hash, anchor_key(next_anchor)) {
            paint_preview(
                &mut self.next.canvas,
                mino,
                gap,
                snap.next_queue.into_iter(),
                true,
            );
            self.writer.transmit_native(
                &self.next.canvas,
                self.next.image_id,
                PLACEMENT,
                1,
                next_anchor,
                out,
            )?;
            self.next.mark(hash, anchor_key(next_anchor));
            emitted = true;
        }

        if emitted {
            self.live = true;
        }
        Ok(emitted)
    }

    pub fn invalidate(&mut self) {
        self.hold.drawn = None;
        self.next.drawn = None;
    }

    pub fn shutdown(&mut self, out: &mut Vec<u8>) {
        if !self.live {
            return;
        }
        self.writer.delete_all(out);
        self.live = false;
        self.hold.drawn = None;
        self.next.drawn = None;
    }
}

fn anchor_key((x, y): (u16, u16)) -> u64 {
    (u64::from(x) << 16) | u64::from(y)
}

fn cell_box(width: u32, height: u32, cell_px: (u16, u16)) -> (u16, u16) {
    let cw = u32::from(cell_px.0.max(1));
    let ch = u32::from(cell_px.1.max(1));
    (
        width.div_ceil(cw).clamp(1, u32::from(u16::MAX)) as u16,
        height.div_ceil(ch).clamp(1, u32::from(u16::MAX)) as u16,
    )
}

fn hold_hash(snap: &GameSnapshot) -> u64 {
    fnv_step(FNV_OFFSET, u64::from(kind_code_of(snap.hold)))
}

fn next_hash(snap: &GameSnapshot) -> u64 {
    let mut hash = FNV_OFFSET;
    for kind in snap.next_queue.iter() {
        hash = fnv_step(hash, u64::from(kind_code_of(Some(*kind))));
    }
    hash
}

fn kind_code_of(kind: Option<PieceKind>) -> u8 {
    kind.map(kind_code).unwrap_or(0)
}

/// Paint up to `NEXT_SLOTS` pieces stacked in the preview canvas.
fn paint_preview(
    canvas: &mut PixelCanvas,
    mino: u32,
    gap: u32,
    pieces: impl Iterator<Item = PieceKind>,
    first_opaque: bool,
) {
    canvas.clear();
    let slot_h = PREVIEW_ROWS * mino + gap;
    for (index, kind) in pieces.enumerate() {
        let dim = !first_opaque && index == 0;
        let color = if dim {
            blend(piece_color(kind), WELL_BG, 140)
        } else {
            piece_color(kind)
        };
        let shape = get_shape(kind, Rotation::North);
        let (mut min_x, mut max_x) = (i8::MAX, i8::MIN);
        let (mut min_y, mut max_y) = (i8::MAX, i8::MIN);
        for &(dx, dy) in shape.iter() {
            min_x = min_x.min(dx);
            max_x = max_x.max(dx);
            min_y = min_y.min(dy);
            max_y = max_y.max(dy);
        }
        let width = (max_x - min_x + 1) as u32;
        let height = (max_y - min_y + 1) as u32;
        let pad_x = (PREVIEW_COLS.saturating_sub(width)) * mino / 2;
        let pad_y = (PREVIEW_ROWS.saturating_sub(height)) * mino / 2;
        let slot = Grid::new((pad_x, index as u32 * slot_h + pad_y), (mino, mino));
        let mut cells = ArrayVec::<_, 4>::new();
        for &(dx, dy) in shape.iter() {
            let _ = cells.try_push((dx - min_x, dy - min_y));
        }
        paint_regions(
            canvas,
            slot,
            &cells,
            |x, y| cells.contains(&(x, y)).then_some(color),
            255,
            255,
        );
    }
}
