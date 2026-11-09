//! Shared BGRA → planar color conversion for the Linux encoders (BT.601
//! limited range, nearest-neighbour scaling folded into the row walk). Rows
//! are split across scoped threads so a 1080p pass stays well under the
//! 60 fps frame budget; both encoders feed from the same compositor frames,
//! so the source validation lives here too.

/// One compositor frame in BGRA byte order (wl_shm ARGB8888 is little-endian
/// XRGB, i.e. B,G,R,X in memory), the shape both Linux encoders consume.
pub(crate) struct BgraFrame<'a> {
    pub pixels: &'a [u8],
    pub width: u32,
    pub height: u32,
    /// Bytes per source row; may exceed width*4.
    pub stride: u32,
}

impl<'a> BgraFrame<'a> {
    pub(crate) fn new(
        pixels: &'a [u8],
        width: u32,
        height: u32,
        stride: u32,
    ) -> Result<Self, String> {
        let frame = Self {
            pixels,
            width,
            height,
            stride,
        };
        frame.validate()?;
        Ok(frame)
    }

    /// Reject malformed compositor frames before any conversion indexes them.
    fn validate(&self) -> Result<(), String> {
        if self.width == 0 || self.height == 0 {
            return Err("capture frame has empty dimensions".into());
        }
        // Every row must hold a full BGRA quad per pixel, or row_slice would
        // panic on a malformed compositor stride.
        if self.stride < self.width * 4 {
            return Err(format!(
                "capture stride {} is narrower than {} pixels",
                self.stride,
                self.width * 4
            ));
        }
        let expected = (self.stride as usize)
            .checked_mul(self.height as usize)
            .ok_or("capture frame size overflow")?;
        if self.pixels.len() < expected {
            return Err(format!(
                "capture frame too small: {} < {expected}",
                self.pixels.len()
            ));
        }
        Ok(())
    }
}

/// BGRA → I420 (planar Y, planar U, planar V), scaled to `dst_width` ×
/// `dst_height`. `i420` must be exactly `dst_width * dst_height * 3 / 2`.
pub(crate) fn bgra_to_i420(
    i420: &mut [u8],
    dst_width: u32,
    dst_height: u32,
    src: &BgraFrame,
) {
    let dst_w = dst_width as usize;
    let dst_h = dst_height as usize;
    debug_assert_eq!(i420.len(), dst_w * dst_h * 3 / 2);
    let (y_plane, uv_plane) = i420.split_at_mut(dst_w * dst_h);
    let (u_plane, v_plane) = uv_plane.split_at_mut(dst_w * dst_h / 4);
    let y_plane: &mut [u8] = y_plane;
    let u_plane: &mut [u8] = u_plane;
    let v_plane: &mut [u8] = v_plane;

    let mut y_rows: Vec<&mut [u8]> = y_plane.chunks_mut(dst_w).collect();
    let mut u_rows: Vec<&mut [u8]> = u_plane.chunks_mut(dst_w / 2).collect();
    let mut v_rows: Vec<&mut [u8]> = v_plane.chunks_mut(dst_w / 2).collect();

    let chroma_rows = dst_h / 2;
    std::thread::scope(|scope| {
        let luma = scope.spawn(|| {
            write_luma_rows(&mut y_rows, dst_w, dst_h, src);
        });
        let chroma = scope.spawn(|| {
            write_i420_chroma_rows(&mut u_rows, &mut v_rows, chroma_rows, dst_w, dst_h, src);
        });
        let _ = (luma.join(), chroma.join());
    });
}

