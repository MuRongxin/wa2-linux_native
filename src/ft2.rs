//! Minimal FreeType binding via runtime dynamic loading (dlopen/dlsym).
//!
//! Same approach as sdl2.rs: no crates, no build-time dependency, and a
//! graceful None when the library is absent. FreeType is used for one thing -
//! turning a character of a font *file* into an 8-bit coverage mask plus its
//! metrics - so text can be drawn at the window's real pixel size instead of
//! being stretched up from the 1280x720 layer.
//!
//! Three struct fields are read straight out of FreeType's structs. Their
//! offsets are the LP64 ones for freetype 2.x, taken from
//! /usr/include/freetype2/freetype/freetype.h:
//!
//!   * FT_FaceRec.glyph        @152  (the glyph slot)
//!   * FT_GlyphSlotRec.format   @144
//!   * FT_GlyphSlotRec.bitmap   @152, an FT_Bitmap of 40 bytes:
//!       rows @0, width @4, pitch @8, buffer @16, pixel_mode @26
//!   * FT_GlyphSlotRec.bitmap_left @192, bitmap_top @196
//!   * FT_GlyphSlotRec.metrics.horiAdvance @80 (26.6 fixed point)
//!
//! Rather than trusting that silently, Ft::open renders one probe glyph and
//! checks that the slot points back at the face we opened and that FreeType
//! reports a bitmap. A wrong offset makes the whole font unusable instead of
//! producing garbage pixels.

use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_long, c_uchar, c_uint, c_ulong, c_void};

extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const RTLD_NOW: c_int = 2;

/// FT_LOAD_RENDER: load the glyph and render it into the slot's bitmap.
const FT_LOAD_RENDER: c_int = 0x0000_0004;
/// FT_RENDER_MODE_NORMAL: 8-bit anti-aliased coverage.
const FT_RENDER_MODE_NORMAL: c_int = 0;
/// FT_ENCODING_UNICODE, as FT_IMAGE_TAG('u','n','i','c').
const FT_ENCODING_UNICODE: c_int = 0x756E_6963u32 as c_int;
/// FT_GLYPH_FORMAT_BITMAP, as FT_IMAGE_TAG('b','i','t','s').
const FT_GLYPH_FORMAT_BITMAP: u32 = 0x6269_7473;
/// FT_PIXEL_MODE_GRAY: buffer holds one coverage byte per pixel.
const FT_PIXEL_MODE_GRAY: u8 = 2;

// Struct offsets, see the module comment.
const FACE_UNITS_PER_EM: usize = 136;
const FACE_ASCENDER: usize = 138;
const FACE_GLYPH: usize = 152;
const SLOT_FACE: usize = 8;
const SLOT_FORMAT: usize = 144;
const SLOT_HORI_ADVANCE: usize = 80;
const SLOT_BITMAP: usize = 152;
const SLOT_BITMAP_LEFT: usize = 192;
const SLOT_BITMAP_TOP: usize = 196;
// Inside FT_Bitmap (40 bytes):
const BM_ROWS: usize = 0;
const BM_WIDTH: usize = 4;
const BM_PITCH: usize = 8;
const BM_BUFFER: usize = 16;
const BM_PIXEL_MODE: usize = 26;

type FtError = c_int;

struct Fns {
    new_face: unsafe extern "C" fn(*mut c_void, *const c_char, c_long, *mut *mut c_void) -> FtError,
    set_pixel_sizes: unsafe extern "C" fn(*mut c_void, c_uint, c_uint) -> FtError,
    select_charmap: unsafe extern "C" fn(*mut c_void, c_int) -> FtError,
    get_char_index: unsafe extern "C" fn(*mut c_void, c_ulong) -> c_uint,
    load_glyph: unsafe extern "C" fn(*mut c_void, c_uint, c_int) -> FtError,
    render_glyph: unsafe extern "C" fn(*mut c_void, c_int) -> FtError,
    done_face: unsafe extern "C" fn(*mut c_void) -> FtError,
    done_library: unsafe extern "C" fn(*mut c_void) -> FtError,
}

/// One rasterized glyph: a coverage mask and where it sits relative to the pen.
pub struct Glyph {
    /// Mask size in pixels.
    pub w: u32,
    pub h: u32,
    /// Coverage, w * h bytes, row-major. 0 is transparent, 255 solid.
    pub mask: Vec<u8>,
    /// Horizontal offset from the pen position to the left edge of the mask.
    pub left: i32,
    /// Vertical offset from the baseline *up* to the top edge of the mask.
    pub top: i32,
    /// How far the pen moves right, in pixels.
    pub advance: i32,
}

/// An open font file at one pixel size.
pub struct Ft {
    library: *mut c_void,
    face: *mut c_void,
    size: u32,
    fns: Fns,
}

