//! A small pixel canvas with optional alpha, used to render the well layers.
//!
//! Packed `f=24` RGB suits the opaque board layer; `f=32` RGBA lets the motion
//! layer paint a piece, a ghost and flashes over the board while everything else
//! stays fully transparent (which also deflates to almost nothing).

use crate::term::fb::Rgb;

/// Wire format for a canvas.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PixelFormat {
    /// 24-bit packed RGB (`f=24`).
    Rgb,
    /// 32-bit packed RGBA with straight alpha (`f=32`).
    Rgba,
}

impl PixelFormat {
    pub fn channels(self) -> u8 {
        match self {
            Self::Rgb => 3,
            Self::Rgba => 4,
        }
    }

    pub fn kitty_code(self) -> u8 {
        match self {
            Self::Rgb => 24,
            Self::Rgba => 32,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PixelCanvas {
    width: u32,
    height: u32,
    format: PixelFormat,
    pixels: Vec<u8>,
}

impl PixelCanvas {
    pub fn new(width: u32, height: u32, format: PixelFormat) -> Self {
        let len = width as usize * height as usize * usize::from(format.channels());
        Self {
            width,
            height,
            format,
            pixels: vec![0u8; len],
        }
    }

    pub fn width(&self) -> u32 {
        self.width
    }

    pub fn height(&self) -> u32 {
        self.height
    }

    pub fn format(&self) -> PixelFormat {
        self.format
    }

    /// Packed pixel bytes, row-major.
    pub fn data(&self) -> &[u8] {
        &self.pixels
    }

    /// Mutable packed pixel bytes (tests and procedural fills).
    pub fn data_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    /// Resize, reusing the allocation. Contents become transparent black.
    pub fn resize(&mut self, width: u32, height: u32) {
        if self.width == width && self.height == height {
            self.pixels.fill(0);
            return;
        }
        self.width = width;
        self.height = height;
        let len = width as usize * height as usize * usize::from(self.format.channels());
        self.pixels.clear();
        self.pixels.resize(len, 0);
    }

    /// Reset every pixel to transparent black.
    pub fn clear(&mut self) {
        self.pixels.fill(0);
    }

    pub fn pixel(&self, x: u32, y: u32) -> Option<Rgb> {
        let index = self.index(x, y)?;
        Some(Rgb::new(
            self.pixels[index],
            self.pixels[index + 1],
            self.pixels[index + 2],
        ))
    }

    /// Alpha of one pixel (`255` for an RGB canvas).
    pub fn alpha(&self, x: u32, y: u32) -> Option<u8> {
        let index = self.index(x, y)?;
        Some(match self.format {
            PixelFormat::Rgb => 255,
            PixelFormat::Rgba => self.pixels[index + 3],
        })
    }

    #[inline]
    fn index(&self, x: u32, y: u32) -> Option<usize> {
        if x >= self.width || y >= self.height {
            return None;
        }
        Some((y as usize * self.width as usize + x as usize) * usize::from(self.format.channels()))
    }

    /// Fill the whole canvas with an opaque colour.
    pub fn fill(&mut self, color: Rgb) {
        self.blend_rect(0, 0, self.width, self.height, color, 255);
    }

    /// Fill a rectangle with an opaque colour, clipped to the canvas.
    pub fn fill_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Rgb) {
        self.blend_rect(x, y, w, h, color, 255);
    }

    /// Blend a colour over a rectangle with `alpha` in `0..=255`.
    ///
    /// On an RGBA canvas this is a straight-alpha "over" composite, so a layer
    /// can be rebuilt from transparent without any bookkeeping.
    pub fn blend_rect(&mut self, x: u32, y: u32, w: u32, h: u32, color: Rgb, alpha: u8) {
        let end_x = x.saturating_add(w).min(self.width);
        let end_y = y.saturating_add(h).min(self.height);
        if x >= end_x || y >= end_y || alpha == 0 {
            return;
        }
        let channels = usize::from(self.format.channels());
        let src = [color.r, color.g, color.b];
        let src_a = u32::from(alpha);
        let stride = self.width as usize * channels;
        for row in y..end_y {
            let start = row as usize * stride + x as usize * channels;
            let end = row as usize * stride + end_x as usize * channels;
            let mut index = start;
            while index < end {
                if channels == 4 {
                    let dst_a = u32::from(self.pixels[index + 3]);
                    // Straight-alpha "over": out_a = sa + da * (1 - sa).
                    let out_a = src_a + dst_a * (255 - src_a) / 255;
                    if out_a == 0 {
                        index += 4;
                        continue;
                    }
                    for (channel, src_channel) in src.iter().enumerate() {
                        let dst = u32::from(self.pixels[index + channel]);
                        let numerator =
                            u32::from(*src_channel) * src_a + dst * dst_a * (255 - src_a) / 255;
                        self.pixels[index + channel] = (numerator / out_a).min(255) as u8;
                    }
                    self.pixels[index + 3] = out_a.min(255) as u8;
                } else {
                    for (channel, src_channel) in src.iter().enumerate() {
                        let dst = u32::from(self.pixels[index + channel]);
                        self.pixels[index + channel] =
                            ((u32::from(*src_channel) * src_a + dst * (255 - src_a)) / 255) as u8;
                    }
                }
                index += channels;
            }
        }
    }

    /// Blend one colour over the whole canvas.
    pub fn blend_all(&mut self, color: Rgb, alpha: u8) {
        let (w, h) = (self.width, self.height);
        self.blend_rect(0, 0, w, h, color, alpha);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fill_rect_clips_to_the_canvas() {
        let mut canvas = PixelCanvas::new(4, 3, PixelFormat::Rgb);
        canvas.fill(Rgb::new(1, 2, 3));
        canvas.fill_rect(2, 1, 99, 99, Rgb::new(9, 9, 9));

        assert_eq!(canvas.pixel(1, 1), Some(Rgb::new(1, 2, 3)));
        assert_eq!(canvas.pixel(2, 1), Some(Rgb::new(9, 9, 9)));
        assert_eq!(canvas.pixel(3, 2), Some(Rgb::new(9, 9, 9)));
        assert_eq!(canvas.data().len(), 4 * 3 * 3);
    }

    #[test]
    fn blend_rect_interpolates_toward_the_source() {
        let mut canvas = PixelCanvas::new(1, 1, PixelFormat::Rgb);
        canvas.fill(Rgb::new(0, 0, 0));
        canvas.blend_rect(0, 0, 1, 1, Rgb::new(255, 255, 255), 128);
        assert_eq!(canvas.pixel(0, 0), Some(Rgb::new(128, 128, 128)));
    }

    #[test]
    fn rgba_canvas_starts_transparent_and_blends_over() {
        let mut canvas = PixelCanvas::new(2, 1, PixelFormat::Rgba);
        assert_eq!(canvas.alpha(0, 0), Some(0));
        assert_eq!(canvas.data().len(), 2 * 4);

        canvas.fill_rect(0, 0, 1, 1, Rgb::new(255, 0, 0));
        assert_eq!(canvas.alpha(0, 0), Some(255));
        assert_eq!(canvas.alpha(1, 0), Some(0), "untouched pixels stay clear");

        // Blending half-transparent white over opaque red must stay opaque.
        canvas.blend_rect(0, 0, 1, 1, Rgb::new(255, 255, 255), 128);
        assert_eq!(canvas.alpha(0, 0), Some(255));
        assert_eq!(canvas.pixel(0, 0), Some(Rgb::new(255, 128, 128)));
    }

    #[test]
    fn resize_reuses_the_allocation_and_keeps_the_format() {
        let mut canvas = PixelCanvas::new(8, 8, PixelFormat::Rgba);
        let capacity = canvas.pixels.capacity();
        canvas.resize(4, 4);
        assert!(canvas.pixels.capacity() >= capacity);
        assert_eq!(canvas.data().len(), 4 * 4 * 4);
        assert_eq!(canvas.format(), PixelFormat::Rgba);
    }
}
