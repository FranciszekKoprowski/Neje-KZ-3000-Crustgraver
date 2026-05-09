
use anyhow::{bail, Result};
use image::{DynamicImage, GenericImageView, Pixel};
 
use super::protocol::{MAX_HEIGHT, MAX_WIDTH};
 
/// Encode a `DynamicImage` into the raw 1-bit payload the engraver expects,
/// with 0x55 bytes inside the data escaped so the serial framing isn't broken.
///
/// Protocol details:
///   - Convert to grayscale; pixels with luma < 128 are "dark" (laser on)
///   - Width rounded up to nearest multiple of 8 (`wr`)
///   - Every odd row is mirrored horizontally (boustrophedon scan)
///   - Pixels packed MSB-first into bytes (dark pixel = 1 = laser on)
///   - Any 0x55 byte in the bitstream is escaped as 0x55 0x00
///     (matches NEJE firmware behaviour observed in the wild)
///   - Terminated with 0x55 (un-escaped end-of-packet marker)
pub fn encode_image(img: &DynamicImage) -> Result<Vec<u8>> {
    let w = img.width()  as usize;
    let h = img.height() as usize;
 
    if w > MAX_WIDTH as usize || h > MAX_HEIGHT as usize {
        bail!(
            "Image too large ({w}×{h}). Max is {MAX_WIDTH}×{MAX_HEIGHT}."
        );
    }
 
    // round width up to multiple of 8
    let wr = (w + 7) & !7;
    let total_bytes = (wr * h) / 8;
 
    // pre-allocate with a little headroom for escape bytes
    let mut out = Vec::with_capacity(total_bytes + total_bytes / 4 + 1);
 
    for row in 0..h {
        let mut byte: u8 = 0;
 
        for col in 0..wr {
            let bit_pos = 7 - (col % 8);   // MSB first
 
            // boustrophedon: odd rows scan right-to-left
            let src_col = if row % 2 == 1 {
                // mirror within the actual image width, pad cols beyond w are white
                if col < w { w - 1 - col } else { w } // w is out-of-range → white
            } else {
                col
            };
 
            let dark = src_col < w && {
                let px   = img.get_pixel(src_col as u32, row as u32);
                let rgba = px.to_rgba();
                // respect alpha: fully transparent → white (not burned)
                let alpha = rgba[3];
                if alpha < 128 {
                    false
                } else {
                    let luma = (0.299 * rgba[0] as f32
                              + 0.587 * rgba[1] as f32
                              + 0.114 * rgba[2] as f32) as u8;
                    luma < 128
                }
            };
 
            if dark {
                byte |= 1 << bit_pos;
            }
 
            // flush every 8 columns
            if bit_pos == 0 {
                push_escaped(&mut out, byte);
                byte = 0;
            }
        }
        // wr is always a multiple of 8, so the last byte is always flushed above
    }
 
    out.push(0x55); // end-of-packet terminator (never escaped)
    Ok(out)
}
 
/// Push one data byte, escaping 0x55 so the serial reader doesn't
/// treat it as an end-of-packet marker.
#[inline]
fn push_escaped(buf: &mut Vec<u8>, b: u8) {
    buf.push(b);
    if b == 0x55 {
        buf.push(0x00); // escape byte
    }
}
 
/// Load an image from disk and return it as a `DynamicImage`.
pub fn load_image(path: &str) -> Result<DynamicImage> {
    let img = image::open(path)
        .map_err(|e| anyhow::anyhow!("Failed to load image '{path}': {e}"))?;
    Ok(img)
}
 
/// Returns `(wr, h, le)` — the three values needed by `protocol::image_info`.
/// `le` counts the unescaped bytes (what the device expects in the header).
pub fn image_dimensions(img: &DynamicImage) -> (u16, u16, u32) {
    let w  = img.width()  as u16;
    let h  = img.height() as u16;
    let wr = (w + 7) & !7;
    let le = (wr as u32 * h as u32) / 8;
    (wr, h, le)
}
 
/// Apply a simple threshold to convert a colour image to a crisp 1-bit look
/// before encoding.  Threshold 0-255; lower = more dark pixels.
/// If `invert` is true, the result is flipped: white becomes burned, black skipped.
/// Use this for white logos on dark backgrounds.
pub fn threshold_image(img: &DynamicImage, threshold: u8, invert: bool) -> DynamicImage {
    use image::{GrayImage, Luma};
    let gray = img.to_luma8();
    let mut out = GrayImage::new(gray.width(), gray.height());
    for (x, y, px) in gray.enumerate_pixels() {
        let dark = px[0] < threshold;
        let burn = if invert { !dark } else { dark };
        let v = if burn { 0u8 } else { 255u8 };
        out.put_pixel(x, y, Luma([v]));
    }
    DynamicImage::ImageLuma8(out)
}

