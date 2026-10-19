//! Text drawn from a real font file.
//!
//! Glyphs are rasterized by FreeType (see ft2) once per character and size
//! into a tile holding two coverage masks - the ink and the outline around it -
//! so a glyph can be stamped in any colour without touching the font again.
//! Tiles are drawn at the window's own pixel size, which is the whole point:
//! the 1280x720 layer is stretched to the window, but the letters on top of it
//! are not.
//!
//! How heavy a glyph reads is split into independent knobs, because stacking
//! them all on top of each other is what made the first vector build look like
//! stickers instead of type:
//!
//!   * the face itself (and which face of a .ttc) - shape and base weight;
//!   * `bold` - morphological thickening of the coverage mask;
//!   * `ink` - the coverage curve (gamma by default, legacy linear gain);
//!   * the outline radius, its alpha, and an optional soft shadow - how the
//!     glyph separates from whatever is behind it.
//!
//! `with_ui_font` hands out one font for the whole program, so screens that
//! only need a caption do not have to thread one through their callers.

use crate::ft2;
use crate::render::Surface;
use std::cell::{Cell, Ref, RefCell};
use std::collections::{HashMap, HashSet};
use std::os::raw::c_int;

/// Default font for engine-side text.
pub const DEFAULT_FONT: &str = "/usr/share/fonts/opentype/noto/NotoSansCJK-Medium.ttc";
/// Default face index inside the .ttc.
///
/// A .ttc is a collection: NotoSansCJK-Medium.ttc holds JP(0), KR(1), SC(2),
/// TC(3), HK(4). Face 0 draws Japanese glyph shapes - different shoulders on
/// 骨/直/着, different component proportions - so the Chinese text has to ask
/// for the SC face. `WA2_FONT` accepts `path#index` and `WA2_FONT_FACE`
/// overrides this.
pub const DEFAULT_FACE: i64 = 2;
/// Default ink gamma in permille. 1000 leaves the coverage untouched; below
/// 1000 lifts the anti-aliased edge without the hard saturation a linear
/// percentage gain causes. See `VectorFont::set_ink_gamma`.
pub const DEFAULT_GAMMA: u32 = 900;
/// Legacy linear coverage gain, in percent. Only used when `WA2_FONT_GAIN`
/// is set explicitly, so the two curves never mix.
pub const DEFAULT_GAIN: u32 = 130;
/// Default outline coverage for dialog text, 0..=255. The original bitmap
/// font carried a full-black edge; a full-alpha ring around every glyph reads
/// as a sticker, so the default keeps it translucent.
pub const DEFAULT_EDGE_ALPHA: u8 = 160;

/// One rasterized glyph, ready to be stamped.
struct Tile {
    w: u32,
    h: u32,
    /// w * h * 4 bytes: R is the ink coverage, G the outline coverage.
    rgba: Vec<u8>,
    /// Offset from the pen (the baseline origin) to the tile's top-left.
    dx: i32,
    dy: i32,
    /// How far the pen moves right after this glyph, in 26.6 fixed point.
    /// Layout accumulates in these units and rounds where a glyph is stamped.
    advance_266: i32,
}

/// How the rasterized coverage is remapped into the ink channel.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum InkMode {
    /// Legacy linear lift, in percent (100 = untouched).
    Gain(u32),
    /// Gamma curve, in permille (1000 = untouched).
    Gamma(u32),
}

/// FreeType hinting / render-target selection for FT_Load_Glyph.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum HintMode {
    /// FT_LOAD_TARGET_LIGHT: the default for CJK dialog text.
    Light,
    /// No explicit target: FreeType's default hinting.
    Normal,
    /// Force the auto-hinter (matters for Latin faces without hinting data).
    Auto,
    /// No hinting at all.
    None,
}

impl HintMode {
    /// FT_Load_Glyph flags. FT_LOAD_RENDER is always set: everything
    /// downstream reads the slot's bitmap.
    fn load_flags(self) -> c_int {
        const FT_LOAD_RENDER: c_int = 0x0000_0004;
        const FT_LOAD_NO_HINTING: c_int = 0x0000_0002;
        const FT_LOAD_FORCE_AUTOHINT: c_int = 0x0000_0020;
        const FT_LOAD_TARGET_LIGHT: c_int = 0x0001_0000;
        match self {
            HintMode::Light => FT_LOAD_RENDER | FT_LOAD_TARGET_LIGHT,
            HintMode::Normal => FT_LOAD_RENDER,
            HintMode::Auto => FT_LOAD_RENDER | FT_LOAD_FORCE_AUTOHINT | FT_LOAD_TARGET_LIGHT,
            HintMode::None => FT_LOAD_RENDER | FT_LOAD_NO_HINTING,
        }
    }

    fn parse(s: &str) -> Option<HintMode> {
        match s.trim().to_ascii_lowercase().as_str() {
            "light" => Some(HintMode::Light),
            "normal" => Some(HintMode::Normal),
            "auto" | "autohint" => Some(HintMode::Auto),
            "none" | "off" => Some(HintMode::None),
            _ => None,
        }
    }

    /// Name as it appears in the startup font line.
    pub fn name(self) -> &'static str {
        match self {
            HintMode::Light => "light",
            HintMode::Normal => "normal",
            HintMode::Auto => "auto",
            HintMode::None => "none",
        }
    }
}

/// Where a piece of text sits, which decides how it is separated from the
/// background. The dialog body sits on CGs and needs the most help; the name
/// sits on the dark rim of the window; UI labels sit on their own opaque
/// plates and want no edge at all.
#[derive(Clone, Copy, Debug)]
pub enum TextRole {
    /// Dialogue body: white ink, thin translucent edge, soft shadow.
    Body,
    /// Speaker name on the window rim: white ink, thin edge, no shadow.
    Name,
    /// Menu / save / timeline labels: flat ink, no edge, no shadow.
    Ui,
}

/// One resolved text style: colour plus how it separates from the background.
#[derive(Clone, Copy, Debug)]
pub struct TextStyle {
    pub color: [u8; 3],
    /// Outline coverage, 0..=255. 0 draws no outline.
    pub edge_alpha: u8,
    /// Soft shadow: (dx, dy, alpha). Alpha 0 disables it.
    pub shadow: (i32, i32, u8),
    /// Shadow colour.
    pub shadow_rgb: [u8; 3],
    /// Rule under the line. The dialogue body only - a menu label sits on its
    /// own plate and has nothing to separate from.
    pub underline: bool,
}

impl TextStyle {
    /// Dialogue body on a CG: white ink, a one-pixel translucent edge and a
    /// slight offset shadow, so thin strokes stay readable over bright art.
    pub fn body() -> TextStyle {
        TextStyle {
            color: [255, 255, 255],
            edge_alpha: DEFAULT_EDGE_ALPHA,
            shadow: (1, 1, 110),
            shadow_rgb: [0, 0, 0],
            underline: false,
        }
    }

    /// Speaker name on the window rim. The body is plain white with a soft
    /// shadow; the name is a touch warmer and carries a slightly firmer edge,
    /// so the two read as different levels of the same interface rather than
    /// one style stamped twice. The original drew the name in the same white,
    /// which left it competing with the line underneath it.
    pub fn name() -> TextStyle {
        TextStyle {
            color: [255, 246, 226],
            edge_alpha: 170,
            shadow: (0, 0, 0),
            shadow_rgb: [0, 0, 0],
            underline: false,
        }
    }

    /// UI labels on their own plates: flat ink, nothing around it.
    pub fn ui() -> TextStyle {
        TextStyle {
            color: [255, 255, 255],
            edge_alpha: 0,
            shadow: (0, 0, 0),
            shadow_rgb: [0, 0, 0],
            underline: false,
        }
    }

    /// The style a role resolves to.
    pub fn for_role(role: TextRole) -> TextStyle {
        match role {
            TextRole::Body => TextStyle::body(),
            TextRole::Name => TextStyle::name(),
            TextRole::Ui => TextStyle::ui(),
        }
    }
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
    /// True once WA2_FONT_EDGE pinned the radius, so a resize keeps it.
    edge_override: Cell<bool>,
    /// Stroke thickening in pixels, applied by growing the coverage mask.
    bold: Cell<u32>,
    /// Coverage curve applied to the ink channel.
    ink: Cell<InkMode>,
    /// Outline coverage of dialog text, 0..=255.
    edge_alpha: Cell<u8>,
    /// Hinting / render target FT_Load_Glyph is called with.
    hint: Cell<HintMode>,
    /// Vertical metrics in pixels at the current size.
    metrics: Cell<ft2::Metrics>,
    /// Characters the face has no glyph for, already reported. The warning is
    /// once per character per font, so a line redrawn every typewriter tick
    /// does not flood the log.
    missing: RefCell<HashSet<char>>,
    cache: RefCell<HashMap<(char, u32), Tile>>,
}

impl VectorFont {
    /// Open path at size pixels, on the default face. None when FreeType or
    /// the file is unavailable, in which case callers keep the bitmap font.
    pub fn open(path: &str, size: u32) -> Option<VectorFont> {
        VectorFont::open_face(path, size, DEFAULT_FACE)
    }

    /// Open a specific face of the file (see DEFAULT_FACE for the .ttc order).
    pub fn open_face(path: &str, size: u32, face: i64) -> Option<VectorFont> {
        let ft = ft2::Ft::open(path, size, face)?;
        let metrics = ft.metrics()?;
        Some(VectorFont::from_ft(ft, size, metrics))
    }

