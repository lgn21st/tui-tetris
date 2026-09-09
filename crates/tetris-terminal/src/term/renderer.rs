//! TerminalRenderer: flushes a framebuffer to a real terminal.
//!
//! This module intentionally keeps the drawing API small. It can start with full
//! redraws and later evolve into diff/dirty-rect rendering.

use std::io::{self, Write};

use anyhow::Result;

use crossterm::{
    QueueableCommand, cursor,
    style::{Attribute, ResetColor, SetAttribute},
    terminal,
};

use crate::term::fb::{CellStyle, FrameBuffer};

pub struct TerminalRenderer<W: Write = io::Stdout> {
    writer: W,
    last: Option<FrameBuffer>,
    buf: Vec<u8>,
}

impl TerminalRenderer<io::Stdout> {
    pub fn new() -> Self {
        Self::with_writer(io::stdout())
    }
}

impl<W: Write> TerminalRenderer<W> {
    /// Construct a renderer backed by an arbitrary writer.
    ///
    /// This keeps terminal encoding and flush behavior testable and benchmarkable
    /// without coupling measurements to a particular terminal emulator.
    pub fn with_writer(writer: W) -> Self {
        Self {
            writer,
            last: None,
            buf: Vec::with_capacity(64 * 1024),
        }
    }

    pub fn enter(&mut self) -> Result<()> {
        terminal::enable_raw_mode()?;
        self.buf.clear();
        self.buf.queue(terminal::EnterAlternateScreen)?;
        self.buf.queue(cursor::Hide)?;
        self.buf.queue(terminal::DisableLineWrap)?;
        self.flush_buf()?;
        Ok(())
    }

    pub fn exit(&mut self) -> Result<()> {
        self.buf.clear();
        self.buf.queue(ResetColor)?;
        self.buf.queue(SetAttribute(Attribute::Reset))?;
        self.buf.queue(terminal::EnableLineWrap)?;
        self.buf.queue(cursor::Show)?;
        self.buf.queue(terminal::LeaveAlternateScreen)?;
        self.flush_buf()?;
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
        let invalidated = self.last.is_none();
        if self.last.is_none() {
            self.last = Some(FrameBuffer::new(fb.width(), fb.height()));
        }

        // Take previous out to avoid borrow conflicts (no cloning).
        let mut prev = self.last.take().unwrap();
        let needs_full = invalidated || prev.width() != fb.width() || prev.height() != fb.height();

        if needs_full {
            self.buf.clear();
            encode_full_into(fb, &mut self.buf)?;
            self.flush_buf()?;
            prev.resize(fb.width(), fb.height());
        } else {
            self.buf.clear();
            encode_diff_into(&prev, fb, &mut self.buf)?;
            if !self.buf.is_empty() {
                self.flush_buf()?;
            }
        }

        // Swap current into prev so next frame can diff without cloning.
        std::mem::swap(&mut prev, fb);
        self.last = Some(prev);
        Ok(())
    }

    fn flush_buf(&mut self) -> Result<()> {
        self.writer.write_all(&self.buf)?;
        self.writer.flush()?;
        Ok(())
    }
}

impl Default for TerminalRenderer<io::Stdout> {
    fn default() -> Self {
        Self::new()
    }
}

/// Encode a full-frame redraw into `out` using ANSI CSI sequences.
pub fn encode_full_into(fb: &FrameBuffer, out: &mut Vec<u8>) -> Result<()> {
    out.extend_from_slice(b"\x1b[2J");
    push_move_to(out, 0, 0);

    let mut current_style: Option<CellStyle> = None;
    for y in 0..fb.height() {
        for x in 0..fb.width() {
            let cell = fb.get(x, y).unwrap_or_default();
            if current_style != Some(cell.style) {
                apply_style_into(out, cell.style);
                current_style = Some(cell.style);
            }
            push_char(out, cell.ch);
        }
        if y + 1 < fb.height() {
            out.extend_from_slice(b"\r\n");
        }
    }

    out.extend_from_slice(b"\x1b[0m");
    Ok(())
}

/// Encode a diff redraw (changed runs) into `out` using ANSI CSI sequences.
pub fn encode_diff_into(prev: &FrameBuffer, next: &FrameBuffer, out: &mut Vec<u8>) -> Result<()> {
    let mut current_style: Option<CellStyle> = None;

    for_each_changed_run(prev, next, |x, y, len| {
        push_move_to(out, x, y);
        for dx in 0..len {
            let cell = next.get(x + dx, y).unwrap_or_default();
            if current_style != Some(cell.style) {
                apply_style_into(out, cell.style);
                current_style = Some(cell.style);
            }
            push_char(out, cell.ch);
        }
        Ok(())
    })?;

    if current_style.is_some() {
        out.extend_from_slice(b"\x1b[0m");
    }
    Ok(())
}

