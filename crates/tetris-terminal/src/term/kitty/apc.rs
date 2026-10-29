//! Kitty graphics escape encoding.
//!
//! Wire shape (7-bit form):
//!
//! ```text
//! ESC _ G <control keys> ; <base64 pixel payload> ESC \
//! ```
//!
//! Design decisions that matter on a live terminal:
//!
//! - `o=z` zlib-compresses the pixel payload. Flat-colour pixel art shrinks from
//!   hundreds of kilobytes to a couple of kilobytes, which keeps a per-move
//!   retransmission cheap.
//! - `q=2` is repeated on every chunk. A continuation chunk without it makes the
//!   terminal answer `Gi=...;OK` onto the tty, which tmux then prints into the
//!   prompt.
//! - `C=1` keeps the text cursor where it was, so image placement never
//!   disturbs the diff renderer's cursor bookkeeping.
//! - `p` pins one placement id so a retransmission *replaces* the old placement
//!   instead of accumulating duplicate copies.
//! - Inside tmux each command is wrapped in `ESC Ptmux; ... ESC \` with every
//!   inner ESC doubled.

use anyhow::Result;
use flate2::{Compress, Compression, FlushCompress, Status};

use crate::term::kitty::canvas::PixelCanvas;
use crate::term::kitty::capability::KittySupport;
use crate::term::kitty::well::WellGeometry;

/// Maximum base64 bytes per escape. The protocol limit is 4096, and it is a
/// multiple of four so chunking never splits a quartet.
pub const BASE64_CHUNK: usize = 4096;

const BASE64_TABLE: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

/// Encodes graphics commands, reusing every buffer across frames.
pub struct KittyEscapeWriter {
    support: KittySupport,
    compressor: Compress,
    deflated: Vec<u8>,
    encoded: Vec<u8>,
}

impl KittyEscapeWriter {
    pub fn new(support: KittySupport) -> Self {
        Self {
            support,
            compressor: Compress::new(Compression::fast(), true),
            deflated: Vec::new(),
            encoded: Vec::new(),
        }
    }

    pub fn support(&self) -> KittySupport {
        self.support
    }

    /// Transmit the canvas as image `id` and place it, covering `cols x rows`
    /// terminal cells starting at `cursor`.
    ///
    /// `z` orders the layer stack. The image is scaled into the cell box by the
    /// terminal, which also keeps it from being truncated at the screen edge.
    #[allow(clippy::too_many_arguments)] // A flat call keeps the hot path allocation-free.
    pub fn transmit_and_place(
        &mut self,
        canvas: &PixelCanvas,
        id: u32,
        placement: u32,
        z: i32,
        geom: WellGeometry,
        cursor: (u16, u16),
        out: &mut Vec<u8>,
    ) -> Result<()> {
        self.transmit(canvas, id, placement, z, Some(geom), cursor, out)
    }

    #[allow(clippy::too_many_arguments)] // Flat scalars keep the hot path allocation-free.
    fn transmit(
        &mut self,
        canvas: &PixelCanvas,
        id: u32,
        placement: u32,
        z: i32,
        geom: Option<WellGeometry>,
        cursor: (u16, u16),
        out: &mut Vec<u8>,
    ) -> Result<()> {
        self.deflate(canvas.data())?;
        // Reserve for the worst case (deflate can grow stored blocks, base64
        // adds a third) so retransmissions never reallocate.
        let worst_encoded = self.deflated.capacity().div_ceil(3) * 4;
        if self.encoded.capacity() < worst_encoded {
            let extra = worst_encoded - self.encoded.capacity();
            self.encoded.reserve(extra);
        }
        encode_base64_into(&self.deflated, &mut self.encoded);

        push_move_to(out, cursor.0, cursor.1);

        let payload = &self.encoded;
        let tmux = self.support.tmux_passthrough;
        let mut offset = 0;
        loop {
            let end = (offset + BASE64_CHUNK).min(payload.len());
            let more = u8::from(end < payload.len());
            begin_apc(out, tmux);
            if offset == 0 {
                out.extend_from_slice(b"a=T,f=");
                push_decimal(out, u32::from(canvas.format().kitty_code()));
                out.extend_from_slice(b",o=z,s=");
                push_decimal(out, canvas.width());
                out.extend_from_slice(b",v=");
                push_decimal(out, canvas.height());
                out.extend_from_slice(b",i=");
                push_decimal(out, id);
                out.extend_from_slice(b",p=");
                push_decimal(out, placement);
                if let Some(geom) = geom {
                    let (cols, rows) = geom.cells();
                    out.extend_from_slice(b",c=");
                    push_decimal(out, u32::from(cols));
                    out.extend_from_slice(b",r=");
                    push_decimal(out, u32::from(rows));
                }
                out.extend_from_slice(b",C=1,q=2,z=");
                push_signed_decimal(out, z);
            } else {
                // Continuations carry only the chunking keys.
                out.extend_from_slice(b"q=2");
            }
            out.extend_from_slice(b",m=");
            out.push(b'0' + more);
            out.push(b';');
            out.extend_from_slice(&payload[offset..end]);
            end_apc(out, tmux);

            if more == 0 {
                break;
            }
            offset = end;
        }
        Ok(())
    }

    /// Transmit an image and place it at its **native** pixel size.
    ///
    /// Used for the HUD previews: they are small sprites drawn at device
    /// resolution, so passing `c=`/`r=` would only make the terminal resample
    /// them into a cell box it does not match.
    pub fn transmit_native(
        &mut self,
        canvas: &PixelCanvas,
        id: u32,
        placement: u32,
        z: i32,
        cursor: (u16, u16),
        out: &mut Vec<u8>,
    ) -> Result<()> {
        self.transmit(canvas, id, placement, z, None, cursor, out)
    }

