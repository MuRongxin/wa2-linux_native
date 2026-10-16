//! Text drawn from a real font file.
//!
//! Glyphs are rasterized by FreeType (see ft2) once per character and size
//! into a tile holding two coverage masks - the ink and the outline around it -
//! so a glyph can be stamped in any colour without touching the font again.
//! Tiles are drawn at the window's own pixel size, which is the whole point:
//! the 1280x720 layer is stretched to the window, but the letters on top of it
//! are not.
//!
//! `with_ui_font` hands out one font for the whole program, so screens that
//! only need a caption do not have to thread one through their callers.

use crate::ft2;
use crate::render::Surface;
use std::cell::{Cell, Ref, RefCell};
use std::collections::HashMap;

/// Default font for engine-side text.
///
/// Medium, not Regular: at the dialog size of 26px a Regular CJK stroke is
/// mostly half-covered anti-aliased pixels and reads grey, where the game's own
/// bitmap glyphs were solid.
pub const DEFAULT_FONT: &str = "/usr/share/fonts/opentype/noto/NotoSansCJK-Medium.ttc";
/// Default coverage gain, in percent. See VectorFont::set_ink_gain.
pub const DEFAULT_GAIN: u32 = 130;

/// One rasterized glyph, ready to be stamped.
struct Tile {
    w: u32,
    h: u32,
    /// w * h * 4 bytes: R is the ink coverage, G the outline coverage.
    rgba: Vec<u8>,
    /// Offset from the pen (the baseline origin) to the tile's top-left.
    dx: i32,
    dy: i32,
    /// How far the pen moves right after this glyph.
    advance: i32,
}

/// A font file rasterized at one pixel size, with a glyph cache.
///
/// Everything is reachable through a shared reference and the cache is keyed
/// by (character, size): one font serves the whole program, so the settings
/// page asking for 18px between two dialog draws at 26px does not throw the
/// 26px glyphs away.
pub struct VectorFont {
    ft: RefCell<ft2::Ft>,
    size: Cell<u32>,
    /// Outline radius in pixels. The original bitmap font carried a one-pixel
    /// black edge, and dialog text sits on CGs, so it still needs one.
    outline: Cell<u32>,
    /// Stroke thickening in pixels, applied by growing the coverage mask. A
    /// face this thin renders grey at 26px, where the original bitmap font was
    /// solid, and this works for any font the player picks.
    bold: Cell<u32>,
    /// Coverage gain in percent. Anti-aliasing leaves a thin CJK stroke mostly
    /// made of half-covered pixels, which reads grey on screen; pushing the
    /// coverage up firms the stroke without changing its shape.
    gain: Cell<u32>,
    ascent: Cell<i32>,
    cache: RefCell<HashMap<(char, u32), Tile>>,
}

impl VectorFont {
    /// Open path at size pixels. None when FreeType or the file is unavailable,
    /// in which case callers keep the bitmap font.
    pub fn open(path: &str, size: u32) -> Option<VectorFont> {
        let ft = ft2::Ft::open(path, size)?;
        let ascent = ft.ascent()?;
        Some(VectorFont::from_ft(ft, size, ascent))
    }

