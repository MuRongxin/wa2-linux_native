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
pub mod text;
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

    /// Like `blit_region` but scales the source alpha by `alpha` (0..=255),
    /// used to dim non-speaking characters.
    pub fn blit_region_alpha(
        &mut self,
        img: &TgaImage,
        sx: i32,
        sy: i32,
        sw: u32,
        sh: u32,
        dx: i32,
        dy: i32,
        alpha: u8,
    ) {
        if alpha == 255 {
            self.blit_region(img, sx, sy, sw, sh, dx, dy);
            return;
        }
        for row in 0..sh as i32 {
            for col in 0..sw as i32 {
                let sxp = sx + col;
                let syp = sy + row;
                if sxp < 0 || syp < 0 || sxp >= img.width as i32 || syp >= img.height as i32 {
                    continue;
                }
                let si = ((syp as u32) * img.width + (sxp as u32)) as usize * 4;
                let a = (img.rgba[si + 3] as u32 * alpha as u32 / 255) as u8;
                let c = [img.rgba[si], img.rgba[si + 1], img.rgba[si + 2], a];
                self.put(dx + col, dy + row, c);
            }
        }
    }

    /// Blit a raw RGBA buffer (w*h*4) at (dx,dy) with alpha blending.
    pub fn blit_rgba(&mut self, rgba: &[u8], w: u32, h: u32, dx: i32, dy: i32) {
        for row in 0..h as i32 {
            for col in 0..w as i32 {
                let si = ((row as u32) * w + col as u32) as usize * 4;
                if si + 3 >= rgba.len() { continue; }
                let c = [rgba[si], rgba[si+1], rgba[si+2], rgba[si+3]];
                self.put(dx + col, dy + row, c);
            }
        }
    }

    /// Fill a rectangle (with alpha) - used for the dialog box.
    pub fn fill_rect(&mut self, x: i32, y: i32, w: u32, h: u32, color: [u8; 4]) {
        for row in 0..h as i32 {
            for col in 0..w as i32 {
                self.put(x + col, y + row, color);
            }
        }
    }

    /// Apply one of the game's `.AMP` colour curves to every pixel.
    ///
    /// The files are plain LUTs: 256 bytes = one curve used for R, G and B
    /// (`nega.AMP` = inversion), 768 bytes = R, G, B curves (`evening.AMP`),
    /// 1280 bytes = five curves of which the first three are R, G and B.
    pub fn apply_lut(&mut self, lut: &[u8]) {
        if lut.len() < 256 {
            return;
        }
        let (r, g, b) = if lut.len() >= 768 {
            (&lut[0..256], &lut[256..512], &lut[512..768])
        } else {
            (&lut[0..256], &lut[0..256], &lut[0..256])
        };
        for px in self.pixels.chunks_exact_mut(4) {
            px[0] = r[px[0] as usize];
            px[1] = g[px[1] as usize];
            px[2] = b[px[2] as usize];
        }
    }

    /// Write the surface as an uncompressed 32-bit TGA (top-down, BGRA on disk).
    /// Box-filtered shrink of this surface, for save-slot thumbnails.
    ///
    /// The screen is 1280x720 and a slot shows it at ~242x136, i.e. every
    /// destination pixel covers about 5x5 source pixels; averaging them beats
    /// nearest-neighbour sampling, which aliases text into mush.
    pub fn thumbnail(&self, w: u32, h: u32) -> TgaImage {
        let mut rgba = Vec::with_capacity((w * h * 4) as usize);
        for y in 0..h {
            let sy0 = (y as u64 * self.height as u64 / h as u64) as u32;
            let sy1 = (((y + 1) as u64 * self.height as u64 / h as u64) as u32).max(sy0 + 1);
            for x in 0..w {
                let sx0 = (x as u64 * self.width as u64 / w as u64) as u32;
                let sx1 = (((x + 1) as u64 * self.width as u64 / w as u64) as u32).max(sx0 + 1);
                let (mut r, mut g, mut b, mut n) = (0u64, 0u64, 0u64, 0u64);
                for sy in sy0..sy1.min(self.height) {
                    for sx in sx0..sx1.min(self.width) {
                        let i = ((sy * self.width + sx) as usize) * 4;
                        r += self.pixels[i] as u64;
                        g += self.pixels[i + 1] as u64;
                        b += self.pixels[i + 2] as u64;
                        n += 1;
                    }
                }
                if n == 0 {
                    rgba.extend_from_slice(&[0, 0, 0, 255]);
                } else {
                    rgba.extend_from_slice(&[(r / n) as u8, (g / n) as u8, (b / n) as u8, 255]);
                }
            }
        }
        TgaImage { width: w, height: h, rgba }
    }

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

