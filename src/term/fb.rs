//! Framebuffer and style types for terminal rendering.

/// 24-bit RGB color.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Rgb {
    pub r: u8,
    pub g: u8,
    pub b: u8,
}

impl Rgb {
    pub const fn new(r: u8, g: u8, b: u8) -> Self {
        Self { r, g, b }
    }
}

/// Minimal per-cell styling.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct CellStyle {
    pub fg: Rgb,
    pub bg: Rgb,
    pub bold: bool,
    pub dim: bool,
}

impl Default for CellStyle {
    fn default() -> Self {
        Self {
            fg: Rgb::new(220, 220, 220),
            bg: Rgb::new(0, 0, 0),
            bold: false,
            dim: false,
        }
    }
}

/// A single terminal cell.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Cell {
    pub ch: char,
    pub style: CellStyle,
}

impl Default for Cell {
    fn default() -> Self {
        Self {
            ch: ' ',
            style: CellStyle::default(),
        }
    }
}

/// 2D framebuffer of styled character cells.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct FrameBuffer {
    width: u16,
    height: u16,
    cells: Vec<Cell>,
}

impl FrameBuffer {
    pub fn new(width: u16, height: u16) -> Self {
        let len = (width as usize) * (height as usize);
        Self {
            width,
            height,
            cells: vec![Cell::default(); len],
        }
    }

    pub fn width(&self) -> u16 {
        self.width
    }

    pub fn height(&self) -> u16 {
        self.height
    }

    /// Resize the framebuffer.
    ///
    /// This preserves the underlying allocation when possible.
    pub fn resize(&mut self, width: u16, height: u16) {
        if self.width == width && self.height == height {
            return;
        }
        self.width = width;
        self.height = height;
        let len = (width as usize) * (height as usize);
        self.cells.resize(len, Cell::default());
    }

    pub fn cells(&self) -> &[Cell] {
        &self.cells
    }

    #[inline(always)]
    fn idx(&self, x: u16, y: u16) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some((y as usize) * (self.width as usize) + (x as usize))
    }

    pub fn get(&self, x: u16, y: u16) -> Option<Cell> {
        self.idx(x, y).map(|i| self.cells[i])
    }

    pub fn set(&mut self, x: u16, y: u16, cell: Cell) {
        if let Some(i) = self.idx(x, y) {
            self.cells[i] = cell;
        }
    }

    pub fn clear(&mut self, cell: Cell) {
        self.cells.fill(cell);
    }

    pub fn put_char(&mut self, x: u16, y: u16, ch: char, style: CellStyle) {
        self.set(x, y, Cell { ch, style });
    }

    pub fn put_str(&mut self, x: u16, y: u16, s: &str, style: CellStyle) {
        let mut cx = x;
        for ch in s.chars() {
            if cx >= self.width {
                break;
            }
            self.put_char(cx, y, ch, style);
            cx += 1;
        }
    }

    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, ch: char, style: CellStyle) {
        for dy in 0..h {
            for dx in 0..w {
                self.put_char(x.saturating_add(dx), y.saturating_add(dy), ch, style);
            }
        }
    }
}