    fn from_ft(ft: ft2::Ft, size: u32, metrics: ft2::Metrics) -> VectorFont {
        VectorFont {
            size: Cell::new(size),
            outline: Cell::new(outline_for(size)),
            edge_override: Cell::new(false),
            bold: Cell::new(0),
            ink: Cell::new(InkMode::Gamma(DEFAULT_GAMMA)),
            edge_alpha: Cell::new(DEFAULT_EDGE_ALPHA),
            hint: Cell::new(HintMode::Light),
            metrics: Cell::new(metrics),
            missing: RefCell::new(HashSet::new()),
            ft: RefCell::new(ft),
            cache: RefCell::new(HashMap::new()),
        }
    }

    pub fn size(&self) -> u32 {
        self.size.get()
    }

    /// Remap the ink coverage through a gamma curve, in permille. 1000 leaves
    /// it untouched; 900 lifts the anti-aliased edge towards solid without the
    /// hard saturation a linear percentage gain produces. Glyphs already
    /// cached at the old curve are dropped.
    pub fn set_ink_gamma(&self, permille: u32) {
        let permille = permille.clamp(400, 2000);
        if self.ink.get() == InkMode::Gamma(permille) {
            return;
        }
        self.ink.set(InkMode::Gamma(permille));
        self.cache.borrow_mut().clear();
    }

    pub fn ink_gamma(&self) -> Option<u32> {
        match self.ink.get() {
            InkMode::Gamma(g) => Some(g),
            InkMode::Gain(_) => None,
        }
    }

    /// Legacy linear lift of the ink coverage, in percent; 100 leaves it
    /// alone. Superseded by the gamma curve - setting this switches the ink
    /// channel back to the linear mode.
    pub fn set_ink_gain(&self, pct: u32) {
        let pct = pct.clamp(25, 400);
        if self.ink.get() == InkMode::Gain(pct) {
            return;
        }
        self.ink.set(InkMode::Gain(pct));
        self.cache.borrow_mut().clear();
    }

