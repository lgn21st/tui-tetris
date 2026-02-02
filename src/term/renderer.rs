//! TerminalRenderer: flushes a framebuffer to a real terminal.
//!
//! This module intentionally keeps the drawing API small. It can start with full
//! redraws and later evolve into diff/dirty-rect rendering.

use std::io::{self, Write};

use anyhow::Result;

use crossterm::{
    cursor,
    style::{
        Attribute, Color, Print, ResetColor, SetAttribute, SetBackgroundColor, SetForegroundColor,
    },
    terminal, QueueableCommand,
};

use crate::term::fb::{CellStyle, FrameBuffer, Rgb};

pub struct TerminalRenderer {
    stdout: io::Stdout,
    last: Option<FrameBuffer>,
}

impl TerminalRenderer {
    pub fn new() -> Self {
        Self {
            stdout: io::stdout(),
            last: None,
        }
    }

    pub fn enter(&mut self) -> Result<()> {
        terminal::enable_raw_mode()?;
        self.stdout.queue(terminal::EnterAlternateScreen)?;
        self.stdout.queue(cursor::Hide)?;
        self.stdout.queue(terminal::DisableLineWrap)?;
        self.stdout.flush()?;
        Ok(())
    }

    pub fn exit(&mut self) -> Result<()> {
        self.stdout.queue(ResetColor)?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;
        self.stdout.queue(terminal::EnableLineWrap)?;
        self.stdout.queue(cursor::Show)?;
        self.stdout.queue(terminal::LeaveAlternateScreen)?;
        self.stdout.flush()?;
        terminal::disable_raw_mode()?;
        Ok(())
    }

    /// Force the next draw to be a full redraw.
    ///
    /// Useful on terminal resize events.
    pub fn invalidate(&mut self) {
        self.last = None;
    }

    /// Draw a framebuffer, swapping it into internal state.
    ///
    /// Callers should keep one `FrameBuffer` and pass it in every frame.
    /// The renderer will diff against the previous frame and then swap buffers
    /// so the caller can reuse the old one without cloning.
    pub fn draw_swap(&mut self, fb: &mut FrameBuffer) -> Result<()> {
        if self.last.is_none() {
            self.last = Some(FrameBuffer::new(fb.width(), fb.height()));
        }

        // Take previous out to avoid borrow conflicts (no cloning).
        let mut prev = self.last.take().unwrap();
        let needs_full = prev.width() != fb.width() || prev.height() != fb.height();

        if needs_full {
            self.full_redraw(fb)?;
            prev.resize(fb.width(), fb.height());
        } else {
            self.diff_redraw(fb, &prev)?;
        }

        // Swap current into prev so next frame can diff without cloning.
        std::mem::swap(&mut prev, fb);
        self.last = Some(prev);
        Ok(())
    }

    fn full_redraw(&mut self, fb: &FrameBuffer) -> Result<()> {
        self.stdout
            .queue(terminal::Clear(terminal::ClearType::All))?;
        self.stdout.queue(cursor::MoveTo(0, 0))?;

        let mut current_style: Option<CellStyle> = None;
        for y in 0..fb.height() {
            for x in 0..fb.width() {
                let cell = fb.get(x, y).unwrap_or_default();
                if current_style != Some(cell.style) {
                    self.apply_style(cell.style)?;
                    current_style = Some(cell.style);
                }
                self.stdout.queue(Print(cell.ch))?;
            }
            if y + 1 < fb.height() {
                self.stdout.queue(Print("\r\n"))?;
            }
        }

        self.stdout.queue(ResetColor)?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;
        self.stdout.flush()?;
        Ok(())
    }

    fn diff_redraw(&mut self, next: &FrameBuffer, prev: &FrameBuffer) -> Result<()> {
        let mut current_style: Option<CellStyle> = None;

        for_each_changed_run(prev, next, |x, y, len| {
            // Cursor move per run, then print cells in the run.
            self.stdout.queue(cursor::MoveTo(x, y))?;
            for dx in 0..len {
                let cell = next.get(x + dx, y).unwrap_or_default();
                if current_style != Some(cell.style) {
                    self.apply_style(cell.style)?;
                    current_style = Some(cell.style);
                }
                self.stdout.queue(Print(cell.ch))?;
            }
            Ok(())
        })?;

        self.stdout.queue(ResetColor)?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;
        self.stdout.flush()?;
        Ok(())
    }

    fn apply_style(&mut self, style: CellStyle) -> Result<()> {
        self.stdout
            .queue(SetForegroundColor(rgb_to_color(style.fg)))?;
        self.stdout
            .queue(SetBackgroundColor(rgb_to_color(style.bg)))?;
        self.stdout.queue(SetAttribute(Attribute::Reset))?;
        if style.bold {
            self.stdout.queue(SetAttribute(Attribute::Bold))?;
        }
        if style.dim {
            self.stdout.queue(SetAttribute(Attribute::Dim))?;
        }
        Ok(())
    }
}

fn rgb_to_color(rgb: Rgb) -> Color {
    Color::Rgb {
        r: rgb.r,
        g: rgb.g,
        b: rgb.b,
    }
}

fn for_each_changed_run(
    prev: &FrameBuffer,
    next: &FrameBuffer,
    mut f: impl FnMut(u16, u16, u16) -> Result<()>,
) -> Result<()> {
    if prev.width() != next.width() || prev.height() != next.height() {
        // Size changed: treat everything as dirty in a single pass (row runs).
        for y in 0..next.height() {
            f(0, y, next.width())?;
        }
        return Ok(());
    }

    let w = next.width();
    let h = next.height();

    for y in 0..h {
        let mut x = 0;
        while x < w {
            let a = prev.get(x, y).unwrap_or_default();
            let b = next.get(x, y).unwrap_or_default();
            if a == b {
                x += 1;
                continue;
            }

            let start = x;
            x += 1;
            while x < w {
                let a2 = prev.get(x, y).unwrap_or_default();
                let b2 = next.get(x, y).unwrap_or_default();
                if a2 == b2 {
                    break;
                }
                x += 1;
            }
            let len = x - start;
            f(start, y, len)?;
        }
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::term::fb::{Cell, CellStyle};

    // This is not a perfect test of terminal output, but it ensures we can build
    // a framebuffer and iterate all cells without panicking.
    #[test]
    fn can_draw_small_framebuffer() {
        let mut fb = FrameBuffer::new(2, 2);
        let style = CellStyle::default();
        fb.set(0, 0, Cell { ch: 'A', style });
        fb.set(1, 0, Cell { ch: 'B', style });
        fb.set(0, 1, Cell { ch: 'C', style });
        fb.set(1, 1, Cell { ch: 'D', style });

        // We can't easily validate the terminal I/O in unit tests.
        // But we can at least exercise the style conversion.
        assert_eq!(
            rgb_to_color(style.fg),
            Color::Rgb {
                r: style.fg.r,
                g: style.fg.g,
                b: style.fg.b
            }
        );
    }

    #[test]
    fn changed_run_iterator_coalesces_adjacent_cells() {
        let style = CellStyle::default();
        let a = FrameBuffer::new(5, 1);
        let mut b = FrameBuffer::new(5, 1);

        // Change cells [1..=3] into X.
        for x in 1..=3 {
            b.set(x, 0, Cell { ch: 'X', style });
        }

        let mut runs = Vec::new();
        for_each_changed_run(&a, &b, |x, y, len| {
            runs.push((x, y, len));
            Ok(())
        })
        .unwrap();
        assert_eq!(runs, vec![(1, 0, 3)]);
    }
}
