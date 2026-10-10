//! Minimal but functional Truevision TGA decoder.
//!
//! Supports the TGA variants used by White Album 2: color-mapped (8-bit
//! indexed font sheets), uncompressed/RLE truecolor (CGs and sprites), and
//! grayscale. Output is always 32-bit RGBA, top-down row order.
//!
//! NOTE: For White Album 2 font sheets the palette's stored alpha is not
//! reliable (the white glyph entries have alpha 0); transparency is instead
//! encoded as palette index 0 (fully transparent). We key on index 0.

#[derive(Clone)]
pub struct TgaImage {
    pub width: u32,
    pub height: u32,
    pub rgba: Vec<u8>,
}

pub fn decode(data: &[u8]) -> Result<TgaImage, String> {
    if data.len() < 18 {
        return Err("TGA too short".into());
    }
    let id_len = data[0] as usize;
    let cmap_type = data[1];
    let img_type = data[2];
    let cmap_start = u16::from_le_bytes([data[3], data[4]]) as usize;
    let cmap_len = u16::from_le_bytes([data[5], data[6]]) as usize;
    let cmap_depth = data[7] as usize;
    let width = u16::from_le_bytes([data[12], data[13]]) as usize;
    let height = u16::from_le_bytes([data[14], data[15]]) as usize;
    let depth = data[16] as usize;
    let desc = data[17];
    if width == 0 || height == 0 {
        return Err("TGA zero dimension".into());
    }

    let mut pos = 18 + id_len;

    // Color map (for indexed images).
    let cmap = if cmap_type != 0 && cmap_len > 0 {
        let bytes_per = (cmap_depth / 8).max(1);
        let total = cmap_len * bytes_per;
        if pos + total > data.len() {
            return Err("TGA colormap out of bounds".into());
        }
        let c = data[pos..pos + total].to_vec();
        pos += total;
        Some((cmap_start, cmap_depth, c))
    } else {
        None
    };

    // WA2's char.pak sprites store `depth = 0` for 32-bit truecolor images
    // (the standard depth byte was left zero); the payload length confirms 4
    // bytes per pixel. Treat depth 0 on truecolor images as 32-bit.
    let pixel_bytes = if depth == 0 && matches!(img_type & 0x7, 2) {
        4
    } else {
        (depth / 8).max(1)
    };
    let expected = width * height * pixel_bytes;
    let rle = matches!(img_type, 9 | 10 | 11);
    let base_type = img_type & 0x7; // 1=indexed, 2=truecolor, 3=gray

    let mut raw = Vec::with_capacity(expected);
    if rle {
        while raw.len() < expected && pos < data.len() {
            let h = data[pos];
            pos += 1;
            let packets = (h & 0x7F) as usize + 1;
            if h & 0x80 != 0 {
                if pos + pixel_bytes > data.len() {
                    break;
                }
                let px = data[pos..pos + pixel_bytes].to_vec();
                pos += pixel_bytes;
                for _ in 0..packets {
                    raw.extend_from_slice(&px);
                }
            } else {
                let need = packets * pixel_bytes;
                if pos + need > data.len() {
                    break;
                }
                raw.extend_from_slice(&data[pos..pos + need]);
                pos += need;
            }
        }
    } else if pos + expected <= data.len() {
        raw.extend_from_slice(&data[pos..pos + expected]);
    } else {
        let take = (data.len() - pos).min(expected);
        raw.extend_from_slice(&data[pos..pos + take]);
    }

    let mut rgba = Vec::with_capacity(width * height * 4);
    let flip = (desc & 0x20) == 0; // bit5==0 => bottom-up
    for y in 0..height {
        let sy = if flip { height - 1 - y } else { y };
        for x in 0..width {
            let idx = (sy * width + x) * pixel_bytes;
            match base_type {
                1 => {
                    let (cstart, cdepth, cmap) =
                        cmap.as_ref().ok_or_else(|| "indexed TGA without colormap".to_string())?;
                    let cb = (cdepth / 8).max(1);
                    let ci = raw[idx] as usize;
                    if ci == 0 {
                        // Transparent color key.
                        rgba.extend_from_slice(&[0, 0, 0, 0]);
                    } else {
                        let entry = ci.saturating_sub(*cstart) * cb;
                        if entry + cb <= cmap.len() {
                            // BGRA (cb==4) or BGR (cb==3) palette. Font atlases use a
                            // black/transparent palette entry as the background (the
                            // background index is 0 in some atlases and 2 in others),
                            // and store glyphs as near-white entries whose palette ALPHA
                            // is unreliable (often 0). So: black entry => transparent,
                            // any non-black entry => opaque glyph pixel.
                            let (r, g, b) = (cmap[entry + 2], cmap[entry + 1], cmap[entry]);
                            if r == 0 && g == 0 && b == 0 {
                                rgba.extend_from_slice(&[0, 0, 0, 0]);
                            } else {
                                rgba.extend_from_slice(&[r, g, b, 255]);
                            }
                        } else {
                            rgba.extend_from_slice(&[0, 0, 0, 255]);
                        }
                    }
                }
                2 => match pixel_bytes {
                    2 => {
                        let v = u16::from_le_bytes([raw[idx], raw[idx + 1]]);
                        let r = (((v >> 10) & 0x1F) * 255 / 31) as u8;
                        let g = (((v >> 5) & 0x1F) * 255 / 31) as u8;
                        let b = ((v & 0x1F) * 255 / 31) as u8;
                        rgba.extend_from_slice(&[r, g, b, 255]);
                    }
                    4 => {
                        rgba.push(raw[idx + 2]);
                        rgba.push(raw[idx + 1]);
                        rgba.push(raw[idx]);
                        rgba.push(raw[idx + 3]);
                    }
                    _ => {
                        rgba.push(raw[idx + 2]);
                        rgba.push(raw[idx + 1]);
                        rgba.push(raw[idx]);
                        rgba.push(255);
                    }
                },
                3 => {
                    let v = raw[idx];
                    rgba.extend_from_slice(&[v, v, v, 255]);
                }
                _ => rgba.extend_from_slice(&[255, 0, 255, 255]),
            }
        }
    }

    Ok(TgaImage {
        width: width as u32,
        height: height as u32,
        rgba,
    })
}