    pub fn ink_gain(&self) -> Option<u32> {
        match self.ink.get() {
            InkMode::Gain(g) => Some(g),
            InkMode::Gamma(_) => None,
        }
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

    /// Outline coverage of dialog text, 0..=255.
    pub fn set_edge_alpha(&self, alpha: u8) {
        self.edge_alpha.set(alpha);
    }

    pub fn edge_alpha(&self) -> u8 {
        self.edge_alpha.get()
    }

    /// Override the outline radius in pixels. None goes back to the
    /// size-derived radius. Changing it invalidates the glyph cache.
    pub fn set_edge_px(&self, px: Option<u32>) {
        let want = px.unwrap_or_else(|| outline_for(self.size.get()));
        if want == self.outline.get() && self.edge_override.get() == px.is_some() {
            return;
        }
        self.edge_override.set(px.is_some());
        self.outline.set(want);
        self.cache.borrow_mut().clear();
    }

    pub fn edge_px(&self) -> u32 {
        self.outline.get()
    }

    /// Select the FreeType hinting / render target. Changing it invalidates
    /// the glyph cache.
    pub fn set_hint(&self, hint: HintMode) {
        if hint == self.hint.get() {
            return;
        }
        if self.ft.borrow_mut().set_load_flags(hint.load_flags()) {
            self.hint.set(hint);
            self.cache.borrow_mut().clear();
        }
    }

    pub fn hint(&self) -> HintMode {
        self.hint.get()
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
        let metrics = match self.ft.borrow().metrics() {
            Some(m) => m,
            None => return false,
        };
        self.size.set(size);
        self.metrics.set(metrics);
        // The outline is derived from the size unless it was pinned.
        if !self.edge_override.get() {
            self.outline.set(outline_for(size));
        }
        true
    }

    /// Vertical metrics in pixels at the current size.
    pub fn metrics(&self) -> ft2::Metrics {
        self.metrics.get()
    }

    /// Pixels above the baseline: where to put the baseline of a line whose
    /// top edge is known.
    pub fn ascent(&self) -> i32 {
        self.metrics.get().ascender
    }

    /// Pixels below the baseline (<= 0).
    pub fn descent(&self) -> i32 {
        self.metrics.get().descender
    }

    /// Suggested line height in pixels.
    pub fn line_height(&self) -> i32 {
        self.metrics.get().height
    }

    /// Baseline for a line box whose top edge and height are known: the
    /// font's own vertical extent is centred in the box, so the same code
    /// lands correctly on any face, size or window scale.
    pub fn baseline_in_box(&self, top: i32, box_h: i32) -> i32 {
        let m = self.metrics.get();
        top + (box_h - m.ascender - m.descender) / 2 + m.ascender
    }

    /// Family name of the open face, e.g. "Noto Sans CJK SC".
    pub fn family(&self) -> String {
        self.ft.borrow().family().unwrap_or_else(|| "?".to_string())
    }

    /// One line describing the open font: family, face index and the knobs.
    /// Printed at startup so the log says which face of the .ttc is in use,
    /// not just which file was opened.
    pub fn describe(&self) -> String {
        let ft = self.ft.borrow();
        let family = ft.family().unwrap_or_else(|| "?".to_string());
        let style = ft.style().unwrap_or_default();
        let ink = match self.ink.get() {
            InkMode::Gamma(g) => format!("gamma={:.2}", g as f32 / 1000.0),
            InkMode::Gain(g) => format!("gain={}%", g),
        };
        format!(
            "{} {} (face {} of {}) {}px {} bold={} edge={}px@{} hint={}",
            family,
            style,
            ft.face_index(),
            ft.num_faces(),
            self.size.get(),
            ink,
            self.bold.get(),
            self.outline.get(),
            self.edge_alpha.get(),
            self.hint.get().name(),
        )
    }

    fn ink_extents(&self, text: &str) -> (i32, i32, i32) {
        let (mut top, mut bottom, mut width) = (0i32, 0i32, 0i32);
        let mut prev: Option<char> = None;
        for ch in text.chars() {
            let t = self.tile(ch);
            top = top.max(-t.dy);
            bottom = bottom.max(t.dy + t.h as i32);
            width += self.advance_266(prev, ch);
            prev = Some(ch);
        }
        (top, bottom, round_266(width))
    }

    /// How far this text's ink reaches above and below the baseline, in
    /// pixels. Layout uses it to place a caption inside the box the artwork
    /// gave it, and to check afterwards that nothing crossed a rule.
    pub fn ink_bounds(&self, text: &str) -> (i32, i32) {
        let (top, bottom, _) = self.ink_extents(text);
        (top, bottom)
    }

    /// Width of text at the current size, kerning included.
    pub fn measure(&self, text: &str) -> i32 {
        self.measure_tracked(text, 0)
    }

    /// Width of text with `tracking` pixels added between glyphs. Tracking is
    /// a layout property, so it counts towards the width used for centring -
    /// that is the whole point of doing it here instead of typing spaces into
    /// the string.
    pub fn measure_tracked(&self, text: &str, tracking: i32) -> i32 {
        let mut x = 0i32;
        let mut prev: Option<char> = None;
        let mut n = 0usize;
        for ch in text.chars() {
            x += self.advance_after(prev, ch);
            if n > 0 {
                x += tracking;
            }
            prev = Some(ch);
            n += 1;
        }
        x
    }

    /// Advance width of one character. Layout uses it to decide where the next
    /// pen position is before committing to drawing.
    pub fn measure_char(&self, ch: char) -> i32 {
        round_266(self.advance_266(None, ch))
    }

    /// How far the pen moves after drawing `ch` when it follows `prev`:
    /// the glyph's own advance plus any kerning between the two, in pixels.
    /// The CJK width policy is applied, so this agrees with what drawing does.
    pub fn advance_after(&self, prev: Option<char>, ch: char) -> i32 {
        round_266(self.advance_266(prev, ch))
    }

    /// The same advance in 26.6 fixed point. The dialog layout keeps its pen
    /// in these units and rounds only when a glyph is stamped, so a long mixed
    /// CJK/Latin line does not accumulate a pixel of drift per character.
    pub fn advance_266(&self, prev: Option<char>, ch: char) -> i32 {
        let t = self.tile(ch);
        let base = if full_width_punct(ch) {
            // The original drew every CJK punctuation cell at the same pitch;
            // see the width policy below.
            (self.size.get() as i32) << 6
        } else {
            t.advance_266
        };
        base + self.kerning_266(prev, ch)
    }

    /// Where the glyph's ink sits relative to the pen: (left offset, width).
    /// Used for the last-character wrap check, which has to look at ink
    /// rather than at the advance the next glyph would start from.
    pub fn char_ink(&self, ch: char) -> (i32, i32) {
        let t = self.tile(ch);
        (t.dx, t.w as i32)
    }

    /// Draw one character at the pen position with a style.
    pub fn draw_char_styled(
        &self,
        surf: &mut Surface,
        ch: char,
        pen: i32,
        baseline: i32,
        style: &TextStyle,
    ) {
        let t = self.tile(ch);
        stamp_styled(surf, &t, pen, baseline, style);
    }

    /// Draw one character at the pen position, white with a black outline.
    pub fn draw_char(&self, surf: &mut Surface, ch: char, pen: i32, baseline: i32) {
        self.draw_char_styled(surf, ch, pen, baseline, &TextStyle::body());
    }

    /// Draw text in white with a black outline, starting at (x, baseline).
    /// Returns the pen position after the last glyph.
    pub fn draw(&self, surf: &mut Surface, text: &str, x: i32, baseline: i32) -> i32 {
        self.draw_styled(surf, text, x, baseline, &TextStyle::body())
    }

    /// Draw text with a style, starting at (x, baseline). Returns the pen
    /// position after the last glyph.
    pub fn draw_styled(
        &self,
        surf: &mut Surface,
        text: &str,
        x: i32,
        baseline: i32,
        style: &TextStyle,
    ) -> i32 {
        // The pen lives in 26.6 and is rounded only where a glyph is
        // stamped, so Latin advances do not lose their sub-pixel part one
        // character at a time.
        let mut pen = (x as i32) << 6;
        let mut prev: Option<char> = None;
        for ch in text.chars() {
            let t = self.tile(ch);
            stamp_styled(surf, &t, round_266(pen), baseline, style);
            pen += self.advance_266(prev, ch);
            prev = Some(ch);
        }
        round_266(pen)
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
        limit: usize,
    ) -> i32 {
        let style = TextStyle { color: ink, ..TextStyle::body() };
        let mut pen = (x as i32) << 6;
        let mut prev: Option<char> = None;
        for (n, ch) in text.chars().enumerate() {
            if n >= limit {
                break;
            }
            let t = self.tile(ch);
            stamp_styled(surf, &t, round_266(pen), baseline, &style);
            pen += self.advance_266(prev, ch);
            prev = Some(ch);
        }
        round_266(pen)
    }

    /// Draw text in one flat colour with no outline, onto a surface that is
    /// already opaque. Menu and page labels sit on their own plate, where the
    /// original drew them solid.
    pub fn draw_flat(&self, surf: &mut Surface, text: &str, x: i32, baseline: i32, ink: [u8; 3]) -> i32 {
        let mut pen = (x as i32) << 6;
        let mut prev: Option<char> = None;
        for ch in text.chars() {
            let t = self.tile(ch);
            stamp_ink(surf, &t, round_266(pen), baseline, ink);
            pen += self.advance_266(prev, ch);
            prev = Some(ch);
        }
        round_266(pen)
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
        self.stamp_buf(&mut buf, w, h, text, pad, pad + top, &TextStyle::for_role(TextRole::Ui), 0);
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
            self.stamp_buf(&mut buf, w, h, line, pad, base, &TextStyle::for_role(TextRole::Ui), 0);
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
                l.x + (l.w - self.measure_tracked(&l.text, l.tracking)) / 2
            } else {
                l.x
            };
            let (ink_top, ink_bottom, _) = self.ink_extents(&l.text);
            let m = self.metrics.get();
            let line = m.ascender - m.descender;
            // Explicit baseline rather than centring the ink: the box is what
            // the artwork laid out, so the font's own line sits centred in it
            // and every caption on the page shares one rhythm. A box shorter
            // than the line would push the baseline past its own bottom, so
            // that case falls back to centring the ink.
            let baseline = if line <= l.h {
                l.y + (l.h - line) / 2 + m.ascender
            } else {
                l.y + (l.h - (ink_top + ink_bottom)) / 2 + ink_top
            };
            // UI labels carry their own colour and sit on an opaque plate, so
            // they take the flat style: no edge, no shadow.
            let style = TextStyle { color: l.rgb, ..TextStyle::for_role(TextRole::Ui) };
            self.stamp_buf(&mut buf, w, h, &l.text, x, baseline, &style, l.tracking);
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
        style: &TextStyle,
        tracking: i32,
    ) {
        let mut pen = (x as i32) << 6;
        let track_266 = (tracking as i32) << 6;
        let mut prev: Option<char> = None;
        let mut first = true;
        for ch in text.chars() {
            // The tracking goes into the gap before each glyph after the
            // first one. Adding it after the advance instead leaves the first
            // gap out, while `measure_tracked` still counts it - so the line
            // renders narrower than it measures and every later gap is one
            // step too far right.
            if !first {
                pen += track_266;
            }
            let t = self.tile(ch);
            stamp_raw(buf, bw, bh, &t, round_266(pen), baseline, style);
            pen += self.advance_266(prev, ch);
            first = false;
            prev = Some(ch);
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
        // The face borrow has to end before missing_tile, which asks the face
        // its family name for the log line.
        let g = { self.ft.borrow_mut().glyph(ch) };
        let Some(g) = g else { return self.missing_tile(ch, size) };
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
        if grow > 0 {
            ink = dilate(&ink, tw, th, grow);
        }
        // The coverage curve is applied before the outline is grown, so the
        // ring follows the stroke as it will actually be seen.
        let lut = self.ink_lut();
        for v in ink.iter_mut() {
            *v = lut[*v as usize];
        }
        let mut rgba = vec![0u8; tw * th * 4];
        if pad > 0 {
            let edge = dilate(&ink, tw, th, pad);
            for i in 0..(tw * th) {
                if ink[i] == 0 {
                    rgba[i * 4 + 1] = edge[i];
                }
            }
        }
        for i in 0..(tw * th) {
            rgba[i * 4] = ink[i];
        }
        Tile {
            w: tw as u32,
            h: th as u32,
            rgba,
            dx: g.left - m as i32,
            dy: -(g.top + m as i32),
            advance_266: g.advance_266,
        }
    }

    /// The face has no glyph for `ch`. The text keeps a gap of the width the
    /// character would have had - never a ragged hole and never a placeholder
    /// box, which read worse than the gap - and the first time each character
    /// goes missing it is named in the log, so a silent hole is a bug report
    /// instead of a mystery.
    fn missing_tile(&self, ch: char, size: u32) -> Tile {
        if self.missing.borrow_mut().insert(ch) {
            let shown = if ch.is_control() || ch.is_whitespace() {
                format!("U+{:04X}", ch as u32)
            } else {
                format!("U+{:04X} '{}'", ch as u32, ch.escape_default())
            };
            eprintln!(
                "font: {} has no glyph for {} - leaving a gap",
                self.family(),
                shown
            );
        }
        let advance = if ch.is_ascii() {
            (size / 2).max(1) as i32
        } else {
            size.max(1) as i32
        };
        Tile {
            w: 0,
            h: 0,
            rgba: Vec::new(),
            dx: 0,
            dy: 0,
            advance_266: (advance as i32) << 6,
        }
    }

    /// 256-entry lookup table for the ink coverage curve.
    fn ink_lut(&self) -> [u8; 256] {
        let mut lut = [0u8; 256];
        match self.ink.get() {
            InkMode::Gain(pct) => {
                for (i, v) in lut.iter_mut().enumerate() {
                    *v = ((i as u32 * pct) / 100).min(255) as u8;
                }
            }
            InkMode::Gamma(permille) => {
                let g = permille as f32 / 1000.0;
                for (i, v) in lut.iter_mut().enumerate() {
                    let a = i as f32 / 255.0;
                    *v = (255.0 * a.powf(g)).round().clamp(0.0, 255.0) as u8;
                }
            }
        }
        lut
    }

    /// Kerning between two characters, in 26.6 fixed point. Only the scripts
    /// that carry a kern table get it: CJK advances are already full-width, and
    /// asking FreeType for every pair would slow the dialog loop for nothing.
    fn kerning_266(&self, prev: Option<char>, ch: char) -> i32 {
        let Some(p) = prev else { return 0 };
        if !(p.is_ascii() && ch.is_ascii()) {
            return 0;
        }
        self.ft.borrow_mut().kerning_266(p, ch)
    }
}

/// Round a 26.6 fixed-point value to whole pixels.
fn round_266(v: i32) -> i32 {
    (v + 32) >> 6
}

/// Characters that must advance a full em in this engine's text, whatever the
/// font file says about them.
///
/// The original bitmap font stepped every one of these at the same 28px pitch,
/// so 「」，。！？…――""''※ all occupied a full-width cell and a line of dialogue
/// had an even rhythm. Noto Sans CJK agrees for almost all of them, but it
/// gives U+2014 (em dash) 23px instead of 26, which shows up as a visible gap
/// inside a ―― run, and a player-supplied Latin face may disagree about more of
/// them. The override keeps the treatment uniform and independent of the face.
///
/// The set is the CP932 double-byte punctuation block plus the fullwidth forms
/// block, minus the characters that also belong to Latin text (acute,
/// diaeresis, plus-minus, times, divide, degree, section, pilcrow, permille):
/// those keep their proportional advance so "10±5" still typesets as Latin.
///
/// U+FF01..U+FF60 are the fullwidth ASCII/bracket forms and U+FFE0..U+FFE6 the
/// fullwidth currency and sign forms; they are one em in a CJK face already, so
/// the override is dead weight for Noto but keeps a player-supplied face from
/// narrowing them. The *halfwidth* forms (U+FF61..U+FF9F, halfwidth katakana
/// and CJK punctuation) are deliberately not in the set - they are narrow by
/// definition and are the one place a Latin-ish face can be right.
fn full_width_punct(ch: char) -> bool {
    matches!(ch,
        // CJK brackets, punctuation, iteratio, space, prolonged sound mark
        '\u{3000}'..='\u{3003}' | '\u{3005}'..='\u{3007}' | '\u{3008}'..='\u{3015}'
        | '\u{30FB}' | '\u{30FC}' | '\u{309B}'..='\u{309E}'
        // fullwidth ASCII variants, brackets, currency and sign forms
        | '\u{FF01}'..='\u{FF60}' | '\u{FFE0}'..='\u{FFE6}'
        // dashes, quotes, dagger, bullet, ellipsis, reference mark
        | '\u{2010}'..='\u{2015}' | '\u{2018}'..='\u{201D}' | '\u{2020}'..='\u{2026}'
        | '\u{203B}'
        // arrows, geometric shapes, stars, music, temperature
        | '\u{2190}'..='\u{2193}' | '\u{2225}' | '\u{25A0}'..='\u{25FF}'
        | '\u{2605}' | '\u{2606}' | '\u{266A}' | '\u{266D}' | '\u{266F}' | '\u{2103}')
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
    /// Extra pixels between glyphs, in this canvas's own pixels (the caller
    /// scales it from the layout). This is letter tracking done properly:
    /// putting real spaces into the string would change its width, its
    /// centring and its wrap.
    pub tracking: i32,
}

impl Label {
    /// A label with no tracking, for callers that do not letter-space.
    pub fn plain(text: impl Into<String>, x: i32, y: i32, w: i32, h: i32, size: u32, rgb: [u8; 3]) -> Label {
        Label {
            text: text.into(),
            x,
            y,
            w,
            h,
            size,
            center: true,
            rgb,
            tracking: 0,
        }
    }
}

/// One cached font instance. `gen` is the generation of the role ids it was
/// opened under: bumping an id drops the instance, and with it the glyph
/// cache, so not one glyph of the old font can survive into the new one.
struct RoleFont {
    gen: u64,
    vf: VectorFont,
}

thread_local! {
    /// One font per role, each with its own FreeType face and its own glyph
    /// cache. Sharing one instance was what locked the menu, the history and
    /// the dialogue to the same file.
    static ROLE_FONTS: RefCell<[Option<RoleFont>; 3]> = const { RefCell::new([None, None, None]) };
}

/// What the player picked for each role, and how often it changed. The ids
/// live outside the thread-local instances so the settings screen can drop
/// every cached font from wherever it is applied.
static ROLE_IDS: std::sync::Mutex<[i32; 3]> = std::sync::Mutex::new([0, 0, 0]);
static ROLE_GEN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(1);

/// The font file and the knobs that change how heavy it reads.
pub struct FontChoice {
    pub path: String,
    /// Face index inside the file (which language of a .ttc).
    pub face: i64,
    /// Stroke thickening in pixels.
    pub bold: u32,
    /// Ink gamma in permille; None means the legacy linear gain is in use.
    pub gamma: Option<u32>,
    /// Legacy linear coverage gain in percent.
    pub gain: u32,
    /// Outline radius override in pixels; None derives it from the size.
    pub edge_px: Option<u32>,
    /// Outline coverage, 0..=255.
    pub edge_alpha: u8,
    /// FreeType hinting / render target.
    pub hint: HintMode,
}

/// Split a `path#index` font spec. A bare path keeps face index None.
fn split_font_spec(spec: &str) -> (String, Option<i64>) {
    match spec.rsplit_once('#') {
        Some((path, idx)) => match idx.trim().parse::<i64>() {
            Ok(n) => (path.to_string(), Some(n)),
            Err(_) => (spec.to_string(), None),
        },
        None => (spec.to_string(), None),
    }
}

// ---------------------------------------------------------------------------
// dialogue text style
// ---------------------------------------------------------------------------

/// How the dialogue body is drawn once the player has moved it off the
/// defaults. It lives with the font roles because a change has to reach the
/// same place: the next line drawn, not the one already on screen.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub struct DialogStyle {
    /// Body size in 720p pixels; the window scales it from there.
    pub size: u32,
    /// Outline radius in pixels. 0 keeps the one the size implies.
    pub edge: u32,
    /// Index into DIALOG_COLORS.
    pub color: u32,
    pub underline: bool,
}