    /// Delete every placement of `id`.
    pub fn delete_image(&mut self, id: u32, out: &mut Vec<u8>) {
        let tmux = self.support.tmux_passthrough;
        begin_apc(out, tmux);
        out.extend_from_slice(b"a=d,d=i,i=");
        push_decimal(out, id);
        out.extend_from_slice(b",q=2");
        end_apc(out, tmux);
    }

    /// Delete every visible image and free the terminal's image storage.
    pub fn delete_all(&mut self, out: &mut Vec<u8>) {
        let tmux = self.support.tmux_passthrough;
        begin_apc(out, tmux);
        out.extend_from_slice(b"a=d,d=A,q=2");
        end_apc(out, tmux);
    }

    fn deflate(&mut self, input: &[u8]) -> Result<()> {
        let result = self.deflate_stream(input);
        // Always restart the stream so a mid-stream error cannot poison the
        // encoder for the rest of the session.
        self.compressor.reset();
        result
    }

    fn deflate_stream(&mut self, input: &[u8]) -> Result<()> {
        self.deflated.clear();
        // Worst case deflate output for incompressible input is the input plus
        // a small stored-block header. Reserving it once keeps the steady state
        // allocation-free whatever the frame looks like.
        let worst = input.len() + 64;
        if self.deflated.capacity() < worst {
            let extra = worst - self.deflated.capacity();
            self.deflated.reserve(extra);
        }
        let mut consumed = 0usize;
        loop {
            let written_before = self.deflated.len();
            let read_before = self.compressor.total_in();
            let status = self.compressor.compress_vec(
                &input[consumed..],
                &mut self.deflated,
                FlushCompress::Finish,
            )?;
            consumed += (self.compressor.total_in() - read_before) as usize;
            if status == Status::StreamEnd {
                break;
            }
            if self.deflated.len() == written_before {
                // The spare capacity is exhausted; make room and continue.
                let extra = self.deflated.capacity().max(1024);
                self.deflated.reserve(extra);
            }
        }
        Ok(())
    }
}

fn encode_base64_into(src: &[u8], dst: &mut Vec<u8>) {
    dst.clear();
    let needed = src.len().div_ceil(3) * 4;
    if dst.capacity() < needed {
        dst.reserve(needed - dst.capacity());
    }
    for group in src.chunks(3) {
        let b0 = u32::from(group[0]);
        let b1 = u32::from(*group.get(1).unwrap_or(&0));
        let b2 = u32::from(*group.get(2).unwrap_or(&0));
        let triple = (b0 << 16) | (b1 << 8) | b2;
        dst.push(BASE64_TABLE[((triple >> 18) & 0x3f) as usize]);
        dst.push(BASE64_TABLE[((triple >> 12) & 0x3f) as usize]);
        if group.len() > 1 {
            dst.push(BASE64_TABLE[((triple >> 6) & 0x3f) as usize]);
        } else {
            dst.push(b'=');
        }
        if group.len() > 2 {
            dst.push(BASE64_TABLE[(triple & 0x3f) as usize]);
        } else {
            dst.push(b'=');
        }
    }
}

pub(crate) fn push_signed_decimal(out: &mut Vec<u8>, value: i32) {
    if value < 0 {
        out.push(b'-');
        push_decimal(out, value.unsigned_abs());
    } else {
        push_decimal(out, value as u32);
    }
}

pub(crate) fn push_decimal(out: &mut Vec<u8>, mut value: u32) {
    let mut digits = [0u8; 10];
    let mut index = digits.len();
    if value == 0 {
        out.push(b'0');
        return;
    }
    while value != 0 {
        index -= 1;
        digits[index] = b'0' + (value % 10) as u8;
        value /= 10;
    }
    out.extend_from_slice(&digits[index..]);
}

/// Move the cursor to a zero-based `(column, row)` cell.
pub(crate) fn push_move_to(out: &mut Vec<u8>, column: u16, row: u16) {
    out.extend_from_slice(b"\x1b[");
    push_decimal(out, u32::from(row) + 1);
    out.push(b';');
    push_decimal(out, u32::from(column) + 1);
    out.push(b'H');
}

fn begin_apc(out: &mut Vec<u8>, tmux_passthrough: bool) {
    if tmux_passthrough {
        out.extend_from_slice(b"\x1bPtmux;");
        // The inner APC introducer, with its ESC doubled for tmux.
        out.extend_from_slice(b"\x1b\x1b_G");
    } else {
        out.extend_from_slice(b"\x1b_G");
    }
}

fn end_apc(out: &mut Vec<u8>, tmux_passthrough: bool) {
    if tmux_passthrough {
        out.extend_from_slice(b"\x1b\x1b\\");
        out.extend_from_slice(b"\x1b\\");
    } else {
        out.extend_from_slice(b"\x1b\\");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn base64_matches_the_reference_alphabet() {
        let mut out = Vec::new();
        encode_base64_into(b"", &mut out);
        assert!(out.is_empty());
        encode_base64_into(b"f", &mut out);
        assert_eq!(out, b"Zg==");
        encode_base64_into(b"fo", &mut out);
        assert_eq!(out, b"Zm8=");
        encode_base64_into(b"foo", &mut out);
        assert_eq!(out, b"Zm9v");
        encode_base64_into(b"foobar", &mut out);
        assert_eq!(out, b"Zm9vYmFy");
    }

    #[test]
    fn decimal_encoding_handles_zero_and_wide_values() {
        let mut out = Vec::new();
        push_decimal(&mut out, 0);
        push_decimal(&mut out, 4294967295);
        assert_eq!(out, b"04294967295");
    }
}