/// A character sprite to composite: image plus screen position and an alpha
/// multiplier (used to dim non-speaking characters).
pub struct SpriteDraw<'a> {
    pub img: &'a TgaImage,
    pub x: i32,
    pub y: i32,
    pub alpha: u8,
}

/// Composite a visual-novel scene: background, standing sprites, bottom dialog
/// box, and the given text rendered with `font` (one line per wrapped logical
/// line).
pub fn render_scene(bg: &TgaImage, text: &str, font: &Font, w: u32, h: u32) -> Surface {
    render_scene_ex(bg, text, font, w, h, &[], None, true, None, None)
}

/// `sys_00000` / `sys_00001` are blitted here. The oval's stroke runs from
/// about y=525 to y=666; glyphs start below the top glow and clear of the
/// snowflakes on either side.
pub const WINDOW_X: i32 = 16;
pub const WINDOW_Y: i32 = 496;
/// Speaker name, on the upper rim. The atlas ink starts ~7px into the cell,
/// so the cell origin sits that far above the white strokes.
pub const NAME_X: i32 = 245;
pub const NAME_Y: i32 = 505;
/// Dialogue line under the top glow. Measured off the original window:
/// white ink at game (270, 553), cell padding (6, 7), pitch 28.
pub const DIALOG_TEXT_X: i32 = 264;
pub const DIALOG_TEXT_Y: i32 = 546;
pub const DIALOG_TEXT_RIGHT: i32 = 1080;
/// Full-width advance. The 40px cell's ink is only ~25px, and the original
/// steps 28px between characters.
pub const DIALOG_PITCH: i32 = 28;
/// Baseline-to-baseline for a wrapped line.
pub const DIALOG_LINE: i32 = 36;

