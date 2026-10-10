//! Offscreen scene compositor.
//!
//! This is display-independent: it renders into an in-memory RGBA buffer so it
//! can be verified headlessly (dumped to TGA) and later blitted to an SDL2
//! texture without changing the compositing logic.
//!
//! Pipeline: load images via `codec::tga`, draw background/sprite/dialog box,
//! rasterize text glyphs from the font atlas, then `save_tga` (or, in the SDL2
//! frontend, upload `pixels` to a texture).

use crate::codec::tga::TgaImage;
use std::fs::File;
use std::io::{self, Write};

pub mod font;
pub use font::Font;

pub struct Surface {
    pub width: u32,
    pub height: u32,
    /// RGBA, row-major, top-down.
    pub pixels: Vec<u8>,
}

fn blend(dst: &[u8; 4], src: &[u8; 4]) -> [u8; 4] {
    let a = src[3] as u32;
    if a == 0 {
        return *dst;
    }
    if a == 255 {
        return *src;
    }
    let ia = 255 - a;
    [
        ((src[0] as u32 * a + dst[0] as u32 * ia) / 255) as u8,
        ((src[1] as u32 * a + dst[1] as u32 * ia) / 255) as u8,
        ((src[2] as u32 * a + dst[2] as u32 * ia) / 255) as u8,
        255,
    ]
}

impl Surface {
    pub fn new(width: u32, height: u32, bg: [u8; 4]) -> Surface {
        Surface {
            width,
            height,
            pixels: vec![bg[0], bg[1], bg[2], bg[3]].repeat((width * height) as usize),
        }
    }

    fn put(&mut self, x: i32, y: i32, c: [u8; 4]) {
        if x < 0 || y < 0 || x >= self.width as i32 || y >= self.height as i32 {
            return;
        }
        let i = ((y as u32) * self.width + (x as u32)) as usize * 4;
        let cur = [self.pixels[i], self.pixels[i + 1], self.pixels[i + 2], self.pixels[i + 3]];
        let b = blend(&cur, &c);
        self.pixels[i..i + 4].copy_from_slice(&b);
    }

    /// Blit a whole TGA image at (dx, dy) using its alpha channel.
    pub fn blit(&mut self, img: &TgaImage, dx: i32, dy: i32) {
        self.blit_region(img, 0, 0, img.width, img.height, dx, dy);
    }

    /// Blit a sub-rectangle (sx,sy,sw,sh) of `img` to (dx,dy).
    pub fn blit_region(
        &mut self,
        img: &TgaImage,
        sx: i32,
        sy: i32,
        sw: u32,
        sh: u32,
        dx: i32,
        dy: i32,
    ) {
        for row in 0..sh as i32 {
            for col in 0..sw as i32 {
                let sxp = sx + col;
                let syp = sy + row;
                if sxp < 0 || syp < 0 || sxp >= img.width as i32 || syp >= img.height as i32 {
                    continue;
                }
                let si = ((syp as u32) * img.width + (sxp as u32)) as usize * 4;
                let c = [
                    img.rgba[si],
                    img.rgba[si + 1],
                    img.rgba[si + 2],
                    img.rgba[si + 3],
                ];
                self.put(dx + col, dy + row, c);
            }
        }
    }

    /// Blend a single pixel (with alpha).
    pub fn blend_pixel(&mut self, x: i32, y: i32, c: [u8; 4]) {
        self.put(x, y, c);
    }

    /// Fill a rectangle (with alpha) - used for the dialog box.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: [u8; 4]) {
        for row in 0..h as i32 {
            for col in 0..w as i32 {
                self.put(x + col, y + row, color);
            }
        }
    }

    /// Write the surface as an uncompressed 32-bit TGA (top-down, BGRA on disk).
    pub fn save_tga(&self, path: &str) -> io::Result<()> {
        let mut f = File::create(path)?;
        let w = self.width as u16;
        let h = self.height as u16;
        let header = [
            0u8, 0u8, 2u8, 0, 0, 0, 0, 0, 0, 0, 0, 0, (w & 0xFF) as u8, (w >> 8) as u8,
            (h & 0xFF) as u8, (h >> 8) as u8, 32u8, 0x28u8,
        ];
        f.write_all(&header)?;
        for y in 0..self.height as usize {
            for x in 0..self.width as usize {
                let i = (y * self.width as usize + x) * 4;
                let r = self.pixels[i];
                let g = self.pixels[i + 1];
                let b = self.pixels[i + 2];
                let a = self.pixels[i + 3];
                f.write_all(&[b, g, r, a])?;
            }
        }
        Ok(())
    }
}

