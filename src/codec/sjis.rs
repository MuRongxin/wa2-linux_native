//! CP932 (Shift-JIS) codec, self-contained (no external crates).
//!
//! Uses the generated `SJIS_TO_UNICODE` table. Decoding turns raw game bytes
//! (scenario .txt, LSCR text strings) into Rust `String`. Encoding does the
//! reverse lookup, which the font module uses to place glyphs in the atlas via
//! the Leaf `code_maps` formula.

use crate::codec::sjis_table::SJIS_TO_UNICODE;
use std::collections::HashMap;
use std::sync::OnceLock;

/// unicode -> cp932 code, built lazily from the forward table.
fn reverse_table() -> &'static HashMap<u32, u16> {
    static REV: OnceLock<HashMap<u32, u16>> = OnceLock::new();
    REV.get_or_init(|| SJIS_TO_UNICODE.iter().map(|&(k, v)| (v, k)).collect())
}

/// Look up a single cp932 code point value -> Unicode scalar, if defined.
fn lookup(code: u16) -> Option<u32> {
    match SJIS_TO_UNICODE.binary_search_by_key(&code, |&(k, _)| k) {
        Ok(i) => Some(SJIS_TO_UNICODE[i].1),
        Err(_) => None,
    }
}

/// Decode CP932 bytes to a String. Unmapped bytes become U+FFFD.
pub fn decode(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        let b = bytes[i];
        if b < 0x80 || (0xA1..=0xDF).contains(&b) {
            // single byte
            let code = b as u16;
            match lookup(code) {
                Some(u) => out.push(char::from_u32(u).unwrap_or('\u{FFFD}')),
                None => out.push('\u{FFFD}'),
            }
            i += 1;
        } else if i + 1 < bytes.len() {
            let code = ((b as u16) << 8) | bytes[i + 1] as u16;
            match lookup(code) {
                Some(u) => out.push(char::from_u32(u).unwrap_or('\u{FFFD}')),
                None => out.push('\u{FFFD}'),
            }
            i += 2;
        } else {
            out.push('\u{FFFD}');
            i += 1;
        }
    }
    out
}

/// Encode a single char to its cp932 byte sequence, if encodable.
pub fn encode_char(ch: char) -> Option<Vec<u8>> {
    let rev = reverse_table();
    let code = *rev.get(&(ch as u32))?;
    if code < 0x100 {
        Some(vec![code as u8])
    } else {
        Some(vec![(code >> 8) as u8, (code & 0xFF) as u8])
    }
}

/// Leaf `code_maps` glyph-cell formula for a 2-byte cp932 code `(lead, trail)`.
///
/// `cell = 189*(lead - 0x88) + (trail - 0x40) - 95`, valid for `lead >= 0x88`
/// (kanji range). Returns `None` for codes outside that range (kana/ASCII,
/// whose cells require the complete omitted table).
pub fn kanji_cell(lead: u8, trail: u8) -> Option<u32> {
    if lead < 0x88 {
        return None;
    }
    let cell = 189 * (lead as i32 - 0x88) + (trail as i32 - 0x40) - 95;
    if cell >= 0 {
        Some(cell as u32)
    } else {
        None
    }
}

/// Cell index for a char, if it maps to a kanji glyph in the atlas.
pub fn cell_for_char(ch: char) -> Option<u32> {
    let bytes = encode_char(ch)?;
    if bytes.len() == 2 {
        kanji_cell(bytes[0], bytes[1])
    } else {
        None
    }
}

/// Decode a CP932 NUL-terminated string from a byte slice (returns bytes up to
/// the first NUL, decoded).
pub fn decode_cstr(bytes: &[u8]) -> String {
    let end = bytes.iter().position(|&b| b == 0).unwrap_or(bytes.len());
    decode(&bytes[..end])
}