/// BGRA → NV12 (planar Y, interleaved UV), scaled to `dst_width` ×
/// `dst_height` with rows of `dst_pitch` bytes (pitch ≥ width; padding bytes
/// are left untouched). `nv12` must be `dst_pitch * dst_height * 3 / 2` —
/// the layout NVENC expects for a registered `NV_ENC_BUFFER_FORMAT_NV12`
/// surface. Row stripes are spread across the machine's cores so a 1080p
/// pass stays a few milliseconds.
pub(crate) fn bgra_to_nv12(
    nv12: &mut [u8],
    dst_pitch: u32,
    dst_width: u32,
    dst_height: u32,
    src: &BgraFrame,
) {
    let dst_w = dst_width as usize;
    let dst_h = dst_height as usize;
    let pitch = dst_pitch as usize;
    debug_assert!(pitch >= dst_w);
    debug_assert_eq!(nv12.len(), pitch * dst_h * 3 / 2);
    let (y_plane, uv_plane) = nv12.split_at_mut(pitch * dst_h);

    // Even stripe boundaries so every luma stripe maps onto whole chroma
    // rows; each thread owns a disjoint Y region plus its matching UV rows.
    let threads = std::thread::available_parallelism()
        .map(|n| n.get())
        .unwrap_or(2)
        .min(32)
        .min(dst_h)
        .max(1);
    let stripe_rows = dst_h.div_ceil(threads) + (dst_h.div_ceil(threads) & 1);

    // Pre-split into disjoint stripes before the scope: `split_at_mut`
    // consumes the remaining slice each step so earlier stripes stay valid.
    let mut y_stripes: Vec<&mut [u8]> = Vec::with_capacity(threads);
    let mut uv_stripes: Vec<&mut [u8]> = Vec::with_capacity(threads);
    let mut stripe_bounds: Vec<(usize, usize)> = Vec::with_capacity(threads);
    let mut y_rest: &mut [u8] = y_plane;
    let mut uv_rest: &mut [u8] = uv_plane;
    let mut start_row = 0;
    while start_row < dst_h {
        let end_row = (start_row + stripe_rows).min(dst_h);
        if start_row == end_row {
            break;
        }
        let rows = end_row - start_row;
        let (head_y, tail_y) = y_rest.split_at_mut(rows * pitch);
        let (head_uv, tail_uv) = uv_rest.split_at_mut((rows / 2) * pitch);
        y_stripes.push(head_y);
        uv_stripes.push(head_uv);
        stripe_bounds.push((start_row, start_row / 2));
        y_rest = tail_y;
        uv_rest = tail_uv;
        start_row = end_row;
    }

    std::thread::scope(|scope| {
        for ((y_stripe, uv_stripe), (start_row, cy_start)) in y_stripes
            .iter_mut()
            .zip(uv_stripes.iter_mut())
            .zip(stripe_bounds.iter().copied())
        {
            scope.spawn(move || {
                for (dy, y_row) in y_stripe.chunks_mut(pitch).enumerate() {
                    let sy = ((start_row + dy) * src.height as usize / dst_h)
                        .min(src.height as usize - 1);
                    let src_row = row_slice(src, sy);
                    // Rows are `pitch` wide; only `dst_w` pixels are written.
                    for (dx, ypx) in y_row.iter_mut().enumerate().take(dst_w) {
                        let sx = dx * src.width as usize / dst_w;
                        let (b, g, r) = sample_bgra(src_row, sx);
                        // BT.601 limited range: Y = (66R + 129G + 25B + 128)>>8 + 16
                        let y = (66 * r as i32 + 129 * g as i32 + 25 * b as i32 + 128) >> 8;
                        *ypx = (y + 16).clamp(0, 255) as u8;
                    }
                }
                for (cy, uv_row) in uv_stripe.chunks_mut(pitch).enumerate() {
                    let cy = cy_start + cy;
                    let sy = (cy * 2 * src.height as usize / dst_h / 2 * 2)
                        .min(src.height as usize - 2);
                    let row0 = row_slice(src, sy);
                    let row1 = row_slice(src, sy + 1);
                    for cx in 0..dst_w / 2 {
                        let sx = (cx * 2 * src.width as usize / dst_w / 2 * 2)
                            .min(src.width as usize - 2);
                        let (b0, g0, r0) = sample_bgra(row0, sx);
                        let (b1, g1, r1) = sample_bgra(row0, sx + 1);
                        let (b2, g2, r2) = sample_bgra(row1, sx);
                        let (b3, g3, r3) = sample_bgra(row1, sx + 1);
                        let b = b0 as i32 + b1 as i32 + b2 as i32 + b3 as i32;
                        let g = g0 as i32 + g1 as i32 + g2 as i32 + g3 as i32;
                        let r = r0 as i32 + r1 as i32 + r2 as i32 + r3 as i32;
                        let u = (-38 * r - 74 * g + 112 * b + 512) >> 10;
                        let v = (112 * r - 94 * g - 18 * b + 512) >> 10;
                        uv_row[2 * cx] = (u + 128).clamp(0, 255) as u8;
                        uv_row[2 * cx + 1] = (v + 128).clamp(0, 255) as u8;
                    }
                }
            });
        }
    });
}

fn write_luma_rows(y_rows: &mut [&mut [u8]], dst_w: usize, dst_h: usize, src: &BgraFrame) {
    for (dy, row) in y_rows.iter_mut().enumerate() {
        let sy = (dy * src.height as usize / dst_h).min(src.height as usize - 1);
        let src_row = row_slice(src, sy);
        for dx in 0..dst_w {
            let sx = dx * src.width as usize / dst_w;
            let (b, g, r) = sample_bgra(src_row, sx);
            // BT.601 limited range: Y = (66R + 129G + 25B + 128)>>8 + 16
            let y = (66 * r as i32 + 129 * g as i32 + 25 * b as i32 + 128) >> 8;
            row[dx] = (y + 16).clamp(0, 255) as u8;
        }
    }
}