fn push_decimal(out: &mut Vec<u8>, mut value: u32) {
    let mut digits = [0u8; 10];
    let mut i = 10;
    if value == 0 {
        out.push(b'0');
        return;
    }
    while value != 0 {
        i -= 1;
        digits[i] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    out.extend_from_slice(&digits[i..]);
}

fn push_move_to(out: &mut Vec<u8>, x: u16, y: u16) {
    out.extend_from_slice(b"\x1b[");
    push_decimal(out, u32::from(y) + 1);
    out.push(b';');
    push_decimal(out, u32::from(x) + 1);
    out.push(b'H');
}

fn push_char(out: &mut Vec<u8>, ch: char) {
    let mut buf = [0u8; 4];
    let encoded = ch.encode_utf8(&mut buf);
    out.extend_from_slice(encoded.as_bytes());
}

fn apply_style_into(out: &mut Vec<u8>, style: CellStyle) {
    out.extend_from_slice(b"\x1b[0m\x1b[38;2;");
    push_decimal(out, u32::from(style.fg.r));
    out.push(b';');
    push_decimal(out, u32::from(style.fg.g));
    out.push(b';');
    push_decimal(out, u32::from(style.fg.b));
    out.extend_from_slice(b"m\x1b[48;2;");
    push_decimal(out, u32::from(style.bg.r));
    out.push(b';');
    push_decimal(out, u32::from(style.bg.g));
    out.push(b';');
    push_decimal(out, u32::from(style.bg.b));
    out.push(b'm');
    if style.bold {
        out.extend_from_slice(b"\x1b[1m");
    }
    if style.dim {
        out.extend_from_slice(b"\x1b[2m");
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
    use std::sync::{Arc, Mutex};

    #[derive(Clone, Default)]
    struct CountingWriter {
        counts: Arc<Mutex<(usize, usize, usize)>>,
    }

    impl Write for CountingWriter {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            let mut counts = self.counts.lock().unwrap();
            counts.0 += 1;
            counts.1 += buf.len();
            Ok(buf.len())
        }

        fn flush(&mut self) -> io::Result<()> {
            self.counts.lock().unwrap().2 += 1;
            Ok(())
        }
    }

    #[test]
    fn can_draw_small_framebuffer() {
        let mut fb = FrameBuffer::new(2, 2);
        let style = CellStyle::default();
        fb.set(0, 0, Cell { ch: 'A', style });
        fb.set(1, 0, Cell { ch: 'B', style });
        fb.set(0, 1, Cell { ch: 'C', style });
        fb.set(1, 1, Cell { ch: 'D', style });
        assert_eq!(fb.get(0, 0).unwrap().ch, 'A');
        assert_eq!(fb.get(1, 1).unwrap().ch, 'D');
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

    #[test]
    fn changed_run_iterator_marks_full_frame_when_prev_size_differs() {
        let prev = FrameBuffer::new(2, 2);
        let next = FrameBuffer::new(3, 2);
        let mut runs = Vec::new();
        for_each_changed_run(&prev, &next, |x, y, len| {
            runs.push((x, y, len));
            Ok(())
        })
        .unwrap();
        assert_eq!(runs, vec![(0, 0, 3), (0, 1, 3)]);
    }

    #[test]
    fn identical_framebuffers_encode_no_terminal_output() {
        let frame = FrameBuffer::new(80, 24);
        let mut output = Vec::new();

        encode_diff_into(&frame, &frame, &mut output).unwrap();

        assert!(output.is_empty());
    }

    #[test]
    fn encode_diff_emits_csi_move_and_glyph() {
        let prev = FrameBuffer::new(4, 2);
        let mut next = FrameBuffer::new(4, 2);
        next.put_char(1, 0, 'X', CellStyle::default());
        let mut output = Vec::new();
        encode_diff_into(&prev, &next, &mut output).unwrap();
        assert!(output.contains(&b'\x1b'));
        assert!(output.contains(&b'X'));
        assert!(output.starts_with(b"\x1b[1;2H"));
    }

    #[test]
    fn injected_writer_skips_output_and_flush_for_unchanged_frame() {
        let writer = CountingWriter::default();
        let counts = Arc::clone(&writer.counts);
        let mut renderer = TerminalRenderer::with_writer(writer);
        let mut frame = FrameBuffer::new(4, 2);

        renderer.draw_swap(&mut frame).unwrap();
        let after_first = *counts.lock().unwrap();
        renderer.draw_swap(&mut frame).unwrap();

        assert!(after_first.0 > 0);
        assert!(after_first.1 > 0);
        assert_eq!(*counts.lock().unwrap(), after_first);
    }

    #[test]
    fn injected_writer_flushes_changed_frame() {
        let writer = CountingWriter::default();
        let counts = Arc::clone(&writer.counts);
        let mut renderer = TerminalRenderer::with_writer(writer);
        let mut frame = FrameBuffer::new(4, 2);
        renderer.draw_swap(&mut frame).unwrap();
        let after_first = *counts.lock().unwrap();

        frame.put_char(1, 0, 'X', CellStyle::default());
        renderer.draw_swap(&mut frame).unwrap();
        let after_change = *counts.lock().unwrap();

        assert!(after_change.0 > after_first.0);
        assert!(after_change.1 > after_first.1);
        assert!(after_change.2 > after_first.2);
    }
}
