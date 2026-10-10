//! Bitmap font rasterizer using a White Album 2 glyph atlas.
//!
//! The atlas is a row-major grid of `cell`x`cell` glyphs. A `Font` maps
//! characters to grid cells through an explicit `char -> cell` table. Two
//! tables are used by White Album 2:
//!
//!   * Chinese edition glyphs: the characters in `wa2.cht.txt` (Traditional
//!     Chinese) occupy cells `748 + i` in the patched atlas (`14pt本体.tga`,
//!     `fon.pak`). This mapping is fully verified.
//!   * Japanese kanji: `code_map.py` (wa2recode) gives the formula
//!     `cell = 189*(L-0x88) + (T-0x40) - 95` for a 2-byte Shift-JIS code
//!     `(L,T)` with `L >= 0x88`. This covers kanji in the original atlas
//!     (`fnt.pak`). Kana/ASCII positions require the complete (omitted)
//!     `code_maps` table and are rendered as fallback boxes until supplied.

use crate::codec::tga::TgaImage;
use crate::render::Surface;
use std::collections::HashMap;

pub struct Font {
    atlas: TgaImage,
    cell: u32,
    cols: u32,
    /// char -> grid cell index
    map: HashMap<char, u32>,
}

impl Font {
    /// Glyph cell size in pixels.
    pub fn cell(&self) -> u32 {
        self.cell
    }

    /// Number of glyph columns in the atlas grid.
    pub fn cols(&self) -> u32 {
        self.cols
    }

    /// The decoded glyph atlas.
    pub fn atlas(&self) -> &TgaImage {
        &self.atlas
    }

    /// Cell index for a character, if mapped.
    pub fn cell_of(&self, ch: char) -> Option<u32> {
        self.map.get(&ch).copied()
    }

    pub fn new(atlas: TgaImage, cell: u32, map: HashMap<char, u32>) -> Font {
        let cols = atlas.width / cell;
        Font { atlas, cell, cols, map }
    }

    /// Build a font from a charset string whose characters occupy cells
    /// `start_pos + i` in order (e.g. wa2.cht.txt at start_pos = 748).
    ///
    /// Whitespace is stripped first: the charset files contain formatting
    /// newlines/spaces, and wa2refont's `read_character_encoding` removes all
    /// `\s` before using the order. Counting whitespace as glyphs would shift
    /// every subsequent character's cell and render the wrong glyphs.
    pub fn from_charset(atlas: TgaImage, charset: &str, cell: u32, start_pos: u32) -> Font {
        let mut map = HashMap::new();
        for (i, ch) in charset.chars().filter(|c| !c.is_whitespace()).enumerate() {
            map.insert(ch, start_pos + i as u32);
        }
        Font::new(atlas, cell, map)
    }

    /// Load an explicit char->cell map. Each line of `path` is
    /// `<char>\t<cell>` (cell decimal). Unknown characters fall back.
    pub fn from_map_file(atlas: TgaImage, cell: u32, path: &str) -> std::io::Result<Font> {
        let data = std::fs::read_to_string(path)?;
        let mut map = HashMap::new();
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.split_whitespace();
            if let (Some(ch), Some(cell)) = (it.next(), it.next()) {
                if let Ok(cell) = cell.parse::<u32>() {
                    if let Some(c) = ch.chars().next() {
                        map.insert(c, cell);
                    }
                }
            }
        }
        Ok(Font::new(atlas, cell, map))
    }

    /// Resolve a character to a glyph cell via the explicit charset/map only.
    ///
    /// NOTE: the Leaf `code_maps` kanji formula (189*(L-0x88)+(T-0x40)-95) was
    /// found NOT to match the `font_jp` (fnt.pak) atlas layout — e.g. it places
    /// 一 (0x88EA) at cell 75, which is blank there. Applying it yields *wrong*
    /// glyphs (worse than a placeholder), so we deliberately do not fall back to
    /// it. Chars absent from the map render as placeholder boxes.
    pub fn resolve_cell(&self, ch: char) -> Option<u32> {
        self.map.get(&ch).copied()
    }

    /// Draw `text` left-to-right starting at (x, y). Characters with no glyph
    /// cell are skipped (advancing by one cell).
    pub fn draw(&self, surf: &mut Surface, text: &str, x: i32, y: i32) {
        let mut cx = x;
        for ch in text.chars() {
            if let Some(idx) = self.resolve_cell(ch) {
                let col = (idx % self.cols) as i32;
                let row = (idx / self.cols) as i32;
                let gx = col * self.cell as i32;
                let gy = row * self.cell as i32;
                surf.blit_region(&self.atlas, gx, gy, self.cell, self.cell, cx, y);
            }
            cx += self.cell as i32;
        }
    }

    /// Like `draw` but renders a translucent magenta box for unmapped
    /// characters so coverage gaps are visible (used to expose missing
    /// kana/ASCII mappings during development).
    pub fn draw_with_fallback(&self, surf: &mut Surface, text: &str, x: i32, y: i32) {
        let mut cx = x;
        for ch in text.chars() {
            if let Some(idx) = self.resolve_cell(ch) {
                let col = (idx % self.cols) as i32;
                let row = (idx / self.cols) as i32;
                let gx = col * self.cell as i32;
                let gy = row * self.cell as i32;
                surf.blit_region(&self.atlas, gx, gy, self.cell, self.cell, cx, y);
            } else {
                surf.fill_rect(cx, y, self.cell, self.cell, [255, 0, 255, 110]);
            }
            cx += self.cell as i32;
        }
    }
}