unsafe fn sym(lib: *mut c_void, name: &str) -> Option<*mut c_void> {
    let c = CString::new(name).ok()?;
    let p = dlsym(lib, c.as_ptr());
    if p.is_null() { None } else { Some(p) }
}

impl Ft {
    /// Open path (a .ttf / .otf / .ttc) at size pixels. None when FreeType,
    /// the file, or a struct offset does not line up.
    pub fn open(path: &str, size: u32) -> Option<Ft> {
        if size == 0 {
            return None;
        }
        unsafe {
            let mut lib = std::ptr::null_mut();
            for cand in ["libfreetype.so.6", "libfreetype.so"] {
                let c = CString::new(cand).unwrap();
                lib = dlopen(c.as_ptr(), RTLD_NOW);
                if !lib.is_null() {
                    break;
                }
            }
            if lib.is_null() {
                return None;
            }
            let init: unsafe extern "C" fn(*mut *mut c_void) -> FtError =
                std::mem::transmute(sym(lib, "FT_Init_FreeType")?);
            let fns = Fns {
                done_library: std::mem::transmute(sym(lib, "FT_Done_FreeType")?),
                new_face: std::mem::transmute(sym(lib, "FT_New_Face")?),
                set_pixel_sizes: std::mem::transmute(sym(lib, "FT_Set_Pixel_Sizes")?),
                select_charmap: std::mem::transmute(sym(lib, "FT_Select_Charmap")?),
                get_char_index: std::mem::transmute(sym(lib, "FT_Get_Char_Index")?),
                load_glyph: std::mem::transmute(sym(lib, "FT_Load_Glyph")?),
                render_glyph: std::mem::transmute(sym(lib, "FT_Render_Glyph")?),
                done_face: std::mem::transmute(sym(lib, "FT_Done_Face")?),
            };
            let mut library: *mut c_void = std::ptr::null_mut();
            if init(&mut library) != 0 {
                return None;
            }
            let cpath = CString::new(path).ok()?;
            let mut face: *mut c_void = std::ptr::null_mut();
            if (fns.new_face)(library, cpath.as_ptr(), 0, &mut face) != 0 || face.is_null() {
                let _ = (fns.done_library)(library);
                return None;
            }
            let mut ft = Ft { library, face, size: 0, fns };
            // A .ttc can open on a non-Unicode charmap; pick it explicitly.
            let _ = (ft.fns.select_charmap)(face, FT_ENCODING_UNICODE);
            if ft.set_size(size).is_none() || !ft.offsets_line_up() {
                return None;
            }
            // The dlopen handle stays open for the life of the process: the
            // function pointers kept in Fns would dangle otherwise.
            Some(ft)
        }
    }

    /// Rasterize at a new pixel size.
    pub fn set_size(&mut self, size: u32) -> Option<()> {
        if size == 0 {
            return None;
        }
        let r = unsafe { (self.fns.set_pixel_sizes)(self.face, 0, size as c_uint) };
        if r != 0 {
            return None;
        }
        self.size = size;
        Some(())
    }

    /// How far the face rises above the baseline, in pixels at the current
    /// size. Text layout aligns baselines with this instead of guessing from
    /// the ink of one glyph.
    pub fn ascent(&self) -> Option<i32> {
        let upem = unsafe { self.read_u16(self.face, FACE_UNITS_PER_EM) } as i32;
        if upem <= 0 {
            return None;
        }
        let asc = unsafe { self.read_i16(self.face, FACE_ASCENDER) } as i32;
        Some((asc * self.size as i32 + upem / 2) / upem)
    }

    /// Render one character. None when the font has no glyph for it, or the
    /// bitmap comes back in a pixel format we do not handle.
    pub fn glyph(&mut self, ch: char) -> Option<Glyph> {
        let slot = self.slot()?;
        unsafe {
            let index = (self.fns.get_char_index)(self.face, ch as u32 as c_ulong);
            if (self.fns.load_glyph)(self.face, index, FT_LOAD_RENDER) != 0 {
                return None;
            }
            // FT_LOAD_RENDER already renders; this keeps the mode explicit if
            // the load flags ever change.
            let _ = (self.fns.render_glyph)(slot, FT_RENDER_MODE_NORMAL);
            let rows = self.read_u32(slot, SLOT_BITMAP + BM_ROWS) as usize;
            let width = self.read_u32(slot, SLOT_BITMAP + BM_WIDTH) as usize;
            if self.read_u8(slot, SLOT_BITMAP + BM_PIXEL_MODE) != FT_PIXEL_MODE_GRAY {
                return None;
            }
            let left = self.read_i32(slot, SLOT_BITMAP_LEFT);
            let top = self.read_i32(slot, SLOT_BITMAP_TOP);
            let advance = (self.read_i64(slot, SLOT_HORI_ADVANCE) + 32) >> 6;
            let mut mask = vec![0u8; width * rows];
            if rows > 0 && width > 0 {
                let buf = self.read_ptr(slot, SLOT_BITMAP + BM_BUFFER) as *const c_uchar;
                if buf.is_null() {
                    return None;
                }
                let pitch = self.read_i32(slot, SLOT_BITMAP + BM_PITCH);
                for y in 0..rows {
                    // `pitch` is signed: FreeType pads rows downwards too.
                    let row = if pitch >= 0 { y * pitch as usize } else { (rows - 1 - y) * (-pitch) as usize };
                    let src = buf.add(row);
                    let dst = y * width;
                    for x in 0..width {
                        mask[dst + x] = *src.add(x);
                    }
                }
            }
            Some(Glyph {
                w: width as u32,
                h: rows as u32,
                mask,
                left,
                top,
                advance: advance as i32,
            })
        }
    }

