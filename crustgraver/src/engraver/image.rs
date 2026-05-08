use anyhow::{bail, Result};
use image::{DynamicImage, GenericImageView, Pixel};

use super::protocol::{MAX_HEIGHT, MAX_WIDTH};

/// Encode a `DynamicImage` into the raw 1-bit packet the engraver expects.
///
/// The device protocol requires:
///   - Width rounded up to the nearest multiple of 8 (`wr`)
///   - Every odd row is mirrored horizontally (boustrophedon scan)
///   - Pixels packed MSB-first into bytes (dark pixel = 1)
///   - Terminated with 0x55
pub fn encode_image(img: &DynamicImage) -> Result<Vec<u8>> {
    let w = img.width() as usize;
    let h = img.height() as usize;

    if w > MAX_WIDTH as usize || h > MAX_HEIGHT as usize {
        bail!(
            "Image too large ({w}×{h}). Max is {MAX_WIDTH}×{MAX_HEIGHT}."
        );
    }

    // round width up to multiple of 8
    let wr = (w + 7) & !7;
    let total_bytes = (wr * h) / 8;

    let mut out = Vec::with_capacity(total_bytes + 1);

    for row in 0..h {
        let mut byte: u8 = 0;

        for col in 0..wr {
            // bit position within current byte (MSB first)
            let bit_pos = 7 - (col % 8);

            // on odd rows reverse the column (boustrophedon)
            let src_col = if row % 2 == 1 {
                w.saturating_sub(1).saturating_sub(col)
            } else {
                col
            };

            // pixels beyond image width are treated as white (0)
            let dark = if src_col < w {
                let px = img.get_pixel(src_col as u32, row as u32);
                let luma = px.to_luma()[0]; // 0 = black, 255 = white
                luma < 128
            } else {
                false
            };

            if dark {
                byte |= 1 << bit_pos;
            }

            // flush byte every 8 columns
            if bit_pos == 0 {
                out.push(byte);
                byte = 0;
            }
        }
        // `wr` is always a multiple of 8 so the last byte is always flushed above
    }

    out.push(0x55); // packet terminator
    Ok(out)
}

/// Load an image from disk and return it as a `DynamicImage`.
pub fn load_image(path: &str) -> Result<DynamicImage> {
    let img = image::open(path)
        .map_err(|e| anyhow::anyhow!("Failed to load image '{path}': {e}"))?;
    Ok(img)
}

// ── helpers for the protocol layer ───────────────────────────────────────────

/// Returns `(wr, h, le)` — the three values needed by `protocol::image_info`.
pub fn image_dimensions(img: &DynamicImage) -> (u16, u16, u32) {
    let w  = img.width() as u16;
    let h  = img.height() as u16;
    let wr = (w + 7) & !7;
    let le = (wr as u32 * h as u32) / 8;
    (wr, h, le)
}