fn write_i420_chroma_rows(
    u_rows: &mut [&mut [u8]],
    v_rows: &mut [&mut [u8]],
    chroma_rows: usize,
    dst_w: usize,
    dst_h: usize,
    src: &BgraFrame,
) {
    for cy in 0..chroma_rows {
        let sy = (cy * 2 * src.height as usize / dst_h / 2 * 2).min(src.height as usize - 2);
        let row0 = row_slice(src, sy);
        let row1 = row_slice(src, sy + 1);
        let (u_row, v_row) = (&mut u_rows[cy], &mut v_rows[cy]);
        for cx in 0..dst_w / 2 {
            let sx = (cx * 2 * src.width as usize / dst_w / 2 * 2).min(src.width as usize - 2);
            let (b0, g0, r0) = sample_bgra(row0, sx);
            let (b1, g1, r1) = sample_bgra(row0, sx + 1);
            let (b2, g2, r2) = sample_bgra(row1, sx);
            let (b3, g3, r3) = sample_bgra(row1, sx + 1);
            let b = b0 as i32 + b1 as i32 + b2 as i32 + b3 as i32;
            let g = g0 as i32 + g1 as i32 + g2 as i32 + g3 as i32;
            let r = r0 as i32 + r1 as i32 + r2 as i32 + r3 as i32;
            // 2x2 average first (values are 4x the mean).
            let u = (-38 * r - 74 * g + 112 * b + 512) >> 10;
            let v = (112 * r - 94 * g - 18 * b + 512) >> 10;
            u_row[cx] = (u + 128).clamp(0, 255) as u8;
            v_row[cx] = (v + 128).clamp(0, 255) as u8;
        }
    }
}

fn row_slice<'a>(src: &'a BgraFrame<'a>, y: usize) -> &'a [u8] {
    let start = y * src.stride as usize;
    let len = src.width as usize * 4;
    &src.pixels[start..start + len]
}

fn sample_bgra(row: &[u8], x: usize) -> (u8, u8, u8) {
    let p = x * 4;
    (row[p], row[p + 1], row[p + 2]) // B, G, R
}

#[cfg(test)]
mod tests {
    use super::*;

    fn solid_pixels(b: u8, g: u8, r: u8, width: usize, height: usize) -> Vec<u8> {
        let mut pixels = vec![0u8; width * height * 4];
        for px in pixels.as_chunks_mut::<4>().0 {
            px[0] = b;
            px[1] = g;
            px[2] = r;
            px[3] = 255;
        }
        pixels
    }

    fn frame_of(pixels: &[u8], width: usize, height: usize) -> BgraFrame<'_> {
        BgraFrame {
            pixels,
            width: width as u32,
            height: height as u32,
            stride: (width * 4) as u32,
        }
    }

    /// Pure white: BT.601 limited-range Y=235 with neutral chroma 128.
    #[test]
    fn nv12_solid_white_has_limited_range_white_and_neutral_chroma() {
        let pixels = solid_pixels(255, 255, 255, 4, 4);
        let src = frame_of(&pixels, 4, 4);
        let mut nv12 = vec![0u8; 4 * 4 * 3 / 2];
        bgra_to_nv12(&mut nv12, 4, 4, 4, &src);
        assert!(nv12[..16].iter().all(|&y| y == 235), "Y plane: {:?}", &nv12[..16]);
        assert!(
            nv12[16..].iter().all(|&c| c == 128),
            "UV plane: {:?}",
            &nv12[16..]
        );
    }

    /// Pure red: deterministic U/V placement proves the interleaved layout
    /// (U at even offsets, V at odd offsets) and the BT.601 coefficients.
    #[test]
    fn nv12_solid_red_matches_expected_limited_range_values() {
        let pixels = solid_pixels(0, 0, 255, 4, 4);
        let src = frame_of(&pixels, 4, 4);
        let mut nv12 = vec![0u8; 4 * 4 * 3 / 2];
        bgra_to_nv12(&mut nv12, 4, 4, 4, &src);
        assert!(nv12[..16].iter().all(|&y| y == 82));
        for pair in nv12[16..].as_chunks::<2>().0 {
            assert_eq!(pair[0], 90, "U");
            assert_eq!(pair[1], 240, "V");
        }
    }

    /// The I420 conversion keeps its existing output: white maps to Y=235,
    /// U=V=128 in the planar layout.
    #[test]
    fn i420_solid_white_keeps_planar_layout() {
        let pixels = solid_pixels(255, 255, 255, 4, 4);
        let src = frame_of(&pixels, 4, 4);
        let mut i420 = vec![0u8; 4 * 4 * 3 / 2];
        bgra_to_i420(&mut i420, 4, 4, &src);
        assert!(i420[..16].iter().all(|&y| y == 235));
        assert!(i420[16..24].iter().all(|&u| u == 128));
        assert!(i420[24..].iter().all(|&v| v == 128));
    }

    #[test]
    fn rejects_malformed_frames() {
        assert!(BgraFrame::new(&[0u8; 8], 2, 2, 8).is_err());
        assert!(BgraFrame::new(&[0u8; 16], 2, 2, 8).is_ok());
        assert!(BgraFrame::new(&[0u8; 16], 2, 0, 8).is_err());
        assert!(BgraFrame::new(&[0u8; 15], 2, 2, 8).is_err());
    }
}