pub const DIALOG_SIZE_MIN: u32 = 20;
pub const DIALOG_SIZE_MAX: u32 = 40;
pub const DIALOG_EDGE_MAX: u32 = 3;

/// The body colours the player can pick: white, then warm and cool tints that
/// still read on a CG.
pub const DIALOG_COLORS: [[u8; 3]; 8] = [
    [255, 255, 255],
    [255, 246, 226],
    [255, 231, 205],
    [226, 255, 235],
    [205, 233, 255],
    [233, 214, 255],
    [255, 214, 231],
    [206, 206, 206],
];

pub const DIALOG_COLOR_NAMES: [&str; 8] = ["白", "暖白", "米色", "淡绿", "淡蓝", "淡紫", "淡粉", "灰"];

impl DialogStyle {
    /// What the game ships with: 26px, the edge the size implies, white, no
    /// rule under the line.
    pub const fn default_style() -> DialogStyle {
        DialogStyle { size: 26, edge: 0, color: 0, underline: false }
    }

    /// Clamp a value that came out of the config file.
    pub fn clamp(self) -> DialogStyle {
        DialogStyle {
            size: self.size.clamp(DIALOG_SIZE_MIN, DIALOG_SIZE_MAX),
            edge: self.edge.min(DIALOG_EDGE_MAX),
            color: self.color.min(DIALOG_COLORS.len() as u32 - 1),
            underline: self.underline,
        }
    }
}

static DIALOG: std::sync::Mutex<DialogStyle> = std::sync::Mutex::new(DialogStyle::default_style());

pub fn set_dialog_style(s: DialogStyle) -> bool {
    let s = s.clamp();
    let mut cur = DIALOG.lock().unwrap_or_else(|e| e.into_inner());
    if *cur == s {
        return false;
    }
    *cur = s;
    true
}

pub fn dialog_style() -> DialogStyle {
    *DIALOG.lock().unwrap_or_else(|e| e.into_inner())
}

/// Body size in 720p pixels, for the caller that scales it to the window.
pub fn dialog_font_size() -> u32 {
    dialog_style().size
}

/// The body style the dialogue is drawn with: the player's colour and rule on
/// top of the shadow the artwork was tuned for.
pub fn dialog_body_style() -> TextStyle {
    let s = dialog_style();
    TextStyle {
        color: DIALOG_COLORS[(s.color as usize).min(DIALOG_COLORS.len() - 1)],
        underline: s.underline,
        ..TextStyle::body()
    }
}

/// The outline radius the player asked for, or None to keep the one the size
/// implies - which is what the game shipped with.
pub fn dialog_edge_px(size: u32) -> Option<u32> {
    let e = dialog_style().edge;
    if e == 0 {
        None
    } else {
        Some(e.min(size / 4).max(1))
    }
}

// ---------------------------------------------------------------------------
// font roles
// ---------------------------------------------------------------------------

/// The three fonts the player sets independently.
///
/// Everything used to borrow one font, so the title screen, the text history
/// and the dialogue had to share a file and a glyph cache. A role owns its own
/// instance now: changing one role drops only that role's cache.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum FontRole {
    /// Title screen, option screens, save/load, timeline, music mode.
    Home,
    /// The text history / backlog screen.
    History,
    /// Dialogue, speaker names and narration.
    Game,
}

/// Display order in the settings page: the text the player reads most comes
/// first. The slot each role owns is `FontRole::index`, not this order.
pub const FONT_ROLES: [FontRole; 3] = [FontRole::Game, FontRole::History, FontRole::Home];

impl FontRole {
    pub fn index(self) -> usize {
        match self {
            FontRole::Home => 0,
            FontRole::History => 1,
            FontRole::Game => 2,
        }
    }

    /// Short tag for the log line.
    pub fn tag(self) -> &'static str {
        match self {
            FontRole::Home => "home",
            FontRole::History => "history",
            FontRole::Game => "game",
        }
    }

    /// Key it is stored under in `wa2.ini`.
    pub fn ini_key(self) -> &'static str {
        match self {
            FontRole::Home => "font_home",
            FontRole::History => "font_history",
            FontRole::Game => "font_game",
        }
    }

    /// Row label on the option page.
    pub fn label(self) -> &'static str {
        match self {
            FontRole::Home => "UI字体",
            FontRole::History => "已读文本字体",
            FontRole::Game => "对话文本字体",
        }
    }

    /// Environment variable that overrides this role alone.
    pub fn env_var(self) -> &'static str {
        match self {
            FontRole::Home => "WA2_FONT_HOME",
            FontRole::History => "WA2_FONT_HISTORY",
            FontRole::Game => "WA2_FONT_GAME",
        }
    }
}

// ---------------------------------------------------------------------------
// font catalogue
// ---------------------------------------------------------------------------

/// One entry of the font list the settings page offers.
#[derive(Clone, Debug)]
pub struct FontProfile {
    /// Stable id, and the only thing wa2.ini stores: a hash of the font file's
    /// own name and face index, so the same value names the same font on
    /// another machine and never depends on where it sorted here. 0 is the
    /// default entry.
    pub id: i32,
    /// Name shown in the settings page: the family, plus the style when the
    /// face carries an interesting one.
    pub name: String,
    /// Resolved file. None only for the default entry when this machine has
    /// nothing that can draw the script.
    pub path: Option<String>,
    pub face: i64,
    /// Family FreeType reports for the resolved face.
    pub family: String,
}

/// Face index of the Simplified Chinese face inside a Noto CJK collection
/// (JP=0, KR=1, SC=2, TC=3, HK=4). Noto ships one file per script region, so
/// the index is fixed for every weight.
const SC_FACE: i64 = 2;

/// A fonts/ folder shipped with the program, then one next to the working
/// directory. A packaged build drops its CJK font here so the default never
/// depends on what the distro happened to install.
fn bundled_dirs() -> Vec<String> {
    let mut v = Vec::new();
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            v.push(dir.join("fonts").to_string_lossy().into_owned());
        }
    }
    v.push("fonts".to_string());
    v
}

/// The default entry: the font shipped with the program if there is one, then
/// the platform's own sans. It is the only entry a packaged build can count on.
fn default_candidates() -> Vec<(String, i64)> {
    let mut v = Vec::new();
    for dir in bundled_dirs() {
        v.push((format!("{}/NotoSansCJK-Medium.ttc", dir), SC_FACE));
        v.push((format!("{}/NotoSansCJK-Regular.ttc", dir), SC_FACE));
    }
    for (path, face) in medium_paths() {
        v.push(((*path).to_string(), *face));
    }
    v
}