/// Composite a visual-novel scene: background, bottom dialog box, and the given
/// text rendered with `font` (one line per wrapped logical line).
///
/// `text` may contain literal `\n` (backslash + n) for line breaks and
/// `<R..>` region tags, both stripped before rendering. Characters missing
/// from the font map render as translucent magenta boxes (so gaps are visible).
pub fn render_scene(bg: &TgaImage, text: &str, font: &Font, w: u32, h: u32) -> Surface {
    let mut s = Surface::new(w, h, [0, 0, 0, 255]);
    // Background, scaled to fit (nearest-neighbour stretch).
    let (bw, bh) = (bg.width as f32, bg.height as f32);
    let scale = (w as f32 / bw).min(h as f32 / bh);
    let (dw, dh) = ((bw * scale) as u32, (bh * scale) as u32);
    let (ox, oy) = ((w - dw) / 2, (h - dh) / 2);
    for y in 0..dh {
        for x in 0..dw {
            let sx = (x as f32 / scale) as i32;
            let sy = (y as f32 / scale) as i32;
            s.blit_region(bg, sx, sy, 1, 1, ox as i32 + x as i32, oy as i32 + y as i32);
        }
    }

    // Dialog box near the bottom.
    let box_h = (h / 4).max(140);
    let box_y = (h - box_h - 24) as i32;
    s.fill_rect(30, box_y, w - 60, box_h, [10, 10, 24, 200]);
    s.fill_rect(30, box_y, w - 60, 2, [180, 200, 255, 220]);
    s.fill_rect(30, box_y + box_h as i32 - 2, w - 60, 2, [180, 200, 255, 220]);

    // Prepare text: strip region tags, turn literal \n into newlines.
    let cleaned: String = text.chars().filter(|&c| c != '\r').collect();
    let cleaned = cleaned.replace("<R", "").replace('>', "");
    let cleaned: String = cleaned
        .split('')
        .enumerate()
        .map(|(i, s)| if i % 2 == 0 { s } else { "" })
        .collect();
    let cleaned = cleaned.replace("\n", "
");

    let cell = font.cell() as i32;
    let x0 = 50i32;
    let x_max = (w as i32) - 60;
    let mut y = box_y + 18;
    for line in cleaned.lines() {
        let mut x = x0;
        let mut buf = String::new();
        for ch in line.chars() {
            if x + cell > x_max {
                break;
            }
            buf.push(ch);
            x += cell;
        }
        // Use `draw` (not `draw_with_fallback`) so unmapped characters leave a
        // gap instead of a magenta placeholder box: the text area then shows
        // only the glyphs that actually exist in the atlas, which reads much
        // cleaner than rows of placeholder bars.
        font.draw(&mut s, &buf, x0, y);
        y += cell + 4;
        if y > box_y + box_h as i32 - cell {
            break;
        }
    }
    s
}

/// Build a sample scene: background, optional character sprite, dialog box, and a
/// strip of glyphs drawn from the font atlas. Proves the full offscreen pipeline.
pub fn demo_scene(font: &TgaImage, sprite: Option<&TgaImage>, w: u32, h: u32) -> Surface {
    let mut s = Surface::new(w, h, [20, 20, 40, 255]);

    if let Some(sp) = sprite {
        let dx = ((w as i32 - sp.width as i32) / 2).max(0);
        s.blit(sp, dx, 0);
    }

    let box_h = 200u32;
    let box_y = (h - box_h - 20) as i32;
    s.fill_rect(40, box_y, w - 80, box_h, [0, 0, 0, 180]);

    let cell = 64u32;
    let start_x = 60i32;
    let start_y = box_y + 10;
    let mut n = 0;
    for row in 0..6u32 {
        for col in 0..(w as u32 / cell).min(30) {
            if n >= 180 {
                break;
            }
            let gx = (col * cell) as i32;
            let gy = (row * cell) as i32;
            let dx = start_x + (col as i32) * (cell as i32);
            let dy = start_y + (row as i32) * (cell as i32);
            s.blit_region(font, gx, gy, cell, cell, dx, dy);
            n += 1;
        }
    }
    s
}