    fn slot(&self) -> Option<*mut c_void> {
        let p = unsafe { self.read_ptr(self.face, FACE_GLYPH) };
        if p.is_null() { None } else { Some(p) }
    }

    /// Render a probe glyph and confirm the struct offsets we assumed.
    fn offsets_line_up(&mut self) -> bool {
        let Some(slot) = self.slot() else { return false };
        if unsafe { self.read_ptr(slot, SLOT_FACE) } != self.face {
            return false;
        }
        let g = match self.glyph('A') {
            Some(g) => g,
            None => return false,
        };
        // The slot must say it holds a bitmap now, and a glyph must advance.
        let format = unsafe { self.read_u32(slot, SLOT_FORMAT) };
        format == FT_GLYPH_FORMAT_BITMAP && g.advance > 0
    }

    /// Read a field at byte offset `off` from the struct at `p`.
    unsafe fn field(&self, p: *mut c_void, off: usize) -> *mut c_void {
        (p as *mut u8).add(off) as *mut c_void
    }
    unsafe fn read_ptr(&self, p: *mut c_void, off: usize) -> *mut c_void {
        *(self.field(p, off) as *mut *mut c_void)
    }
    unsafe fn read_u8(&self, p: *mut c_void, off: usize) -> u8 {
        *(self.field(p, off) as *mut u8)
    }
    unsafe fn read_u16(&self, p: *mut c_void, off: usize) -> u16 {
        *(self.field(p, off) as *mut u16)
    }
    unsafe fn read_i16(&self, p: *mut c_void, off: usize) -> i16 {
        *(self.field(p, off) as *mut i16)
    }
    unsafe fn read_u32(&self, p: *mut c_void, off: usize) -> u32 {
        *(self.field(p, off) as *mut u32)
    }
    unsafe fn read_i32(&self, p: *mut c_void, off: usize) -> i32 {
        *(self.field(p, off) as *mut i32)
    }
    unsafe fn read_i64(&self, p: *mut c_void, off: usize) -> i64 {
        *(self.field(p, off) as *mut i64)
    }
}

impl Drop for Ft {
    fn drop(&mut self) {
        unsafe {
            let _ = (self.fns.done_face)(self.face);
            let _ = (self.fns.done_library)(self.library);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Ft;

    /// The system CJK face the engine ships with. Tests skip when it is absent
    /// rather than failing a machine that has no Noto installed.
    const FONT: &str = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc";

    #[test]
    fn a_full_width_glyph_advances_one_em() {
        let Some(mut ft) = Ft::open(FONT, 40) else { return };
        let g = match ft.glyph('一') {
            Some(g) => g,
            None => return,
        };
        // An ideograph fills its advance: one em at 40px.
        assert_eq!(g.advance, 40);
        assert!(g.w > 30 && g.w <= 40, "ink width {}", g.w);
        // 一 is one horizontal stroke, far wider than tall. A wrong struct
        // offset would instead hand back 40x40 of noise.
        assert!(g.h > 0 && g.h < 12, "ink height {}", g.h);
        assert!(g.mask.iter().any(|&v| v > 200), "no solid coverage");
    }

    #[test]
    fn latin_is_narrower_than_a_kanji() {
        let Some(mut ft) = Ft::open(FONT, 40) else { return };
        let (Some(a), Some(i)) = (ft.glyph('A'), ft.glyph('一')) else { return };
        assert!(a.advance < i.advance, "A {} vs 一 {}", a.advance, i.advance);
        // Same baseline: both sit on it, so 一 rises far less than the cap.
        assert!(a.top > i.top, "A top {} vs 一 top {}", a.top, i.top);
    }
}