/// Noto Sans CJK Medium, the weight the dialog layout was tuned on.
fn medium_paths() -> &'static [(&'static str, i64)] {
    if cfg!(target_os = "windows") {
        &[("C:/Windows/Fonts/msyhbd.ttc", 0), ("C:/Windows/Fonts/msyh.ttc", 0)]
    } else if cfg!(target_os = "macos") {
        &[
            ("/System/Library/Fonts/PingFang.ttc", 0),
            ("/Library/Fonts/NotoSansCJK-Medium.ttc", SC_FACE),
        ]
    } else {
        &[
            ("/usr/share/fonts/opentype/noto/NotoSansCJK-Medium.ttc", SC_FACE),
            ("/usr/share/fonts/truetype/noto/NotoSansCJK-Medium.ttc", SC_FACE),
            ("/usr/local/share/fonts/noto/NotoSansCJK-Medium.ttc", SC_FACE),
        ]
    }
}

/// A face that cannot draw the script is not offered: it would put tofu, or
/// Japanese glyph shapes, in the middle of the Chinese lines.
fn script_ok(ft: &mut ft2::Ft) -> bool {
    ft.glyph('汉').is_some() && ft.glyph('あ').is_some() && ft.glyph('A').is_some()
}
/// FNV-1a over "filename#face": the id travels with the font file rather than
/// with the position it happened to sort into on this machine.
fn stable_id(path: &str, face: i64) -> i32 {
    let name = std::path::Path::new(path)
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| path.to_string());
    let mut h: u32 = 0x811c_9dc5;
    let key = format!("{}#{}", name, face);
    for b in key.as_bytes() {
        h ^= *b as u32;
        h = h.wrapping_mul(0x0100_0193);
    }
    match (h & 0x7fff_ffff) as i32 {
        0 => 1,
        n => n,
    }
}

/// How many faces of a file to try. A ttc holds one per script region and the
/// Chinese one is rarely face 0.
fn faces_in(path: &str) -> i64 {
    let low = path.to_ascii_lowercase();
    if !low.ends_with(".ttc") && !low.ends_with(".otc") {
        return 1;
    }
    match ft2::Ft::open(path, 24, 0) {
        Some(ft) => ft.num_faces().clamp(1, 8),
        None => 1,
    }
}

/// Where the platform keeps its fonts.
fn font_search_dirs() -> Vec<String> {
    let mut v = Vec::new();
    if cfg!(target_os = "windows") {
        v.push("C:/Windows/Fonts".to_string());
    } else if cfg!(target_os = "macos") {
        v.push("/System/Library/Fonts".to_string());
        v.push("/System/Library/Fonts/Supplemental".to_string());
        v.push("/Library/Fonts".to_string());
    } else {
        v.push("/usr/share/fonts".to_string());
        v.push("/usr/local/share/fonts".to_string());
        if let Ok(home) = std::env::var("HOME") {
            v.push(format!("{}/.local/share/fonts", home));
            v.push(format!("{}/.fonts", home));
        }
    }
    v
}

fn collect_font_files(out: &mut Vec<String>, dir: &str, depth: usize) {
    if depth > 6 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else { return };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            collect_font_files(out, &p.to_string_lossy(), depth + 1);
        } else {
            let low = p.to_string_lossy().to_ascii_lowercase();
            if low.ends_with(".ttf") || low.ends_with(".otf") || low.ends_with(".ttc") || low.ends_with(".otc") {
                out.push(p.to_string_lossy().into_owned());
            }
        }
    }
}

fn font_files() -> Vec<String> {
    let mut files = Vec::new();
    for d in font_search_dirs() {
        collect_font_files(&mut files, &d, 0);
    }
    files.sort();
    files.dedup();
    files
}
/// Every font on this machine that can actually draw the script, one entry per
/// family and style. Opening a thousand-odd files is slow, so the result is
/// kept in data/cache/fonts.tsv between runs.
/// Every font on this machine that can actually draw the script, one entry per
/// family and style. Opening a thousand-odd files is slow, so the result is
/// kept in data/cache/fonts.tsv between runs.
///
/// A collection is where this gets interesting: NotoSansCJK-Regular.ttc holds
/// five script regions and face 0 is Japanese, which has plenty of kanji and
/// would happily count as covered - and then draw Japanese glyph shapes for
/// hanzi like 骨, 直 and 着. So the Simplified Chinese face is looked for by
/// name, and only a file with no such face falls back to the first one that
/// has coverage.
fn scan_system_fonts() -> Vec<FontProfile> {
    let mut out = Vec::new();
    let mut seen: HashSet<String> = HashSet::new();
    for path in font_files() {
        let mut fallback: Option<FontProfile> = None;
        for face in 0..faces_in(&path) {
            let Some(mut ft) = ft2::Ft::open(&path, 24, face) else { break };
            if !script_ok(&mut ft) {
                continue;
            }
            let family = ft.family().unwrap_or_default();
            let style = ft.style().unwrap_or_default();
            let name = if style.is_empty() || style == "Regular" {
                family.clone()
            } else {
                format!("{} {}", family, style)
            };
            let prof = FontProfile {
                id: stable_id(&path, face),
                name,
                path: Some(path.clone()),
                face,
                family: family.clone(),
            };
            // The Simplified Chinese face of a collection: Noto Sans CJK SC,
            // SourceHanSansSC, PingFang SC - the families name their region.
            let sc = family.ends_with(" SC") || family.ends_with(" Hans");
            if sc {
                if seen.insert(format!("{}|{}", family, style)) {
                    out.push(prof);
                }
                fallback = None;
                break;
            }
            if fallback.is_none() {
                fallback = Some(prof);
            }
        }
        if let Some(p) = fallback {
            if seen.insert(format!("{}|{}", p.family, p.name)) {
                out.push(p);
            }
        }
    }
    out.sort_by(|a, b| a.name.to_lowercase().cmp(&b.name.to_lowercase()));
    out
}

fn font_cache_path() -> Option<std::path::PathBuf> {
    Some(crate::data::Data::resolve()?.root.join("cache").join("fonts.tsv"))
}

/// The cached scan, if the font directories still hold the same file count.
fn load_font_cache(count: usize) -> Option<Vec<FontProfile>> {
    let path = font_cache_path()?;
    let text = std::fs::read_to_string(&path).ok()?;
    let mut lines = text.lines();
    let recorded: usize = lines.next()?.trim().strip_prefix("# files=")?.parse().ok()?;
    if recorded != count {
        return None;
    }
    let mut out = Vec::new();
    for line in lines {
        let f: Vec<&str> = line.split('\t').collect();
        if f.len() < 5 {
            continue;
        }
        let path = f[2].to_string();
        if !std::path::Path::new(&path).is_file() {
            return None;
        }
        out.push(FontProfile {
            id: f[0].parse().ok()?,
            name: f[1].to_string(),
            path: Some(path),
            face: f[3].parse().unwrap_or(0),
            family: f[4].to_string(),
        });
    }
    Some(out)
}

fn save_font_cache(count: usize, list: &[FontProfile]) {
    let Some(path) = font_cache_path() else { return };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let mut out = format!("# files={}\n", count);
    for p in list {
        if let Some(path) = p.path.as_ref() {
            out.push_str(&format!("{}\t{}\t{}\t{}\t{}\n", p.id, p.name, path, p.face, p.family));
        }
    }
    let tmp = path.with_extension("tsv.tmp");
    if std::fs::write(&tmp, out).is_ok() {
        let _ = std::fs::rename(&tmp, &path);
    }
}
fn resolve_catalog() -> Vec<FontProfile> {
    let files = font_files();
    let discovered = match load_font_cache(files.len()) {
        Some(v) => v,
        None => {
            let v = scan_system_fonts();
            save_font_cache(files.len(), &v);
            v
        }
    };
    let mut def = FontProfile {
        id: 0,
        name: "默认".to_string(),
        path: None,
        face: SC_FACE,
        family: String::new(),
    };
    for (path, face) in default_candidates() {
        if let Some(mut ft) = ft2::Ft::open(&path, 24, face) {
            if script_ok(&mut ft) {
                def.family = ft.family().unwrap_or_default();
                def.face = face;
                def.path = Some(path);
                break;
            }
        }
    }
    match def.path.as_ref() {
        Some(path) => eprintln!("font: default -> {} (face {}) {}", path, def.face, def.family),
        None => eprintln!(
            "font: ERROR - nothing here can draw Chinese; install a CJK font or drop one in {}",
            bundled_dirs().first().cloned().unwrap_or_else(|| "fonts".to_string())
        ),
    }
    let mut out = Vec::with_capacity(discovered.len() + 1);
    out.push(def);
    let mut seen: HashSet<i32> = HashSet::new();
    seen.insert(0);
    for p in discovered {
        if seen.insert(p.id) {
            out.push(p);
        }
    }
    eprintln!("font: {} fonts here can draw Chinese", out.len() - 1);
    out
}

static CATALOG: std::sync::OnceLock<Vec<FontProfile>> = std::sync::OnceLock::new();

/// The catalogue: the default entry first, then everything the machine has
/// that can draw the script.
pub fn font_catalog() -> &'static [FontProfile] {
    CATALOG.get_or_init(resolve_catalog).as_slice()
}

pub fn font_profile(id: i32) -> Option<&'static FontProfile> {
    font_catalog().iter().find(|p| p.id == id)
}

/// Position of an id in the catalogue, for the arrow keys.
pub fn font_profile_index(id: i32) -> usize {
    font_catalog().iter().position(|p| p.id == id).unwrap_or(0)
}