    fn from_ft(ft: ft2::Ft, size: u32, ascent: i32) -> VectorFont {
        VectorFont {
            size: Cell::new(size),
            outline: Cell::new(outline_for(size)),
            bold: Cell::new(0),
            gain: Cell::new(DEFAULT_GAIN),
            ft: RefCell::new(ft),
            ascent: Cell::new(ascent),
            cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn size(&self) -> u32 {
        self.size.get()
    }

    /// Push the glyph coverage up by the given percentage, clamped at full.
    /// Used to firm thin anti-aliased strokes; 100 leaves the coverage alone.
    pub fn set_ink_gain(&self, pct: u32) {
        let pct = pct.clamp(25, 400);
        if pct == self.gain.get() {
            return;
        }
        self.gain.set(pct);
        self.cache.borrow_mut().clear();
    }

    pub fn ink_gain(&self) -> u32 {
        self.gain.get()
    }

    /// How much the strokes are thickened, in pixels.
    pub fn bold(&self) -> u32 {
        self.bold.get()
    }

    /// Thicken the strokes by growing the coverage mask. Glyphs already cached
    /// at the old weight are dropped.
    pub fn set_bold(&self, px: u32) {
        if px == self.bold.get() {
            return;
        }
        self.bold.set(px);
        self.cache.borrow_mut().clear();
    }

    /// Re-rasterize at a new pixel size. Already-cached sizes survive.
    pub fn set_size(&self, size: u32) -> bool {
        if size == 0 {
            return false;
        }
        if size == self.size.get() {
            return true;
        }
        if self.ft.borrow_mut().set_size(size).is_none() {
            return false;
        }
        let ascent = match self.ft.borrow().ascent() {
            Some(a) => a,
            None => return false,
        };
        self.size.set(size);
        self.outline.set(outline_for(size));
        self.ascent.set(ascent);
        true
    }

    /// Pixels above the baseline: where to put the baseline of a line whose
    /// top edge is known.
    pub fn ascent(&self) -> i32 {
        self.ascent.get()
    }

    fn ink_extents(&self, text: &str) -> (i32, i32, i32) {
        let (mut top, mut bottom, mut width) = (0i32, 0i32, 0i32);
        for ch in text.chars() {
            let t = self.tile(ch);
            top = top.max(-t.dy);
            bottom = bottom.max(t.dy + t.h as i32);
            width += t.advance;
        }
        (top, bottom, width)
    }

    /// Width of text at the current size.
    pub fn measure(&self, text: &str) -> i32 {
        let mut x = 0;
        for ch in text.chars() {
            x += self.tile(ch).advance;
        }
        x
    }

    /// Advance width of one character. Layout uses it to decide where the next
    /// pen position is before committing to drawing.
    pub fn measure_char(&self, ch: char) -> i32 {
        self.tile(ch).advance
    }

    /// Draw one character at the pen position, white with a black outline.
    pub fn draw_char(&self, surf: &mut Surface, ch: char, pen: i32, baseline: i32) {
        let t = self.tile(ch);
        stamp(surf, &t, pen, baseline, [255, 255, 255], [0, 0, 0]);
    }

    /// Draw text in white with a black outline, starting at (x, baseline).
    /// Returns the pen position after the last glyph.
    pub fn draw(&self, surf: &mut Surface, text: &str, x: i32, baseline: i32) -> i32 {
        self.draw_limited(surf, text, x, baseline, [255, 255, 255], [0, 0, 0], usize::MAX)
    }

    /// Draw at most limit characters - the typewriter reveals a line by
    /// redrawing a longer prefix of it.
    pub fn draw_limited(
        &self,
        surf: &mut Surface,
        text: &str,
        x: i32,
        baseline: i32,
        ink: [u8; 3],
        outline: [u8; 3],
        limit: usize,
    ) -> i32 {
        let mut pen = x;
        for (n, ch) in text.chars().enumerate() {
            if n >= limit {
                break;
            }
            let advance = {
                let t = self.tile(ch);
                stamp(surf, &t, pen, baseline, ink, outline);
                t.advance
            };
            pen += advance;
        }
        pen
    }


    /// Draw text in one flat colour with no outline, onto a surface that is
    /// already opaque. Menu and page labels sit on their own plate, where the
    /// original drew them solid.
    pub fn draw_flat(&self, surf: &mut Surface, text: &str, x: i32, baseline: i32, ink: [u8; 3]) -> i32 {
        let mut pen = x;
        for ch in text.chars() {
            let advance = {
                let t = self.tile(ch);
                stamp_ink(surf, &t, pen, baseline, ink);
                t.advance
            };
            pen += advance;
        }
        pen
    }

    /// One line rasterized into its own transparent RGBA buffer, sized to the
    /// text. The (w, h, rgba) shape is what the UI code already positions and
    /// tints by.
    pub fn line_rgba(&self, text: &str, size: u32) -> Option<(u32, u32, Vec<u8>)> {
        if text.is_empty() || !self.set_size(size) {
            return None;
        }
        let pad = 4i32;
        let (top, bottom, width) = self.ink_extents(text);
        let w = (width + pad * 2).max(1) as u32;
        let h = (top + bottom + pad * 2).max(1) as u32;
        let mut buf = vec![0u8; (w * h * 4) as usize];
        self.stamp_buf(&mut buf, w, h, text, pad, pad + top, [255, 255, 255], false);
        Some((w, h, buf))
    }

    /// Several lines stacked into one w x h buffer, the way the save/load
    /// captions and the backlog want them.
    pub fn block_rgba(
        &self,
        lines: &[String],
        w: u32,
        h: u32,
        size: u32,
    ) -> Option<(u32, u32, Vec<u8>)> {
        if w == 0 || h == 0 || !self.set_size(size) {
            return None;
        }
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let line_h = size as i32 + 8;
        let pad = 4i32;
        for (i, line) in lines.iter().enumerate() {
            let y = i as i32 * line_h;
            if y + line_h > h as i32 {
                break;
            }
            let (top, bottom, _) = self.ink_extents(line);
            let base = y + (line_h - (top + bottom)) / 2 + top;
            self.stamp_buf(&mut buf, w, h, line, pad, base, [255, 255, 255], false);
        }
        Some((w, h, buf))
    }

    /// Every label painted onto one transparent canvas. The settings page
    /// caches a canvas per state, so the labels have to come back as a buffer.
    pub fn labels_rgba(&self, w: u32, h: u32, labels: &[Label]) -> Option<Vec<u8>> {
        if w == 0 || h == 0 {
            return None;
        }
        let mut buf = vec![0u8; (w * h * 4) as usize];
        for l in labels {
            if l.text.is_empty() || !self.set_size(l.size) {
                continue;
            }
            let x = if l.center {
                l.x + (l.w - self.measure(&l.text)) / 2
            } else {
                l.x
            };
            let (top, bottom, _) = self.ink_extents(&l.text);
            let y = l.y + (l.h - (top + bottom)) / 2;
            self.stamp_buf(&mut buf, w, h, &l.text, x, y + top, l.rgb, false);
        }
        Some(buf)
    }

    /// Stamp into a raw RGBA buffer. Unlike Surface::put, which forces the
    /// alpha to 255, this keeps the glyph's coverage so the buffer can be
    /// blitted - and re-tinted - later without losing its antialiasing.
    fn stamp_buf(
        &self,
        buf: &mut [u8],
        bw: u32,
        bh: u32,
        text: &str,
        x: i32,
        baseline: i32,
        ink: [u8; 3],
        outline: bool,
    ) {
        let mut pen = x;
        for ch in text.chars() {
            let advance = {
                let t = self.tile(ch);
                stamp_raw(buf, bw, bh, &t, pen, baseline, ink, outline);
                t.advance
            };
            pen += advance;
        }
    }

    fn tile(&self, ch: char) -> Ref<'_, Tile> {
        let key = (ch, self.size.get());
        if !self.cache.borrow().contains_key(&key) {
            let t = self.build_tile(ch);
            self.cache.borrow_mut().insert(key, t);
        }
        Ref::map(self.cache.borrow(), |m| match m.get(&key) {
            Some(t) => t,
            None => unreachable!(),
        })
    }

    fn build_tile(&self, ch: char) -> Tile {
        let size = self.size.get();
        let g = match self.ft.borrow_mut().glyph(ch) {
            Some(g) => g,
            None => {
                // No glyph in the face: leave a gap the width of the character.
                let advance = if ch.is_ascii() {
                    (size / 2).max(1) as i32
                } else {
                    size.max(1) as i32
                };
                return Tile { w: 0, h: 0, rgba: Vec::new(), dx: 0, dy: 0, advance };
            }
        };
        let pad = self.outline.get() as usize;
        let grow = self.bold.get() as usize;
        // The margin has to hold both the thickened stroke and the outline
        // drawn around it, or the tile clips what it just grew.
        let m = pad + grow;
        let (gw, gh) = (g.w as usize, g.h as usize);
        let (tw, th) = (gw + m * 2, gh + m * 2);
        let mut ink = vec![0u8; tw * th];
        for y in 0..gh {
            for x in 0..gw {
                ink[(y + m) * tw + (x + m)] = g.mask[y * gw + x];
            }
        }
        // Thickening first means the outline follows the heavier stroke
        // instead of the thin original.
        let ink = if grow > 0 { dilate(&ink, tw, th, grow) } else { ink };
        let mut rgba = vec![0u8; tw * th * 4];
        if pad > 0 {
            let edge = dilate(&ink, tw, th, pad);
            for i in 0..(tw * th) {
                if ink[i] == 0 {
                    rgba[i * 4 + 1] = edge[i];
                }
            }
        }
        let gain = self.gain.get();
        for i in 0..(tw * th) {
            rgba[i * 4] = ((ink[i] as u32 * gain) / 100).min(255) as u8;
        }
        Tile {
            w: tw as u32,
            h: th as u32,
            rgba,
            dx: g.left - m as i32,
            dy: -(g.top + m as i32),
            advance: g.advance,
        }
    }
}

/// Maximum filter over a round structuring element: every pixel takes the
/// strongest coverage within radius r, which is how the strokes get thicker and
/// how the outline is grown around them.
fn dilate(mask: &[u8], w: usize, h: usize, r: usize) -> Vec<u8> {
    if r == 0 || w == 0 || h == 0 {
        return mask.to_vec();
    }
    let r2 = (r as i32) * (r as i32);
    let mut out = vec![0u8; w * h];
    for y in 0..h {
        for x in 0..w {
            let mut best = 0u8;
            for oy in -(r as i32)..=r as i32 {
                for ox in -(r as i32)..=r as i32 {
                    if ox * ox + oy * oy > r2 {
                        continue;
                    }
                    let (sx, sy) = (x as i32 + ox, y as i32 + oy);
                    if sx >= 0 && sy >= 0 && sx < w as i32 && sy < h as i32 {
                        best = best.max(mask[sy as usize * w + sx as usize]);
                    }
                }
            }
            out[y * w + x] = best;
        }
    }
    out
}

/// One label placed on a shared canvas.
pub struct Label {
    pub text: String,
    /// Box the label sits in, in the canvas's own pixels.
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub size: u32,
    /// Centre the text in the box instead of left-aligning it.
    pub center: bool,
    pub rgb: [u8; 3],
}

thread_local! {
    static UI: RefCell<Option<VectorFont>> = const { RefCell::new(None) };
}

/// The font file and the two knobs that change how heavy it reads.
pub struct FontChoice {
    pub path: String,
    /// Stroke thickening in pixels.
    pub bold: u32,
    /// Coverage gain in percent.
    pub gain: u32,
}

/// What the player asked for. WA2_FONT / WA2_FONT_BOLD / WA2_FONT_GAIN are the
/// stand-in for the font settings page, which will write the same three values.
pub fn font_choice() -> FontChoice {
    FontChoice {
        path: std::env::var("WA2_FONT").unwrap_or_else(|_| DEFAULT_FONT.to_string()),
        bold: std::env::var("WA2_FONT_BOLD")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(0),
        gain: std::env::var("WA2_FONT_GAIN")
            .ok()
            .and_then(|v| v.parse().ok())
            .unwrap_or(DEFAULT_GAIN),
    }
}

/// Point a font at a choice. Both knobs drop the glyph cache, so this only does
/// work when something actually changed.
pub fn apply_choice(vf: &VectorFont, choice: &FontChoice) {
    vf.set_bold(choice.bold);
    vf.set_ink_gain(choice.gain);
}

/// Run f with the engine's own font at size pixels.
///
/// Screens that only need a caption or two borrow the font here instead of
/// threading one through every UI function; the font is opened on first use
/// from WA2_FONT (or the default Noto CJK face) and is shared by every call.
/// None means FreeType or the font file is unavailable, and the caller has to
/// do without the text.
pub fn with_ui_font<R>(size: u32, f: impl FnOnce(&VectorFont) -> R) -> Option<R> {
    UI.with(|cell| {
        let mut slot = cell.borrow_mut();
        if slot.is_none() {
            let choice = font_choice();
            *slot = VectorFont::open(&choice.path, size.max(8));
        }
        let vf = slot.as_ref()?;
        if !vf.set_size(size) {
            return None;
        }
        apply_choice(vf, &font_choice());
        Some(f(vf))
    })
}

/// The bitmap font carried a one-pixel edge at its ~26px dialog size; keep that
/// weight as the text gets bigger, capped so a huge window does not go chunky.
fn outline_for(size: u32) -> u32 {
    (size / 16).clamp(1, 3)
}

/// Stamp one tile, ink only, onto an opaque surface.
fn stamp_ink(surf: &mut Surface, t: &Tile, pen: i32, baseline: i32, ink: [u8; 3]) {
    let ox = pen + t.dx;
    let oy = baseline + t.dy;
    for row in 0..t.h as i32 {
        for col in 0..t.w as i32 {
            let i = ((row as u32 * t.w + col as u32) * 4) as usize;
            let a = t.rgba[i];
            if a > 0 {
                surf.blend_pixel(ox + col, oy + row, [ink[0], ink[1], ink[2], a]);
            }
        }
    }
}

/// Stamp one tile into a raw RGBA buffer, keeping the coverage as alpha.
fn stamp_raw(
    buf: &mut [u8],
    bw: u32,
    bh: u32,
    t: &Tile,
    pen: i32,
    baseline: i32,
    ink: [u8; 3],
    outline: bool,
) {
    let ox = pen + t.dx;
    let oy = baseline + t.dy;
    for row in 0..t.h as i32 {
        for col in 0..t.w as i32 {
            let i = ((row as u32 * t.w + col as u32) * 4) as usize;
            let a = t.rgba[i];
            let e = t.rgba[i + 1];
            let (rgb, cov) = if a > 0 {
                (ink, a)
            } else if outline && e > 0 {
                ([0, 0, 0], e)
            } else {
                continue;
            };
            let (x, y) = (ox + col, oy + row);
            if x < 0 || y < 0 || x >= bw as i32 || y >= bh as i32 {
                continue;
            }
            let o = ((y as u32 * bw + x as u32) * 4) as usize;
            if cov <= buf[o + 3] {
                continue;
            }
            buf[o] = rgb[0];
            buf[o + 1] = rgb[1];
            buf[o + 2] = rgb[2];
            buf[o + 3] = cov;
        }
    }
}

/// Stamp one tile: the outline first so the ink sits inside it.
fn stamp(surf: &mut Surface, t: &Tile, pen: i32, baseline: i32, ink: [u8; 3], outline: [u8; 3]) {
    if t.w == 0 || t.h == 0 {
        return;
    }
    let ox = pen + t.dx;
    let oy = baseline + t.dy;
    for row in 0..t.h as i32 {
        for col in 0..t.w as i32 {
            let i = ((row as u32 * t.w + col as u32) * 4) as usize;
            let a = t.rgba[i];
            let e = t.rgba[i + 1];
            if a > 0 {
                surf.blend_pixel(ox + col, oy + row, [ink[0], ink[1], ink[2], a]);
            } else if e > 0 {
                surf.blend_pixel(ox + col, oy + row, [outline[0], outline[1], outline[2], e]);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{VectorFont, DEFAULT_FONT};
    use crate::render::Surface;

    /// Draw one line on a `bg` plate and report where the ink and the outline
    /// ended up. A white plate makes the outline countable, a black one the ink.
    fn probe(text: &str, size: u32, bg: [u8; 4]) -> Option<(Surface, i32, usize, usize)> {
        let mut f = VectorFont::open(DEFAULT_FONT, size)?;
        let mut s = Surface::new(240, 80, bg);
        let baseline = 50;
        let end = f.draw(&mut s, text, 4, baseline);
        let mut ink = 0;
        let mut edge = 0;
        for y in 0..s.height as i32 {
            for x in 0..s.width as i32 {
                let i = ((y as u32 * s.width + x as u32) * 4) as usize;
                if s.pixels[i] > 200 {
                    ink += 1;
                } else if s.pixels[i] < 60 {
                    edge += 1;
                }
            }
        }
        Some((s, end, ink, edge))
    }

    #[test]
    fn a_line_advances_one_em_per_ideograph() {
        // Skip on a machine without the font: the engine falls back to the
        // bitmap atlas there, it is not a failure of this code.
        let Some((_s, end, ink, edge)) = probe("一丁七", 26, [255, 255, 255, 255]) else { return };
        assert!(ink > 0, "nothing was drawn");
        assert!(edge > 0, "no outline around the ink");
        // Three full-width glyphs from x=4, within a pixel of rounding.
        assert!((end - (4 + 26 * 3)).abs() <= 3, "pen ended at {end}");
    }

    /// The UI screens cache a label as a (w, h, rgba) buffer and tint it later,
    /// so the coverage has to survive as alpha instead of being flattened onto
    /// the destination.
    #[test]
    fn a_label_buffer_keeps_its_coverage_as_alpha() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 22) else { return };
        let Some((w, h, buf)) = f.line_rgba("文本显示速度", 22) else { return };
        assert_eq!(buf.len(), w as usize * h as usize * 4);
        let mut solid = 0;
        let mut partial = 0;
        for px in buf.chunks_exact(4) {
            if px[3] == 0 {
                continue;
            }
            // Anything with coverage is white ink; the tint is applied later.
            assert!(px[0] > 200 && px[1] > 200 && px[2] > 200, "not white: ");
            if px[3] == 255 {
                solid += 1;
            } else {
                partial += 1;
            }
        }
        assert!(solid > 20, "no solid ink");
        assert!(partial > 20, "no antialiased edge");
    }

    /// The dialog is 26px CJK: a Regular face at that size is more than half
    /// half-covered anti-aliased pixels and reads grey, which is what made the
    /// first vector build look washed out. The default face and gain have to
    /// leave the strokes mostly solid.



    #[test]
    fn the_default_face_draws_solid_strokes_at_dialog_size() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        let mut surf = Surface::new(900, 60, [0, 0, 0, 255]);
        f.draw(&mut surf, "路邊的樹木葉子差不多已經落盡", 4, 40);
        let mut solid = 0usize;
        let mut any = 0usize;
        for px in surf.pixels.chunks_exact(4) {
            if px[0] > 128 {
                any += 1;
                if px[0] > 245 {
                    solid += 1;
                }
            }
        }
        assert!(any > 500, "nothing drawn, {any}");
        let frac = solid as f32 / any as f32;
        assert!(frac > 0.72, "strokes only {:.2} solid ({solid}/{any})", frac);
    }

    #[test]
    fn ink_stays_above_the_descent() {
        let Some((s, _end, _ink, _edge)) = probe("一丁七", 26, [0, 0, 0, 255]) else { return };
        let mut lowest = -1;
        for y in 0..s.height as i32 {
            for x in 0..s.width as i32 {
                let i = ((y as u32 * s.width + x as u32) * 4) as usize;
                if s.pixels[i] > 200 {
                    lowest = lowest.max(y);
                }
            }
        }
        // 一丁七 has no descender, so the ink must sit on the baseline (50)
        // rather than hanging below it.
        assert!(lowest > 0 && lowest <= 53, "lowest ink row {lowest}");
    }
}
