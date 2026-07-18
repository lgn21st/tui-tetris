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
        let Some(start) = self.idx(x, y) else {
            return;
        };
        let available = (self.width - x) as usize;
        for (cell, ch) in self.cells[start..start + available]
            .iter_mut()
            .zip(s.chars())
        {
            *cell = Cell { ch, style };
        }
    }

    pub fn put_u32(&mut self, x: u16, y: u16, mut v: u32, style: CellStyle) {
        // Max 10 digits for u32.
        let mut buf = [0u8; 10];
        let mut i = 10;
        if v == 0 {
            i -= 1;
            buf[i] = b'0';
        } else {
            while v != 0 {
                let digit = (v % 10) as u8;
                v /= 10;
                i -= 1;
                buf[i] = b'0' + digit;
            }
        }
        let s = std::str::from_utf8(&buf[i..]).expect("digits are valid utf8");
        self.put_str(x, y, s, style);
    }

    pub fn fill_rect(&mut self, x: u16, y: u16, w: u16, h: u16, ch: char, style: CellStyle) {
        let end_x = x.saturating_add(w).min(self.width);
        let end_y = y.saturating_add(h).min(self.height);
        if x >= end_x || y >= end_y {
            return;
        }

        let fill = Cell { ch, style };
        let width = self.width as usize;
        for row in y as usize..end_y as usize {
            let start = row * width + x as usize;
            let end = row * width + end_x as usize;
            self.cells[start..end].fill(fill);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn put_str_clips_at_the_right_edge() {
        let mut fb = FrameBuffer::new(3, 1);
        let style = CellStyle::default();

        fb.put_str(1, 0, "ABCD", style);

        assert_eq!(fb.get(0, 0).unwrap().ch, ' ');
        assert_eq!(fb.get(1, 0).unwrap().ch, 'A');
        assert_eq!(fb.get(2, 0).unwrap().ch, 'B');
    }

    #[test]
    fn fill_rect_clips_to_the_framebuffer() {
        let mut fb = FrameBuffer::new(3, 2);
        let style = CellStyle::default();

        fb.fill_rect(1, 1, 4, 3, 'X', style);

        assert_eq!(fb.get(0, 1).unwrap().ch, ' ');
        assert_eq!(fb.get(1, 1).unwrap().ch, 'X');
        assert_eq!(fb.get(2, 1).unwrap().ch, 'X');
        assert_eq!(fb.get(2, 0).unwrap().ch, ' ');
    }
}