pub fn font_profile_count() -> i32 {
    font_catalog().len() as i32
}

/// An id that names a font this machine still has; anything else (a stale
/// config, a hand-edited value, an uninstalled font) falls back to the
/// default rather than to a file that is not there.
fn clamp_profile_id(id: i32) -> i32 {
    match font_profile(id) {
        Some(p) if p.path.is_some() => id,
        _ => 0,
    }
}

/// Step through the catalogue from id, skipping entries that are not loadable.
/// Wraps around, so the arrows always land somewhere.
pub fn cycle_profile_id(id: i32, dir: i32) -> i32 {
    let cat = font_catalog();
    if cat.is_empty() {
        return 0;
    }
    let n = cat.len() as i32;
    let mut i = font_profile_index(id) as i32;
    for _ in 0..n {
        i = (i + dir + n) % n;
        if cat[i as usize].path.is_some() {
            return cat[i as usize].id;
        }
    }
    0
}

/// Point the three roles at the ids the player saved. Returns true when
/// anything changed, which is when the cached instances have to go.
pub fn set_role_ids(ids: [i32; 3]) -> bool {
    let next = [clamp_profile_id(ids[0]), clamp_profile_id(ids[1]), clamp_profile_id(ids[2])];
    let mut cur = ROLE_IDS.lock().unwrap_or_else(|e| e.into_inner());
    if *cur == next {
        return false;
    }
    *cur = next;
    ROLE_GEN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
    true
}

pub fn role_ids() -> [i32; 3] {
    *ROLE_IDS.lock().unwrap_or_else(|e| e.into_inner())
}

/// The id a role is using, already clamped to something loadable.
pub fn role_id(role: FontRole) -> i32 {
    role_ids()[role.index()]
}

/// True when the role asked for a font this machine cannot provide and the
/// default is standing in for it. The settings panel says so out loud instead
/// of letting the player believe the choice took effect.
pub fn role_fallback(role: FontRole) -> bool {
    if std::env::var_os(role.env_var()).is_some() || std::env::var_os("WA2_FONT").is_some() {
        return false;
    }
    match font_profile(role_id(role)) {
        Some(p) => p.path.is_none(),
        None => true,
    }
}

/// What the player asked for. WA2_FONT / WA2_FONT_FACE / WA2_FONT_BOLD /
/// WA2_FONT_GAMMA (or the legacy WA2_FONT_GAIN) / WA2_FONT_EDGE /
/// WA2_FONT_EDGE_ALPHA / WA2_FONT_HINT are the stand-in for the font settings
/// page, which writes the same values per role.
///
/// A role's own WA2_FONT_* wins, then the global WA2_FONT, then the profile id
/// from `wa2.ini`, then the default profile.
pub fn font_choice_for(role: FontRole) -> FontChoice {
    let spec = std::env::var(role.env_var())
        .or_else(|_| std::env::var("WA2_FONT"))
        .unwrap_or_default();
    let (path, face) = if spec.is_empty() {
        match font_profile(role_id(role)).and_then(|p| p.path.clone()) {
            Some(p) => (p, font_profile(role_id(role)).map(|p| p.face).unwrap_or(DEFAULT_FACE)),
            None => (DEFAULT_FONT.to_string(), DEFAULT_FACE),
        }
    } else {
        let (path, spec_face) = split_font_spec(&spec);
        let face = std::env::var("WA2_FONT_FACE")
            .ok()
            .and_then(|v| v.trim().parse::<i64>().ok())
            .or(spec_face)
            .unwrap_or(DEFAULT_FACE);
        (path, face)
    };
    env_choice(path, face)
}

/// The knobs that shape the ink, shared by every role: they are environment
/// only, because the settings page does not expose them yet.
fn env_choice(path: String, face: i64) -> FontChoice {
    let bold = std::env::var("WA2_FONT_BOLD")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(0);
    // The gamma curve is the default; the legacy gain only comes back when it
    // is asked for explicitly, so the two never mix in one run.
    let gamma = std::env::var("WA2_FONT_GAMMA")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok())
        .or(Some(DEFAULT_GAMMA));
    let gain = std::env::var("WA2_FONT_GAIN")
        .ok()
        .and_then(|v| v.parse().ok())
        .unwrap_or(DEFAULT_GAIN);
    let gamma = if std::env::var("WA2_FONT_GAIN").is_ok() { None } else { gamma };
    let edge_px = std::env::var("WA2_FONT_EDGE")
        .ok()
        .and_then(|v| v.trim().parse::<u32>().ok());
    let edge_alpha = std::env::var("WA2_FONT_EDGE_ALPHA")
        .ok()
        .and_then(|v| v.trim().parse::<u8>().ok())
        .unwrap_or(DEFAULT_EDGE_ALPHA);
    let hint = std::env::var("WA2_FONT_HINT")
        .ok()
        .and_then(|v| HintMode::parse(&v))
        .unwrap_or(HintMode::Light);
    FontChoice { path, face, bold, gamma, gain, edge_px, edge_alpha, hint }
}

/// Point a font at a choice. Both knobs drop the glyph cache, so this only does
/// work when something actually changed.
pub fn apply_choice(vf: &VectorFont, choice: &FontChoice) {
    vf.set_bold(choice.bold);
    match choice.gamma {
        Some(g) => vf.set_ink_gamma(g),
        None => vf.set_ink_gain(choice.gain),
    }
    vf.set_edge_px(choice.edge_px);
    vf.set_edge_alpha(choice.edge_alpha);
    vf.set_hint(choice.hint);
}

/// Open a font for one role, owned by the caller.
///
/// The runtime's dialogue font and the history screen hold theirs outright;
/// the shared UI path goes through [`with_role_font`].
pub fn open_role_font(role: FontRole, size: u32) -> Option<VectorFont> {
    let choice = font_choice_for(role);
    let vf = VectorFont::open_face(&choice.path, size.max(8), choice.face)?;
    apply_choice(&vf, &choice);
    // Which file and which face of it actually opened: a .ttc defaults to its
    // first (Japanese) face, and the file name alone never says which one the
    // role ended up with.
    eprintln!(
        "font role={} profile={} path={} family={} face={}",
        role.tag(),
        role_id(role),
        choice.path,
        vf.family(),
        choice.face
    );
    Some(vf)
}

/// Open one catalogue entry by id, for the panel's preview line.
pub fn open_profile_font(id: i32, size: u32) -> Option<VectorFont> {
    let p = font_profile(id)?;
    let path = p.path.as_ref()?;
    let vf = VectorFont::open_face(path, size.max(8), p.face)?;
    apply_choice(&vf, &env_choice(path.clone(), p.face));
    Some(vf)
}

/// Run f with one role's font at size pixels.
///
/// Screens that only need a caption or two borrow the font here instead of
/// threading one through every UI function. The instance is opened on first
/// use - or re-opened after the player changed that role's font - and is then
/// shared by every call for that role.
/// None means FreeType or the font file is unavailable, and the caller has to
/// do without the text.
pub fn with_role_font<R>(role: FontRole, size: u32, f: impl FnOnce(&VectorFont) -> R) -> Option<R> {
    ROLE_FONTS.with(|cell| {
        let gen = ROLE_GEN.load(std::sync::atomic::Ordering::Relaxed);
        let mut slots = cell.borrow_mut();
        let slot = &mut slots[role.index()];
        if slot.as_ref().map(|s| s.gen) != Some(gen) {
            *slot = open_role_font(role, size.max(8)).map(|vf| RoleFont { gen, vf });
        }
        let vf = &slot.as_ref()?.vf;
        if !vf.set_size(size) {
            return None;
        }
        apply_choice(vf, &font_choice_for(role));
        Some(f(vf))
    })
}