/// Like [`render_scene`] but composites the supplied character sprites between
/// the background and the dialog box.
pub fn render_scene_ex(
    bg: &TgaImage,
    text: &str,
    font: &Font,
    w: u32,
    h: u32,
    sprites: &[SpriteDraw],
    // Optional `.AMP` colour curve applied to the scene art (background and
    // sprites) but *not* to the dialog box or the text.
    lut: Option<&[u8]>,
    // 「ウィンドウ一時消去」: hide the dialog box and its text.
    window: bool,
    // Filled message window (`sys_00001`) and its oval frame (`sys_00000`).
    plate: Option<&TgaImage>,
    frame: Option<&TgaImage>,
) -> Surface {
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

    // Standing character sprites (bottom-anchored), below the dialog box.
    for sp in sprites {
        s.blit_region_alpha(sp.img, 0, 0, sp.img.width, sp.img.height, sp.x, sp.y, sp.alpha);
    }

    // Colour filter (sepia / nega / ...) over the scene art.
    if let Some(lut) = lut {
        s.apply_lut(lut);
    }

    // Dialog box near the bottom. The lowest 44 px of the screen are left free
    // for the command button row, so the box is shorter than it used to be.
    let box_h = (h / 4).min(160).max(120);
    let box_y = (h - box_h - 44) as i32;
    // 「ウィンドウ一時消去」: the scene stays, the box and its text do not.
    if !window {
        return s;
    }
    // Game window: the feathered plate, then the oval, then the glyphs.
    // Without those assets, fall back to the flat plate the tools use.
    let (x0, x_max, mut y, y_limit) = if plate.is_some() {
        if let Some(pl) = plate {
            s.blit(pl, WINDOW_X, WINDOW_Y);
        }
        if let Some(fr) = frame {
            s.blit(fr, WINDOW_X, WINDOW_Y);
        }
        (DIALOG_TEXT_X, DIALOG_TEXT_RIGHT, DIALOG_TEXT_Y, DIALOG_TEXT_Y + DIALOG_LINE * 3)
    } else {
        s.fill_rect(30, box_y, w - 60, box_h, [10, 10, 24, 200]);
        s.fill_rect(30, box_y, w - 60, 2, [180, 200, 255, 220]);
        s.fill_rect(30, box_y + box_h as i32 - 2, w - 60, 2, [180, 200, 255, 220]);
        (50, (w as i32) - 60, box_y + 18, box_y + box_h as i32)
    };

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

    let full = font.cell() as i32;
    let step = if plate.is_some() { DIALOG_PITCH } else { full };
    let line_h = if plate.is_some() { DIALOG_LINE } else { full + 4 };
    for line in cleaned.lines() {
        let mut x = x0;
        let mut buf = String::new();
        for ch in line.chars() {
            let adv = if plate.is_some() && (ch as u32) < 128 { step / 2 } else { step };
            if x + adv > x_max {
                break;
            }
            buf.push(ch);
            x += adv;
        }
        // Use `draw` (not `draw_with_fallback`) so unmapped characters leave a
        // gap instead of a magenta placeholder box: the text area then shows
        // only the glyphs that actually exist in the atlas, which reads much
        // cleaner than rows of placeholder bars.
        if plate.is_some() {
            font.draw_pitch(&mut s, &buf, x0, y, DIALOG_PITCH);
        } else {
            font.draw(&mut s, &buf, x0, y);
        }
        y += line_h;
        if y > y_limit - line_h {
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

/// Like `render_scene`, but rasterizes the dialog text with a SYSTEM CJK font
/// (ffmpeg drawtext) instead of the game bitmap atlas.
///
/// Use this when the atlas glyph order is unknown: text stays fully readable
/// even though it is not drawn in the game's own face.
pub fn render_scene_sysfont(
    bg: &TgaImage,
    text: &str,
    w: u32,
    h: u32,
    fontfile: &str,
) -> Surface {
    let mut s = Surface::new(w, h, [0, 0, 0, 255]);
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
    let box_h = (h / 4).max(140);
    let box_y = (h - box_h - 24) as i32;
    s.fill_rect(30, box_y, w - 60, box_h, [10, 10, 24, 200]);
    s.fill_rect(30, box_y, w - 60, 2, [180, 200, 255, 220]);
    s.fill_rect(30, box_y + box_h as i32 - 2, w - 60, 2, [180, 200, 255, 220]);

    // strip <R..> region tags and turn literal backslash-n into real newlines
    let cleaned: String = text.chars().filter(|&c| c != '\r').collect();
    let cleaned = cleaned.replace("<R", "\u{1}").replace('>', "\u{1}");
    let cleaned: String = cleaned
        .split('\u{1}')
        .enumerate()
        .map(|(i, s)| if i % 2 == 0 { s } else { "" })
        .collect();
    let cleaned = cleaned.replace("\\n", "\n");
    let lines: Vec<String> = cleaned.lines().map(|l| l.to_string()).collect();

    let tw = w - 100;
    let th = box_h - 30;
    if let Some((rw, rh, rgba)) = text::block_rgba(&lines, tw, th, 26, fontfile) {
        s.blit_rgba(&rgba, rw, rh, 50, box_y + 16);
    }
    s
}