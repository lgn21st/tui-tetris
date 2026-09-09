//! Choose how many terminal cells make one mino.
//!
//! A terminal cell is almost never square. Painting a mino as 2 columns × 1 row
//! is square only when the cell itself is 1:2. This module picks the integer
//! `(cell_w, cell_h)` whose pixel rectangle is closest to a square.

/// Integer terminal columns/rows per mino that best approximate a square.
///
/// `cell_px_w` / `cell_px_h` are the pixel size of one terminal cell.
/// Falls back to `(2, 1)` when the terminal does not report pixels.
pub fn squarest_cell_size(cell_px_w: u16, cell_px_h: u16) -> (u16, u16) {
    if cell_px_w == 0 || cell_px_h == 0 {
        return (2, 1);
    }

    let mut best = (2, 1);
    let mut best_rel = u32::MAX;
    let mut best_area = u32::MAX;

    for h in 1..=2u16 {
        for w in 1..=3u16 {
            let pw = u32::from(w) * u32::from(cell_px_w);
            let ph = u32::from(h) * u32::from(cell_px_h);
            let err = pw.abs_diff(ph);
            let rel = err.saturating_mul(1000) / pw.max(ph).max(1);
            let area = u32::from(w) * u32::from(h);
            // Prefer the smallest mino that is equally square so a 10×20
            // well still fits typical 80×24 terminals.
            if rel < best_rel || (rel == best_rel && area < best_area) {
                best = (w, h);
                best_rel = rel;
                best_area = area;
            }
        }
    }
    best
}

/// Pixel size of one terminal cell from the tty ioctl, if the emulator fills it.
pub fn detect_cell_pixels() -> Option<(u16, u16)> {
    let ws = crossterm::terminal::window_size().ok()?;
    if ws.columns == 0 || ws.rows == 0 || ws.width == 0 || ws.height == 0 {
        return None;
    }
    let px_w = ws.width / ws.columns;
    let px_h = ws.height / ws.rows;
    if px_w == 0 || px_h == 0 {
        return None;
    }
    Some((px_w, px_h))
}

#[cfg(test)]
mod tests {
    use super::squarest_cell_size;

    #[test]
    fn classic_1_by_2_cells_use_two_columns() {
        assert_eq!(squarest_cell_size(8, 16), (2, 1));
        assert_eq!(squarest_cell_size(10, 20), (2, 1));
    }

    #[test]
    fn square_font_cells_use_one_by_one() {
        assert_eq!(squarest_cell_size(12, 12), (1, 1));
        assert_eq!(squarest_cell_size(16, 16), (1, 1));
    }

    #[test]
    fn tall_cells_can_use_three_columns() {
        assert_eq!(squarest_cell_size(6, 20), (3, 1));
    }

    #[test]
    fn missing_metrics_keep_the_2x1_default() {
        assert_eq!(squarest_cell_size(0, 16), (2, 1));
        assert_eq!(squarest_cell_size(8, 0), (2, 1));
    }
}