/// Run f with the menu font at size pixels.
///
/// Every screen that is not the dialogue or the history draws from the Home
/// role, so the player's menu choice covers the title screen, the option
/// screens, save/load, the timeline and the music mode in one go.
pub fn with_ui_font<R>(size: u32, f: impl FnOnce(&VectorFont) -> R) -> Option<R> {
    with_role_font(FontRole::Home, size, f)
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
fn stamp_raw(buf: &mut [u8], bw: u32, bh: u32, t: &Tile, pen: i32, baseline: i32, style: &TextStyle) {
    let ox = pen + t.dx;
    let oy = baseline + t.dy;
    for row in 0..t.h as i32 {
        for col in 0..t.w as i32 {
            let i = ((row as u32 * t.w + col as u32) * 4) as usize;
            let a = t.rgba[i];
            let e = t.rgba[i + 1];
            let (rgb, cov) = if a > 0 {
                (style.color, a)
            } else if style.edge_alpha > 0 && e > 0 {
                ([0, 0, 0], ((e as u32 * style.edge_alpha as u32) / 255) as u8)
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

/// Stamp one tile with a style, in one pass per pixel so the layers keep their
/// order: the ink wins where it exists, the translucent edge takes over around
/// it, and the soft shadow - the glyph's own coverage shifted by the style's
/// offset - only shows where neither reached.
fn stamp_styled(surf: &mut Surface, t: &Tile, pen: i32, baseline: i32, style: &TextStyle) {
    if t.w == 0 || t.h == 0 {
        return;
    }
    let ox = pen + t.dx;
    let oy = baseline + t.dy;
    let (sdx, sdy, shadow_alpha) = style.shadow;
    let edge_alpha = style.edge_alpha;
    let shadow_on = shadow_alpha > 0 && (sdx != 0 || sdy != 0);
    for row in 0..t.h as i32 {
        for col in 0..t.w as i32 {
            let i = ((row as u32 * t.w + col as u32) * 4) as usize;
            let a = t.rgba[i];
            let e = t.rgba[i + 1];
            let (x, y) = (ox + col, oy + row);
            if a > 0 {
                surf.blend_pixel(x, y, [style.color[0], style.color[1], style.color[2], a]);
            } else if e > 0 && edge_alpha > 0 {
                let cov = ((e as u32 * edge_alpha as u32) / 255) as u8;
                if cov > 0 {
                    surf.blend_pixel(x, y, [0, 0, 0, cov]);
                }
            } else if shadow_on {
                let (sc, sr) = (col - sdx, row - sdy);
                if sc >= 0 && sr >= 0 && sc < t.w as i32 && sr < t.h as i32 {
                    let ink_cov = t.rgba[((sr as u32 * t.w + sc as u32) * 4) as usize];
                    if ink_cov > 0 {
                        let cov = ((ink_cov as u32 * shadow_alpha as u32) / 255) as u8;
                        surf.blend_pixel(
                            x,
                            y,
                            [style.shadow_rgb[0], style.shadow_rgb[1], style.shadow_rgb[2], cov],
                        );
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{
        font_catalog, role_ids, round_266, set_role_ids, with_role_font, FontRole, Label, TextStyle,
        VectorFont, DEFAULT_FONT, DEFAULT_GAMMA,
    };
    use crate::render::Surface;

    /// The default entry is id 0, because 0 is what every old config file and
    /// every out-of-range value resolves to. Ids are otherwise hashes of the
    /// font file, so they are not dense - they just have to be unique.
    #[test]
    fn catalogue_ids_start_at_zero_and_are_unique() {
        let cat = font_catalog();
        assert_eq!(cat[0].id, 0);
        let mut ids: Vec<i32> = cat.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), cat.len(), "an id must name one font, not two");
    }

    /// An id this machine does not have - a hand-edited config, an
    /// uninstalled font - falls back to the default instead of to a file that
    /// is not there.
    #[test]
    fn unknown_profile_ids_fall_back_to_the_default() {
        let Some(real) = font_catalog().iter().find(|p| p.id != 0 && p.path.is_some()) else {
            return
        };
        set_role_ids([0, 0, 0]);
        set_role_ids([real.id, 0, 0]);
        assert_eq!(role_ids()[0], real.id, "a real id has to stick");
        set_role_ids([i32::MAX, -3, 0]);
        assert_eq!(role_ids(), [0, 0, 0], "out-of-range ids must not stick");
        set_role_ids([0, 0, 0]);
    }
    /// The whole point of the role layer: pointing one role at another profile
    /// leaves the others on the font they were opened with.
    #[test]
    fn roles_keep_their_own_font() {
        set_role_ids([0, 0, 0]);
        let Some(other) = font_catalog().iter().find(|p| p.id != 0 && p.path.is_some()) else {
            return
        };
        let game_before = with_role_font(FontRole::Game, 20, |f| f.describe());
        set_role_ids([other.id, 0, 0]);
        let home = with_role_font(FontRole::Home, 20, |f| f.describe());
        let game_after = with_role_font(FontRole::Game, 20, |f| f.describe());
        set_role_ids([0, 0, 0]);
        let (Some(home), Some(before), Some(after)) = (home, game_before, game_after) else {
            return
        };
        assert_eq!(before, after, "the game font must not follow the home one");
        assert_ne!(home, after, "the home font should follow its own id");
    }

    /// Tracking has to move the glyphs, not just the measured width: every gap
    /// between two glyphs gets the extra pixels. A loop that adds the tracking
    /// after the second glyph makes the first gap disappear while the measured
    /// width still counts it, so the line comes out left-shifted and the last
    /// glyphs crowd one gap.
    #[test]
    fn tracking_moves_glyphs_apart() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 24) else { return };
        let ink_span = |tracking: i32| {
            let label = Label {
                text: "画面设置".to_string(),
                x: 0,
                y: 0,
                w: 320,
                h: 40,
                size: 24,
                center: false,
                rgb: [255, 255, 255],
                tracking,
            };
            let buf = f.labels_rgba(320, 40, &[label]).expect("label buffer");
            let mut min = 320i32;
            let mut max = -1i32;
            for y in 0..40u32 {
                for x in 0..320u32 {
                    let i = ((y * 320 + x) * 4) as usize;
                    if buf[i + 3] > 0 {
                        min = min.min(x as i32);
                        max = max.max(x as i32);
                    }
                }
            }
            (min, max)
        };
        let (a0, a1) = ink_span(0);
        let (b0, b1) = ink_span(6);
        assert_eq!(b0, a0, "tracking must not move the first glyph");
        assert_eq!(b1 - a1, 6 * 3, "three gaps each get six pixels");
    }

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
                } else if s.pixels[i] < 200 {
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

    /// The dialog is 26px CJK: the default face and coverage curve have to
    /// leave the strokes mostly solid, with a continuous anti-aliased edge
    /// rather than a binary one.
    #[test]
    fn the_default_face_draws_solid_strokes_at_dialog_size() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        let mut surf = Surface::new(900, 60, [0, 0, 0, 255]);
        f.draw(&mut surf, "路邊的樹木葉子差不多已經落盡", 4, 40);
        let mut solid = 0usize;
        let mut partial = 0usize;
        let mut any = 0usize;
        for px in surf.pixels.chunks_exact(4) {
            if px[0] > 128 {
                any += 1;
                if px[0] > 245 {
                    solid += 1;
                } else {
                    partial += 1;
                }
            }
        }
        assert!(any > 500, "nothing drawn, {any}");
        let frac = solid as f32 / any as f32;
        // Mostly solid strokes, but not pushed to a binary edge: the old
        // 130% linear gain reached ~0.79 by saturating every mid-coverage
        // pixel, which is what closed the counters of dense glyphs. The gamma
        // curve keeps a real anti-aliased band instead.
        assert!(frac > 0.60, "strokes only {:.2} solid ({solid}/{any})", frac);
        assert!(partial as f32 / any as f32 > 0.15, "no anti-aliased edge ({partial}/{any})");
    }

    /// The gamma curve must not saturate the way the old linear gain did:
    /// pushing 130% linearly turned every mid-coverage pixel into solid ink
    /// and closed the counter of dense glyphs.
    #[test]
    fn the_gamma_curve_keeps_a_partial_edge() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        let Some((_w, _h, buf)) = f.line_rgba("鬱邊體樂觀發", 26) else { return };
        let mut solid = 0usize;
        let mut partial = 0usize;
        for px in buf.chunks_exact(4) {
            if px[3] == 0 {
                continue;
            }
            if px[3] == 255 {
                solid += 1;
            } else {
                partial += 1;
            }
        }
        assert!(solid > 100, "no solid ink in dense glyphs ({solid})");
        assert!(partial > 100, "edge saturated to solid ({partial})");
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

    /// The face index is what puts Simplified Chinese glyphs on screen: face 0
    /// of the same file is Japanese and draws 骨/直/着 with different shapes.
    #[test]
    fn the_sc_face_is_the_default() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        let family = f.family();
        assert!(family.contains("SC"), "default face is {family}");
    }

    /// Layout reads the metrics off the size object, so a line box centres the
    /// font's own vertical extent: the baseline sits between the ascender and
    /// the descender, not on top of the ascender alone.
    #[test]
    fn the_baseline_centres_the_line_box() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        let m = f.metrics();
        assert!(m.ascender > 0 && m.descender < 0, "metrics {m:?}");
        let base = f.baseline_in_box(100, 36);
        let above = base - 100;
        // The ink band the metrics describe sits centred in the box: the
        // slack above the ascender equals the slack below the descender.
        let slack_above = above - m.ascender;
        let slack_below = 36 - (above + m.descender);
        // Integer division can leave the slack a pixel uneven.
        assert!((slack_above - slack_below).abs() <= 1, "baseline {base} is not centred");
        assert!(slack_above >= 0, "ink taller than the line box: {slack_above}");
        // The old layout put the baseline one ascender below the box top,
        // which pushed every line down by the whole slack.
        assert!(above < m.ascender + 12, "baseline {base} leaves {above} above");
    }

    /// The default ink curve is the gamma one, and the legacy gain is opt-in.
    #[test]
    fn gamma_is_the_default_ink_curve() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        assert_eq!(f.ink_gamma(), Some(DEFAULT_GAMMA));
        assert_eq!(f.ink_gain(), None);
        f.set_ink_gain(130);
        assert_eq!(f.ink_gain(), Some(130));
        assert_eq!(f.ink_gamma(), None);
    }

    /// The face index is load-bearing, not cosmetic: 直 and 令 are drawn
    /// with visibly different shapes by the Japanese and the Simplified
    /// Chinese face of the same file. Rendering both and comparing proves the
    /// engine is really on the SC face it claims to be on.
    #[test]
    fn the_face_index_changes_the_glyphs() {
        let (Some(jp), Some(sc)) = (
            VectorFont::open_face(DEFAULT_FONT, 26, 0),
            VectorFont::open_face(DEFAULT_FONT, 26, 2),
        ) else {
            return
        };
        let mut diff_total = 0usize;
        for ch in ['直', '令', '门'] {
            let mut a = Surface::new(80, 80, [0, 0, 0, 255]);
            let mut b = Surface::new(80, 80, [0, 0, 0, 255]);
            jp.draw_flat(&mut a, &ch.to_string(), 20, 50, [255, 255, 255]);
            sc.draw_flat(&mut b, &ch.to_string(), 20, 50, [255, 255, 255]);
            let diff = a.pixels.iter().zip(&b.pixels).filter(|(x, y)| x != y).count();
            assert!(diff > 100, "{ch}: JP and SC faces drew the same glyph ({diff} px differ)");
            diff_total += diff;
        }
        // The default must be the SC face, i.e. the same one as index 2.
        let Some(def) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        let mut a = Surface::new(80, 80, [0, 0, 0, 255]);
        let mut b = Surface::new(80, 80, [0, 0, 0, 255]);
        def.draw_flat(&mut a, "直", 20, 50, [255, 255, 255]);
        sc.draw_flat(&mut b, "直", 20, 50, [255, 255, 255]);
        let diff = a.pixels.iter().zip(&b.pixels).filter(|(x, y)| x != y).count();
        assert_eq!(diff, 0, "default face differs from the SC face by {diff} px");
    }

    /// Kerning only applies between two Latin characters; CJK pairs must keep
    /// their full-width advance.
    #[test]
    fn kerning_applies_to_latin_only() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        let plain = f.measure_char('A') + f.measure_char('V');
        let kerned = f.measure("AV");
        assert!(kerned <= plain, "AV {kerned} wider than {plain}");
        let cjk = f.measure("一丁");
        assert_eq!(cjk, 26 * 2, "CJK pair measured {cjk}");
    }

    /// The body style stacks three separable layers, and on a mid-grey plate
    /// they land in different bands: white ink, a translucent dark edge around
    /// it, and a soft shadow. Measuring the bands is how the "not a black
    /// sticker" requirement stays true after later tuning.
    #[test]
    fn the_body_style_layers_ink_edge_and_shadow() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        let mut with = Surface::new(140, 90, [128, 128, 128, 255]);
        let mut without = Surface::new(140, 90, [128, 128, 128, 255]);
        let plain = TextStyle { shadow: (0, 0, 0), ..TextStyle::body() };
        f.draw_styled(&mut with, "國", 50, 60, &TextStyle::body());
        f.draw_styled(&mut without, "國", 50, 60, &plain);
        let at = |s: &Surface, x: i32, y: i32| -> u8 {
            if x < 0 || y < 0 || x >= s.width as i32 || y >= s.height as i32 {
                return 0;
            }
            s.pixels[((y as u32 * s.width + x as u32) * 4) as usize]
        };
        let (mut ink, mut ring, mut shadow) = (0usize, 0usize, 0usize);
        let mut misplaced = 0usize;
        for y in 0..with.height as i32 {
            for x in 0..with.width as i32 {
                let v = at(&with, x, y);
                let v0 = at(&without, x, y);
                if v > 220 {
                    ink += 1;
                } else if v < 60 {
                    ring += 1;
                } else if v < 105 {
                    shadow += 1;
                    // Everything the shadow adds must be the glyph's own
                    // coverage shifted one pixel down-right: the pixel up-left
                    // of it is touched by the glyph in the same render (ink
                    // brightens the grey plate, edge and shadow darken it), so
                    // an untouched plate pixel up-left means stray darkness.
                    if v != v0 && at(&with, x - 1, y - 1) == 128 {
                        misplaced += 1;
                    }
                }
            }
        }
        assert!(ink > 150, "no solid ink: {ink}");
        assert!(ring > 30, "no translucent edge ring: {ring}");
        assert!(shadow > 15, "no soft shadow: {shadow}");
        assert_eq!(misplaced, 0, "{misplaced} shadow pixels are not the shifted ink");
    }

    /// The CJK width policy: the punctuation the original drew in full-width
    /// cells keeps a full em whatever the font file says, so a line of dialogue
    /// has an even rhythm and the punctuation does not change width when the
    /// player swaps the font.
    #[test]
    fn cjk_punctuation_keeps_a_full_width_advance() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        let full = [
            '\u{3000}', '\u{3001}', '\u{3002}', '\u{300C}', '\u{300D}', '\u{300E}', '\u{300F}',
            '\u{2018}', '\u{2019}', '\u{201C}', '\u{201D}', '\u{2026}', '\u{203B}', '\u{2014}',
            // The fullwidth forms block is what the scripts actually use most:
            // '，' alone is 65k occurrences, '？' 15k, '！' 4.8k, '～' 3.7k.
            '\u{FF0C}', '\u{FF1F}', '\u{FF01}', '\u{FF5E}', '\u{FF08}', '\u{FF09}', '\u{FF1A}',
        ];
        for ch in full {
            assert_eq!(
                f.measure_char(ch),
                26,
                "U+{:04X} should advance a full em",
                ch as u32
            );
        }
        // A half-width space stays the font's own space, and Latin keeps its
        // proportional advance: this policy is about CJK punctuation, not
        // about squeezing everything to one width.
        let space = f.measure_char(' ');
        assert!(space > 0 && space < 13, "half-width space advanced {space}");
        assert!(f.measure_char('A') < 26, "Latin must stay proportional");
        // The halfwidth forms block is narrow by definition and must not have
        // been swept up by the U+FF01..U+FF60 range above.
        let halfwidth_stop = f.measure_char('\u{FF61}');
        assert!(halfwidth_stop > 0 && halfwidth_stop < 26, "halfwidth form widened to {halfwidth_stop}");
    }

    /// Two ellipsis dots are two cells wide - the shape the original had - and
    /// an em-dash run keeps the same rhythm, which is the one place Noto's own
    /// advance (23px for U+2014) would have made the line ragged.
    #[test]
    fn ellipsis_and_dash_runs_keep_the_dialogue_rhythm() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        let em = f.measure_char('\u{4E00}');
        assert_eq!(f.measure("\u{2026}\u{2026}"), 2 * em, "…… is two cells");
        assert_eq!(f.measure("\u{2014}\u{2014}"), 2 * em, "—— is two cells");
        // Mixed punctuation between ideographs must not change the count.
        assert_eq!(
            f.measure("\u{4E00}\u{3002}\u{4E00}"),
            3 * em,
            "ideograph, 。 , ideograph"
        );
    }

    /// Layout works in 26.6 and rounds only where a glyph is stamped, so a
    /// long Latin line does not lose its fractional advances one character at
    /// a time.
    #[test]
    fn a_long_latin_line_does_not_drift() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        // "AVAV" is the pair Noto kerns; repeating it 20 times is 40 Latin
        // glyphs, every one of them with a fractional advance.
        let line = "AVAVAVAVAVAVAVAVAVAVAVAVAVAVAVAVAVAV".to_string();
        // Per-glyph rounding would drift: each advance is 16px + a fraction,
        // so rounding 40 of them must not land 40x the error of one.
        let mut naive = 0i32;
        for ch in line.chars() {
            naive += f.measure_char(ch);
        }
        let exact = round_266(
            line.chars()
                .enumerate()
                .fold(0i32, |acc, (i, ch)| acc + f.advance_266(if i == 0 { None } else { line.chars().nth(i - 1) }, ch)),
        );
        assert_eq!(f.measure(&line), exact, "measure and the fixed-point pen disagree");
        // The kerning is what keeps this from being a wash; assert the two
        // differ by less than a pixel, i.e. no accumulation.
        assert!((naive - exact).abs() <= 1, "rounding drifted: {naive} vs {exact}");
    }

    /// A character the face has no glyph for leaves a gap of the width it
    /// would have had - no ragged hole, no placeholder box - and the font
    /// reports it once instead of swallowing it.
    #[test]
    fn a_missing_glyph_leaves_a_sized_gap() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        // A colour emoji is absent from a CJK text face.
        let missing = '\u{1F600}';
        assert_eq!(f.measure_char(missing), 26, "gap should be one em");
        // Drawn twice: the second pass must hit the cache, not warn again or
        // fall over.
        let mut s = Surface::new(200, 60, [0, 0, 0, 255]);
        f.draw_styled(&mut s, &missing.to_string(), 4, 40, &TextStyle::body());
        f.draw_styled(&mut s, &missing.to_string(), 4, 40, &TextStyle::body());
        // Text around it still lines up: gap + two ideographs.
        assert_eq!(f.measure(&format!("一{missing}一")), 26 * 3);
    }

    /// The name has to read as its own level, not as a second copy of the
    /// body line: a warmer ink and a firmer edge, no shadow.
    #[test]
    fn the_name_style_is_distinct_from_the_body() {
        let body = TextStyle::body();
        let name = TextStyle::name();
        assert_ne!(body.color, name.color, "name and body are the same colour");
        assert!(
            name.color[0] > name.color[2] && name.color[2] >= 200,
            "name should be a warm near-white, got {:?}",
            name.color
        );
        assert!(name.edge_alpha > body.edge_alpha, "name edge is not firmer");
        assert_eq!(name.shadow.2, 0, "the rim is dark; the name needs no shadow");
    }

    /// A style with no edge and no shadow draws exactly the ink, and a style
    /// with a shadow puts dark pixels below-right of the ink.
    #[test]
    fn styles_control_edge_and_shadow() {
        let Some(f) = VectorFont::open(DEFAULT_FONT, 26) else { return };
        let flat = TextStyle::ui();
        let mut a = Surface::new(120, 60, [255, 255, 255, 255]);
        f.draw_styled(&mut a, "字", 10, 40, &flat);
        let mut dark = 0;
        for px in a.pixels.chunks_exact(4) {
            if px[0] < 200 {
                dark += 1;
            }
        }
        assert_eq!(dark, 0, "flat style still darkens the plate");

        let body = TextStyle::body();
        let mut b = Surface::new(120, 60, [255, 255, 255, 255]);
        f.draw_styled(&mut b, "字", 10, 40, &body);
        let mut dark = 0;
        for px in b.pixels.chunks_exact(4) {
            if px[0] < 200 {
                dark += 1;
            }
        }
        assert!(dark > 10, "body style left no separation: {dark}");
    }
}
