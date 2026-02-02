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

    /// Full redraw of the framebuffer.
    pub fn draw(&mut self, fb: &FrameBuffer) -> Result<()> {
        let prev_opt = self.last.as_ref();
        let needs_full = match prev_opt {
            None => true,
            Some(prev) => prev.width() != fb.width() || prev.height() != fb.height(),
        };

        if needs_full {
            self.full_redraw(fb)?;
        } else {
            // Clone the previous framebuffer to avoid borrowing self immutably across a mutable call.
            let prev = prev_opt.unwrap().clone();
            self.diff_redraw(fb, &prev)?;
        }

        self.last = Some(fb.clone());
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

        for idx in diff_indices(prev, next) {
            let x = (idx % next.width() as usize) as u16;
            let y = (idx / next.width() as usize) as u16;
            let cell = next.cells()[idx];

            self.stdout.queue(cursor::MoveTo(x, y))?;
            if current_style != Some(cell.style) {
                self.apply_style(cell.style)?;
                current_style = Some(cell.style);
            }
            self.stdout.queue(Print(cell.ch))?;
        }

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

fn diff_indices(prev: &FrameBuffer, next: &FrameBuffer) -> Vec<usize> {
    if prev.width() != next.width() || prev.height() != next.height() {
        return (0..(next.width() as usize * next.height() as usize)).collect();
    }

    let mut out = Vec::new();
    for (i, (a, b)) in prev.cells().iter().zip(next.cells().iter()).enumerate() {
        if a != b {
            out.push(i);
        }
    }
    out
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
    fn diff_indices_detects_changes() {
        let style = CellStyle::default();
        let mut a = FrameBuffer::new(2, 2);
        let mut b = FrameBuffer::new(2, 2);

        a.set(0, 0, Cell { ch: 'A', style });
        b.set(0, 0, Cell { ch: 'A', style });
        b.set(1, 1, Cell { ch: 'X', style });

        let diff = diff_indices(&a, &b);
        assert_eq!(diff, vec![3]);
    }
}
