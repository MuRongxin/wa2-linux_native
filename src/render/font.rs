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
    /// char -> grid cell index (legacy charset tables / explicit map files)
    map: HashMap<char, u32>,
    /// Shift-JIS code -> grid cell index, loaded from a `wa2_cell_map_*.tsv`.
    ///
    /// This is the authoritative table (it is the game exe's own font table,
    /// converted to cells); when present it takes priority over every heuristic
    /// in `resolve_cell`. Keying by *code* rather than by character is
    /// deliberate: Shift-JIS 0x8160/0x817C decode to different Unicode
    /// codepoints depending on the codec (U+301C vs U+FF5E, U+2212 vs U+FF0D),
    /// so a char-keyed table silently loses those glyphs.
    code_map: HashMap<u32, u32>,
    /// True when `code_map` came from an authoritative table.
    authoritative: bool,
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
        Font {
            atlas,
            cell,
            cols,
            map,
            code_map: HashMap::new(),
            authoritative: false,
        }
    }

    /// Build a font from a `wa2_cell_map_*.tsv` (`<sjis-code> <char> <cell>`),
    /// which is the game exe's own font table expressed as atlas cells.
    ///
    /// With this table loaded, glyph lookup is exact - no hand-tuned formulas,
    /// no codec ambiguity - and the Chinese atlas (`cn_body80.tga`) works with
    /// the same code path as the Japanese one.
    pub fn from_cell_map(atlas: TgaImage, cell: u32, path: &std::path::Path) -> std::io::Result<Font> {
        let data = std::fs::read_to_string(path)?;
        let mut code_map = HashMap::new();
        for line in data.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with('#') {
                continue;
            }
            let mut it = line.split_whitespace();
            let (code, _ch, cellidx) = match (it.next(), it.next(), it.next()) {
                (Some(a), Some(b), Some(c)) => (a, b, c),
                _ => continue,
            };
            let code = match code.strip_prefix("0x").or_else(|| code.strip_prefix("0X")) {
                Some(h) => u32::from_str_radix(h, 16).ok(),
                None => code.parse::<u32>().ok(),
            };
            if let (Some(code), Ok(cellidx)) = (code, cellidx.parse::<u32>()) {
                code_map.insert(code, cellidx);
            }
        }
        let cols = atlas.width / cell;
        Ok(Font {
            atlas,
            cell,
            cols,
            map: HashMap::new(),
            code_map,
            authoritative: true,
        })
    }

    /// Number of mapped glyphs (diagnostic).
    pub fn mapped_glyphs(&self) -> usize {
        if self.authoritative { self.code_map.len() } else { self.map.len() }
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

    /// Build a font from a charset string, KEEPING whitespace.
    ///
    /// Use for charsets that map 1:1 onto the atlas and whose first entries are
    /// legitimately blank/space glyphs (e.g. the SJIS-ordered table where cell 0
    /// is a space and cell 1 is '!'). Stripping whitespace there would shift
    /// every glyph by one cell.
    pub fn from_charset_raw(atlas: TgaImage, charset: &str, cell: u32, start_pos: u32) -> Font {
        let mut map = HashMap::new();
        for (i, ch) in charset.chars().enumerate() {
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

    /// Resolve a character to a glyph cell.
    ///
    /// Two-byte Shift-JIS characters are computed with the `jp_body80` atlas
    /// layout, which was verified cell-by-cell against contact sheets:
    ///  - 0x81 (punctuation/symbols): `cell = 138 + n`, n = 1 for 0x8140
    ///    (、=140, 。=141, 「=192, 」=193, 『=194, …=174, ÷=202, ＆=223)
    ///  - 0x82 (hiragana, 0x829F..0x82F1): `cell = 360 + (T-0x9F)` (ぁ..ん)
    ///  - 0x83 (katakana, 0x8340..0x8396): `cell = 443 + j` (ァ..ヶ)
    ///  - kanji (lead >= 0x88): `cell = 189*(L-0x88) + (T-0x40) + 653`,
    ///    verified with "春希小春雪菜和泉千晶冬馬曽北原屋敷"
    /// ASCII uses `cell = code - 0x20` (cell 0 = blank, 16 = '0', 33 = 'A').
    pub fn resolve_cell(&self, ch: char) -> Option<u32> {
        if self.authoritative {
            // Exact path: ASCII at `code - 0x20` (verified against both atlases),
            // everything double-byte from the exe font table. Characters the
            // table does not contain are left blank on purpose - a wrong glyph
            // reads worse than a gap, and the heuristic formulas below are known
            // to be off by one in places (e.g. ム renders as ・).
            if let Some(bytes) = crate::codec::sjis::encode_char(ch) {
                if bytes.len() == 1 {
                    let c = bytes[0] as u32;
                    if (0x20..=0x7E).contains(&c) {
                        return Some(c - 0x20);
                    }
                } else if bytes.len() == 2 {
                    let code = ((bytes[0] as u32) << 8) | bytes[1] as u32;
                    if let Some(cell) = self.code_map.get(&code) {
                        return Some(*cell);
                    }
                }
            }
            return self.map.get(&ch).copied();
        }
        if let Some(bytes) = crate::codec::sjis::encode_char(ch) {
            // ASCII: atlas cell 0 is 0x20 (blank), cell 1 = '!', cell 16 = '0',
            // cell 33 = 'A' — matches the contact sheets. (The earlier -1 shift
            // came from charset files: `from_charset` strips whitespace, which
            // also dropped the leading space glyph and shifted every ASCII cell.)
            if bytes.len() == 1 {
                let c = bytes[0] as i32;
                if (0x20..=0x7E).contains(&c) {
                    return Some((c - 0x20) as u32);
                }
            }
            if bytes.len() == 2 {
                let (l, t) = (bytes[0] as i32, bytes[1] as i32);
                let cell = match l {
                    0x81 => {
                        if (0x40..=0x7E).contains(&t) {
                            138 + (t - 0x3F)
                        } else if (0x80..=0x9E).contains(&t) {
                            // The atlas inserts one extra glyph at cell 202 (a
                            // "·"-like mark), shifting every symbol from ÷
                            // (0x8180) onward by +1 — verified cell-by-cell:
                            // ÷=203, ＆=224, ☆=228, ＜=206, ＞=207.
                            139 + (t - 0x40)
                        } else {
                            -1
                        }
                    }
                    0x82 => {
                        if (0x9F..=0xF1).contains(&t) {
                            360 + (t - 0x9F)
                        } else {
                            -1
                        }
                    }
                    0x83 => {
                        if (0x40..=0x7E).contains(&t) {
                            443 + (t - 0x40)
                        } else if (0x80..=0x96).contains(&t) {
                            443 + (t - 0x41)
                        } else {
                            -1
                        }
                    }
                    _ if l >= 0x88 => 189 * (l - 0x88) + (t - 0x40) + 653,
                    _ => -1,
                };
                if cell >= 0 {
                    return Some(cell as u32);
                }
            }
        }
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
