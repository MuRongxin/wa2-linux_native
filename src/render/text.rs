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
        }
    }

    /// UI labels on their own plates: flat ink, nothing around it.
    pub fn ui() -> TextStyle {
        TextStyle {
            color: [255, 255, 255],
            edge_alpha: 0,
            shadow: (0, 0, 0),
            shadow_rgb: [0, 0, 0],
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

    /// Width of text at the current size, kerning included.
    pub fn measure(&self, text: &str) -> i32 {
        let mut x = 0;
        let mut prev: Option<char> = None;
        for ch in text.chars() {
            x += self.advance_after(prev, ch);
            prev = Some(ch);
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
        self.stamp_buf(&mut buf, w, h, text, pad, pad + top, &TextStyle::for_role(TextRole::Ui));
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
            self.stamp_buf(&mut buf, w, h, line, pad, base, &TextStyle::for_role(TextRole::Ui));
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
            // UI labels carry their own colour and sit on an opaque plate, so
            // they take the flat style: no edge, no shadow.
            let style = TextStyle { color: l.rgb, ..TextStyle::for_role(TextRole::Ui) };
            self.stamp_buf(&mut buf, w, h, &l.text, x, y + top, &style);
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
    ) {
        let mut pen = (x as i32) << 6;
        let mut prev: Option<char> = None;
        for ch in text.chars() {
            let t = self.tile(ch);
            stamp_raw(buf, bw, bh, &t, round_266(pen), baseline, style);
            pen += self.advance_266(prev, ch);
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
}

thread_local! {
    static UI: RefCell<Option<VectorFont>> = const { RefCell::new(None) };
}

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

/// What the player asked for. WA2_FONT / WA2_FONT_FACE / WA2_FONT_BOLD /
/// WA2_FONT_GAMMA (or the legacy WA2_FONT_GAIN) / WA2_FONT_EDGE /
/// WA2_FONT_EDGE_ALPHA / WA2_FONT_HINT are the stand-in for the font settings
/// page, which will write the same values.
pub fn font_choice() -> FontChoice {
    let spec = std::env::var("WA2_FONT").unwrap_or_else(|_| DEFAULT_FONT.to_string());
    let (path, spec_face) = split_font_spec(&spec);
    let face = std::env::var("WA2_FONT_FACE")
        .ok()
        .and_then(|v| v.trim().parse::<i64>().ok())
        .or(spec_face)
        .unwrap_or(DEFAULT_FACE);
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
            *slot = VectorFont::open_face(&choice.path, size.max(8), choice.face);
            // The title menu, settings, save/load and timeline labels all come
            // from this instance, which is separate from the dialogue font the
            // runtime opens. Without a line here there is nothing in the log to
            // tell which face those screens drew with.
            if let Some(vf) = slot.as_ref() {
                eprintln!("ui font: {}", vf.describe());
            }
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
    use super::{round_266, TextStyle, VectorFont, DEFAULT_FONT, DEFAULT_GAMMA};
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
