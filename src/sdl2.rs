//! Minimal SDL2 frontend via runtime dynamic loading (dlopen/dlsym).
//!
//! Load the SDL2 runtime (.so on Linux, .dylib/framework on macOS) without dev
//! headers, `sdl2.pc`, or a build-time dependency. We call only the handful of
//! functions needed to blit our offscreen `Surface` to a real window. If SDL2
//! is missing the app degrades gracefully (`Sdl::new` returns `None`).

use crate::render::Surface;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::{Mutex, OnceLock};

extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const RTLD_NOW: c_int = 2;

// Keep the Linux search order unchanged. Absolute Homebrew paths also work
// when dyld's search environment is stripped (for example, GUI launches).
#[cfg(not(target_os = "macos"))]
const SDL_LIBRARIES: &[&str] = &[
    "libSDL2-2.0.so.0", "libSDL2.so.0", "libSDL2.so", "libSDL2-2.0.so",
];
#[cfg(target_os = "macos")]
const SDL_LIBRARIES: &[&str] = &[
    "libSDL2-2.0.0.dylib", "libSDL2.dylib",
    "/opt/homebrew/lib/libSDL2-2.0.0.dylib",
    "/opt/homebrew/lib/libSDL2.dylib",
    "/opt/homebrew/opt/sdl2/lib/libSDL2.dylib",
    "/usr/local/lib/libSDL2-2.0.0.dylib",
    "/usr/local/lib/libSDL2.dylib",
    "/usr/local/opt/sdl2/lib/libSDL2.dylib",
    "SDL2.framework/SDL2", "/Library/Frameworks/SDL2.framework/SDL2",
];

/// Use the same discovery for the window and diagnostics. Handles remain open
/// because the runtime keeps function pointers into SDL for its entire lifetime.
unsafe fn open_sdl_library() -> *mut c_void {
    for &candidate in SDL_LIBRARIES {
        let name = CString::new(candidate).unwrap();
        let lib = dlopen(name.as_ptr(), RTLD_NOW);
        if !lib.is_null() {
            return lib;
        }
    }
    std::ptr::null_mut()
}

fn default_audio_driver() -> Option<&'static str> {
    // Preserve the existing Linux/PipeWire preference. On macOS let SDL choose
    // CoreAudio, and always honor an explicit SDL_AUDIODRIVER override.
    if cfg!(target_os = "linux") { Some("pulse") } else { None }
}

// SDL constants
const SDL_INIT_VIDEO: u32 = 0x0000_0020;
const SDL_INIT_AUDIO: u32 = 0x0000_0010;
const SDL_WINDOW_SHOWN: u32 = 0x0000_0004;
const SDL_WINDOW_ALLOW_HIGHDPI: u32 = 0x0000_2000;
const SDL_WINDOW_RESIZABLE: u32 = 0x0000_0020;
/// Borderless window the size of the desktop. Exclusive fullscreen (flag 1)
/// asks the display to switch to 1280×720, which the compositor ignores, so
/// the 「全屏」 setting never changed the window.
const SDL_WINDOW_FULLSCREEN_DESKTOP: u32 = 0x0000_1001;
const SDL_WINDOWPOS_CENTERED: i32 = 0x2FFF_0000u32 as i32;
const SDL_RENDERER_ACCELERATED: u32 = 0x0000_0002;
const SDL_RENDERER_SOFTWARE: u32 = 0x0000_0001;
const SDL_RENDERER_PRESENTVSYNC: u32 = 0x0000_0004;
// SDL_PIXELFORMAT_ABGR8888: on little-endian the in-memory byte order is
// R,G,B,A - exactly how `Surface::pixels` and `TgaImage::rgba` are laid out.
// (0x16362004 would be ARGB8888 => B,G,R,A, i.e. red/blue swapped.)
const SDL_PIXELFORMAT_RGBA32: u32 = 0x1676_2004; // == SDL_PIXELFORMAT_ABGR8888
const SDL_TEXTUREACCESS_STATIC: c_int = 0;
const SDL_QUIT: u32 = 0x100;
const SDL_KEYDOWN: u32 = 0x300;
const SDL_MOUSEMOTION: u32 = 0x400;
const SDL_MOUSEBUTTONDOWN: u32 = 0x401;
const SDL_MOUSEBUTTONUP: u32 = 0x402;
const SDL_MOUSEWHEEL: u32 = 0x403;
const SDLK_ESCAPE: i32 = 41;
const SDLK_q: i32 = 20;
const SDLK_SPACE: i32 = 44;
const SDLK_RETURN: i32 = 40;
const SDLK_RIGHT: i32 = 79;
const SDLK_DOWN: i32 = 81;
const SDLK_LEFT: i32 = 80;
const SDLK_UP: i32 = 82;
const SDLK_PAGEUP: i32 = 75;
// SDL scancode (USB HID usage): F1 = 58 ... F11 = 68. 95 was the X11
// keycode, which is what `xmodmap` shows, not what SDL puts in the event.
const SDLK_F11: i32 = 68;
const SDLK_1: i32 = 30;
const SDLK_2: i32 = 31;
const SDLK_3: i32 = 32;
const SDLK_4: i32 = 33;
const SDLK_5: i32 = 34;
const SDLK_6: i32 = 35;
const SDLK_7: i32 = 36;
const SDLK_8: i32 = 37;
const SDLK_9: i32 = 38;
const SDLK_HOME: i32 = 74;
const SDLK_END: i32 = 77;
const SDLK_TAB: i32 = 43;
const SDLK_r: i32 = 21;
const SDLK_f: i32 = 9;
const SDLK_t: i32 = 23;

pub struct Sdl {
    _lib: *mut c_void,
    window: *mut c_void,
    renderer: *mut c_void,
    texture: *mut c_void,
    tex_w: u32,
    tex_h: u32,
    f_create_window: unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, c_int, u32) -> *mut c_void,
    f_create_renderer: unsafe extern "C" fn(*mut c_void, c_int, u32) -> *mut c_void,
    f_create_texture: unsafe extern "C" fn(*mut c_void, u32, c_int, c_int, c_int) -> *mut c_void,
    f_update_texture: unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void, c_int) -> c_int,
    f_render_clear: unsafe extern "C" fn(*mut c_void) -> c_int,
    f_render_copy: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, *const c_void) -> c_int,
    f_render_present: unsafe extern "C" fn(*mut c_void),
    f_render_read_pixels: unsafe extern "C" fn(*mut c_void, *const c_void, u32, *mut c_void, c_int) -> c_int,
    f_get_pixel_format_name: unsafe extern "C" fn(u32) -> *const c_char,
    f_get_window_size: unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int),
    f_get_renderer_output_size: unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int) -> c_int,
    f_poll_event: unsafe extern "C" fn(*mut c_void) -> c_int,
    f_destroy_texture: unsafe extern "C" fn(*mut c_void),
    f_destroy_renderer: unsafe extern "C" fn(*mut c_void),
    f_destroy_window: unsafe extern "C" fn(*mut c_void),
    f_delay: unsafe extern "C" fn(u32),
    f_create_cursor: Option<unsafe extern "C" fn(*const u8, *const u8, c_int, c_int, c_int, c_int) -> *mut c_void>,
    f_set_cursor: Option<unsafe extern "C" fn(*mut c_void)>,
    f_free_cursor: Option<unsafe extern "C" fn(*mut c_void)>,
    f_show_cursor: Option<unsafe extern "C" fn(c_int) -> c_int>,
    cursor_normal: *mut c_void,
    cursor_hover: *mut c_void,
    cursor_pressed: *mut c_void,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    None,
    Advance,
    Back,
    /// Right arrow. The timeline uses it to step forward along the scenario
    /// graph; every other screen treats it as Advance.
    Right,
    /// Left arrow. Screens that do not give it a meaning of their own treat
    /// it as `Back`; the font settings panel uses it to step backwards.
    Left,
    /// Menu navigation (arrow up / down).
    Up,
    Down,
    Fullscreen,
    Quit,
    /// Number key 1-9 pressed (for choice selection).
    Choice(u8),
    /// Pointer event in *window* coordinates: motion (`pressed == false`) or a
    /// button press (`pressed == true`). Use `Sdl::surface_pos` to convert to
    /// the 1280x720 surface the game draws into.
    Pointer { x: i32, y: i32, pressed: bool },
    /// Right mouse button pressed (its function is the `R_click` setting).
    RightClick,
    /// Pointer moved with the left button held (dragging).
    Drag { x: i32, y: i32 },
    /// Left button released. A click is a press followed by this, without a drag.
    Release { x: i32, y: i32 },
    /// Mouse wheel. Positive `y` is scroll up (away from the user).
    Wheel { y: i32 },
    /// Page Up: cruise backward through lines already shown.
    Rewind,
    /// Home / End / Tab / R / F. One variant so screens that do not use them
    /// keep their existing catch-all arms.
    Nav(NavKey),
}

/// Extra navigation keys understood by the timeline chart.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NavKey {
    /// Back to the story start.
    Home,
    /// Forward to the end of the current route.
    End,
    /// Next chapter band.
    Tab,
    /// Reset zoom and scroll.
    Reset,
    /// Back to the newest save's scene.
    Focus,
    /// Cycle the timeline route/chapter filter.
    Filter,
}

/// SDL_Event (56 bytes) and SDL_SysWMinfo (72 bytes) both require 8-byte
/// alignment on LP64. Spare capacity alone does not make a byte array safe
/// for SDL to access as one of these C structs.
#[repr(C, align(8))]
struct SdlRawBuffer([u8; 128]);

fn fullscreen_shortcut(sc: i32, mods: u16) -> bool {
    // F11 everywhere; macOS standard Control+Command+F and Alt+Enter too.
    sc == SDLK_F11 || (sc == SDLK_f && mods & 0x00c0 != 0 && mods & 0x0c00 != 0)
        || (sc == SDLK_RETURN && mods & 0x0300 != 0)
}

/// Fit a source aspect into the output without stretching artwork or hitboxes.
fn fitted_rect(w: i32, h: i32, sw: i32, sh: i32) -> [i32; 4] {
    let (w, h, sw, sh) = (w.max(1) as i64, h.max(1) as i64, sw.max(1) as i64, sh.max(1) as i64);
    let (cw, ch) = if w * sh <= h * sw { (w, (w * sh / sw).max(1)) }
        else { ((h * sw / sh).max(1), h) };
    [((w-cw)/2) as i32, ((h-ch)/2) as i32, cw as i32, ch as i32]
}

fn valid_rgba(w: u32, h: u32, len: usize) -> bool {
    w > 0 && h > 0 && w <= i32::MAX as u32 / 4 && h <= i32::MAX as u32
        && (w as usize).checked_mul(h as usize).and_then(|n| n.checked_mul(4)) == Some(len)
}

unsafe fn sym(lib: *mut c_void, name: &str) -> Option<*mut c_void> {
    let c = CString::new(name).ok()?;
    let p = dlsym(lib, c.as_ptr());
    if p.is_null() { None } else { Some(p) }
}

/// Return SDL's 1-bit cursor data and mask for the normal, hover and pressed
/// states. SDL copies these buffers during `SDL_CreateCursor`, so stack arrays
/// are sufficient here.
fn cursor_bitmap(kind: u8) -> ([u8; 32], [u8; 32]) {
    let mut data = [0u8; 32];
    let mut mask = [0u8; 32];
    let mut pixel = |x: usize, y: usize, black: bool| {
        if x >= 16 || y >= 16 { return; }
        let i = y * 2 + x / 8;
        let bit = 0x80 >> (x & 7);
        mask[i] |= bit;
        if black { data[i] |= bit; }
    };
    for y in 0..16usize {
        let tip_w = if kind == 1 { (y + 2).min(13) } else { (y + 1).min(11) };
        for x in 0..=tip_w {
            let border = x == 0 || x == tip_w || y == 0;
            pixel(x, y, border || kind == 2);
        }
        if y >= 8 {
            let stem = if kind == 1 { 5..=9 } else { 4..=8 };
            for x in stem {
                pixel(x, y, kind == 2 || x == 5 || x == 8);
            }
        }
    }
    (data, mask)
}

impl Sdl {
    /// Try to open a window + streaming texture. Returns None if SDL2 is
    /// unavailable or window creation fails (e.g. no display).
    pub fn new(title: &str, tw: u32, th: u32) -> Option<Sdl> {
        unsafe {
            let lib = open_sdl_library();
            if lib.is_null() { None } else { Sdl::init_from(lib, title, tw, th) }
        }
    }

    unsafe fn init_from(lib: *mut c_void, title: &str, tw: u32, th: u32) -> Option<Sdl> {
        let s_init = sym(lib, "SDL_Init")?;
        let f_create_window: unsafe extern "C" fn(*const c_char, c_int, c_int, c_int, c_int, u32) -> *mut c_void =
            std::mem::transmute(sym(lib, "SDL_CreateWindow")?);
        let f_create_renderer: unsafe extern "C" fn(*mut c_void, c_int, u32) -> *mut c_void =
            std::mem::transmute(sym(lib, "SDL_CreateRenderer")?);
        let f_create_texture: unsafe extern "C" fn(*mut c_void, u32, c_int, c_int, c_int) -> *mut c_void =
            std::mem::transmute(sym(lib, "SDL_CreateTexture")?);
        let f_update_texture: unsafe extern "C" fn(*mut c_void, *const c_void, *const c_void, c_int) -> c_int =
            std::mem::transmute(sym(lib, "SDL_UpdateTexture")?);
        let f_render_clear: unsafe extern "C" fn(*mut c_void) -> c_int = std::mem::transmute(sym(lib, "SDL_RenderClear")?);
        let f_render_copy: unsafe extern "C" fn(*mut c_void, *mut c_void, *const c_void, *const c_void) -> c_int =
            std::mem::transmute(sym(lib, "SDL_RenderCopy")?);
        let f_render_present: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(sym(lib, "SDL_RenderPresent")?);
        let f_render_read_pixels: unsafe extern "C" fn(*mut c_void, *const c_void, u32, *mut c_void, c_int) -> c_int = std::mem::transmute(sym(lib, "SDL_RenderReadPixels")?);
        let f_get_pixel_format_name: unsafe extern "C" fn(u32) -> *const c_char = std::mem::transmute(sym(lib, "SDL_GetPixelFormatName")?);
        let f_get_window_size: unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int) =
            std::mem::transmute(sym(lib, "SDL_GetWindowSize")?);
        let f_get_renderer_output_size: unsafe extern "C" fn(*mut c_void, *mut c_int, *mut c_int) -> c_int =
            std::mem::transmute(sym(lib, "SDL_GetRendererOutputSize")?);
        let f_open_audio: unsafe extern "C" fn(*const c_char, c_int, *const SdlAudioSpec, *mut SdlAudioSpec, c_int) -> u32 = std::mem::transmute(sym(lib, "SDL_OpenAudioDevice")?);
        let f_queue_audio: unsafe extern "C" fn(u32, *const c_void, u32) -> c_int = std::mem::transmute(sym(lib, "SDL_QueueAudio")?);
        let f_pause_audio: unsafe extern "C" fn(u32, c_int) = std::mem::transmute(sym(lib, "SDL_PauseAudioDevice")?);
        let f_queued_audio_size = std::mem::transmute(sym(lib, "SDL_GetQueuedAudioSize")?);
        let f_close_audio = std::mem::transmute(sym(lib, "SDL_CloseAudioDevice")?);
        let f_get_error: unsafe extern "C" fn() -> *const c_char = std::mem::transmute(sym(lib, "SDL_GetError")?);
        let f_poll_event: unsafe extern "C" fn(*mut c_void) -> c_int = std::mem::transmute(sym(lib, "SDL_PollEvent")?);
        let f_destroy_texture: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(sym(lib, "SDL_DestroyTexture")?);
        let f_destroy_renderer: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(sym(lib, "SDL_DestroyRenderer")?);
        let f_destroy_window: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(sym(lib, "SDL_DestroyWindow")?);
        let f_delay: unsafe extern "C" fn(u32) = std::mem::transmute(sym(lib, "SDL_Delay")?);
        let f_create_cursor = sym(lib, "SDL_CreateCursor")
            .map(|p| std::mem::transmute(p));
        let f_set_cursor = sym(lib, "SDL_SetCursor")
            .map(|p| std::mem::transmute(p));
        let f_free_cursor = sym(lib, "SDL_FreeCursor")
            .map(|p| std::mem::transmute(p));
        let f_show_cursor = sym(lib, "SDL_ShowCursor")
            .map(|p| std::mem::transmute(p));

        // SDL picks the driver at init time from SDL_AUDIODRIVER.
        if std::env::var_os("SDL_AUDIODRIVER").is_none() {
            if let Some(driver) = default_audio_driver() {
                std::env::set_var("SDL_AUDIODRIVER", driver);
            }
        }
        let init: unsafe extern "C" fn(u32) -> c_int = std::mem::transmute(s_init);
        // init video+audio together; if that fails, fall back to video only
        let r = init(SDL_INIT_VIDEO | SDL_INIT_AUDIO);
        if r != 0 { let _ = init(SDL_INIT_VIDEO); }
        // Publish the audio API only after SDL has been loaded. Previously these
        // pointers were resolved but never installed, so Mixer::new always got
        // device 0 and every run was silent.
        let _ = AUDIO.set(AudioFns { open: f_open_audio, queue: f_queue_audio, pause: f_pause_audio, queued_size: f_queued_audio_size, close: f_close_audio, get_error: f_get_error });
        let f_full: unsafe extern "C" fn(*mut c_void, u32) -> c_int =
            std::mem::transmute(sym(lib, "SDL_SetWindowFullscreen")?);
        let _ = FULLSCREEN.set(f_full);
        let t = CString::new(title).ok()?;
        let window = f_create_window(
            t.as_ptr(),
            SDL_WINDOWPOS_CENTERED,
            SDL_WINDOWPOS_CENTERED,
            tw as c_int,
            th as c_int,
            // Native resize handles and macOS green/fullscreen button must remain
            // available; drawable rendering and pointer mapping follow the size.
            SDL_WINDOW_SHOWN | SDL_WINDOW_ALLOW_HIGHDPI | SDL_WINDOW_RESIZABLE,
        );
        if window.is_null() {
            return None;
        }
        // The compositor already produces a full RGBA frame.  Uploading that
        // frame through SDL's software renderer makes every 1080p/4K frame a
        // second CPU-side copy.  Prefer the GPU path and keep software as a
        // fallback for Xvfb/llvmpipe setups and for explicit headless tests.
        let force_software = std::env::var_os("WA2_SOFTWARE").is_some();
        let mut renderer = if force_software {
            f_create_renderer(window, -1, SDL_RENDERER_SOFTWARE)
        } else {
            f_create_renderer(window, -1, SDL_RENDERER_ACCELERATED)
        };
        if renderer.is_null() {
            renderer = if force_software {
                f_create_renderer(window, -1, SDL_RENDERER_ACCELERATED)
            } else {
                f_create_renderer(window, -1, SDL_RENDERER_SOFTWARE)
            };
        }
        if renderer.is_null() {
            return None;
        }
        let texture = f_create_texture(renderer, SDL_PIXELFORMAT_RGBA32, SDL_TEXTUREACCESS_STATIC, tw as c_int, th as c_int);
        if texture.is_null() {
            return None;
        }
        let mut sdl = Sdl {
            _lib: lib,
            window,
            renderer,
            texture,
            tex_w: tw,
            tex_h: th,
            f_create_window,
            f_create_renderer,
            f_create_texture,
            f_update_texture,
            f_render_clear,
            f_render_copy,
            f_render_present,
            f_render_read_pixels,
            f_get_pixel_format_name,
            f_get_window_size,
            f_get_renderer_output_size,
            f_poll_event,
            f_destroy_texture,
            f_destroy_renderer,
            f_destroy_window,
            f_delay,
            f_create_cursor,
            f_set_cursor,
            f_free_cursor,
            f_show_cursor,
            cursor_normal: std::ptr::null_mut(),
            cursor_hover: std::ptr::null_mut(),
            cursor_pressed: std::ptr::null_mut(),
        };
        // The hand-built 16px monochrome arrow is NOT an original WA2 asset.
        // Keep the platform's sharp, DPI-aware pointer unless explicitly requested.
        if std::env::var("WA2_CURSOR").as_deref() == Ok("legacy") {
            sdl.enable_game_cursor();
        }
        let logical = sdl.window_size();
        let drawable = sdl.drawable_size();
        eprintln!("SDL: logical={}x{} drawable={}x{} pixel_ratio={:.2}x{:.2}",
            logical.0, logical.1, drawable.0, drawable.1,
            drawable.0 as f64 / logical.0 as f64, drawable.1 as f64 / logical.1 as f64);
        Some(sdl)
    }

    /// Install a small high-contrast game cursor. The original resource tree
    /// does not ship a standalone cursor image, so this uses the same cyan /
    /// dark outline language as the UI and keeps all three interaction states
    /// visible without relying on a desktop theme.
    fn enable_game_cursor(&mut self) {
        let (Some(create), Some(set), Some(show)) =
            (self.f_create_cursor, self.f_set_cursor, self.f_show_cursor)
        else {
            return;
        };
        let normal = cursor_bitmap(0);
        let hover = cursor_bitmap(1);
        let pressed = cursor_bitmap(2);
        unsafe {
            self.cursor_normal = create(normal.0.as_ptr(), normal.1.as_ptr(), 16, 16, 1, 1);
            self.cursor_hover = create(hover.0.as_ptr(), hover.1.as_ptr(), 16, 16, 1, 1);
            self.cursor_pressed = create(pressed.0.as_ptr(), pressed.1.as_ptr(), 16, 16, 1, 1);
            if !self.cursor_normal.is_null() {
                set(self.cursor_normal);
                show(1);
            }
        }
    }

    fn set_cursor_visual(&mut self, pressed: bool, hover: bool) {
        let Some(set) = self.f_set_cursor else { return };
        let cursor = if pressed {
            self.cursor_pressed
        } else if hover {
            self.cursor_hover
        } else {
            self.cursor_normal
        };
        if !cursor.is_null() {
            unsafe { set(cursor); }
        }
    }

    /// Upload `surf` pixels to the texture and present, scaled to the window.
    pub fn present(&mut self, surf: &Surface) {
        if !valid_rgba(surf.width, surf.height, surf.pixels.len()) {
            eprintln!("SDL: invalid surface dimensions or buffer length");
            return;
        }
        Self::dump_frame(surf);
        let t0 = if Self::timing() { Some(std::time::Instant::now()) } else { None };
        unsafe {
            if surf.width != self.tex_w || surf.height != self.tex_h {
                let t = (self.f_create_texture)(
                    self.renderer,
                    SDL_PIXELFORMAT_RGBA32,
                    SDL_TEXTUREACCESS_STATIC,
                    surf.width as c_int,
                    surf.height as c_int,
                );
                if t.is_null() {
                    eprintln!("SDL texture creation failed: {}", sdl_get_error());
                    return;
                }
                (self.f_destroy_texture)(self.texture);
                self.texture = t;
                self.tex_w = surf.width;
                self.tex_h = surf.height;
            }
            let pitch = (surf.width * 4) as c_int;
            (self.f_update_texture)(self.texture, std::ptr::null(), surf.pixels.as_ptr() as *const c_void, pitch);
            (self.f_render_clear)(self.renderer);
            let (ow, oh) = self.output_size();
            let rect = fitted_rect(ow, oh, self.tex_w as i32, self.tex_h as i32);
            (self.f_render_copy)(self.renderer, self.texture, std::ptr::null(), rect.as_ptr().cast());
            (self.f_render_present)(self.renderer);
        }
        if let Some(t0) = t0 {
            eprintln!(
                "  [timing] present {}x{}: {:.1} ms",
                surf.width,
                surf.height,
                t0.elapsed().as_secs_f64() * 1000.0
            );
        }
    }

    /// Checked rendering diagnostic: capture SDL's actual drawable pixels before
    /// Present invalidates its backbuffer. Drawable size can exceed window size
    /// on Retina. This verifies the renderer, not the macOS compositor.
    pub fn present_checked(&mut self, surf: &Surface) -> Result<Surface, String> {
        if !valid_rgba(surf.width, surf.height, surf.pixels.len()) {
            return Err("invalid RGBA surface".into());
        }
        unsafe {
            if surf.width != self.tex_w || surf.height != self.tex_h {
                let texture = (self.f_create_texture)(self.renderer, SDL_PIXELFORMAT_RGBA32,
                    SDL_TEXTUREACCESS_STATIC, surf.width as c_int, surf.height as c_int);
                if texture.is_null() { return Err(format!("create texture: {}", sdl_get_error())); }
                (self.f_destroy_texture)(self.texture);
                self.texture = texture;
                self.tex_w = surf.width;
                self.tex_h = surf.height;
            }
            if (self.f_update_texture)(self.texture, std::ptr::null(), surf.pixels.as_ptr().cast(),
                (surf.width * 4) as c_int) != 0 {
                return Err(format!("update texture: {}", sdl_get_error()));
            }
            let (ow, oh) = self.output_size();
            let destination = fitted_rect(ow, oh, surf.width as i32, surf.height as i32);
            if (self.f_render_clear)(self.renderer) != 0
                || (self.f_render_copy)(self.renderer, self.texture, std::ptr::null(), destination.as_ptr().cast()) != 0 {
                return Err(format!("render texture: {}", sdl_get_error()));
            }
            let (mut w, mut h) = (0, 0);
            if (self.f_get_renderer_output_size)(self.renderer, &mut w, &mut h) != 0
                || w <= 0 || h <= 0 || w > 16384 || h > 16384 {
                return Err(format!("invalid renderer output {w}x{h}: {}", sdl_get_error()));
            }
            let mut captured = Surface::new(w as u32, h as u32, [0, 0, 0, 0]);
            // An explicit rectangle also bounds writes if the window is resized
            // between output-size query and readback.
            let rect = [0i32, 0, w, h];
            if (self.f_render_read_pixels)(self.renderer, rect.as_ptr().cast(), SDL_PIXELFORMAT_RGBA32,
                captured.pixels.as_mut_ptr().cast(), w * 4) != 0 {
                return Err(format!("read renderer: {}", sdl_get_error()));
            }
            (self.f_render_present)(self.renderer);
            Ok(captured)
        }
    }

    /// WA2_FRAME_DUMP=path writes every presented frame as a TGA.
    ///
    /// X11 screen grabbers are unreliable here (ffmpeg -f x11grab hangs and
    /// import can come back empty), so the renderer exports its own frames.
    fn dump_frame(surf: &Surface) {
        static PATH: std::sync::OnceLock<Option<std::path::PathBuf>> = std::sync::OnceLock::new();
        let Some(path) = PATH
            .get_or_init(|| std::env::var_os("WA2_FRAME_DUMP").map(std::path::PathBuf::from))
            .as_ref()
        else {
            return;
        };
        let tmp = path.with_extension("tga.tmp");
        if surf.save_tga(&tmp.to_string_lossy()).is_ok() {
            let _ = std::fs::rename(&tmp, path);
        }
    }

    /// Whether scene-switch profiling is on (WA2_TIMING=1).
    fn timing() -> bool {
        static V: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
        *V.get_or_init(|| std::env::var_os("WA2_TIMING").is_some())
    }

    /// Upload a raw RGBA buffer (w*h*4 bytes) to the texture and present,
    /// stretched to the window. Used for video frames.
    pub fn present_raw(&mut self, rgba: &[u8], w: u32, h: u32) {
        if !valid_rgba(w, h, rgba.len()) {
            eprintln!("SDL: invalid raw RGBA dimensions or buffer length");
            return;
        }
        unsafe {
            if w != self.tex_w || h != self.tex_h {
                let t = (self.f_create_texture)(self.renderer, SDL_PIXELFORMAT_RGBA32, SDL_TEXTUREACCESS_STATIC, w as c_int, h as c_int);
                if t.is_null() {
                    eprintln!("SDL texture creation failed: {}", sdl_get_error());
                    return;
                }
                (self.f_destroy_texture)(self.texture);
                self.texture = t;
                self.tex_w = w;
                self.tex_h = h;
            }
            (self.f_update_texture)(self.texture, std::ptr::null(), rgba.as_ptr() as *const c_void, (w * 4) as c_int);
            (self.f_render_clear)(self.renderer);
            let (ow, oh) = self.output_size();
            let rect = fitted_rect(ow, oh, self.tex_w as i32, self.tex_h as i32);
            (self.f_render_copy)(self.renderer, self.texture, std::ptr::null(), rect.as_ptr().cast());
            (self.f_render_present)(self.renderer);
        }
    }

    /// Drain SDL events. A click or key that shares the queue with later mouse
    /// motion is kept: during skip the game spends a long time between polls,
    /// and the motion that follows a click would otherwise replace it.
    pub fn poll_input(&mut self) -> Input {
        let mut storage = SdlRawBuffer([0u8; 128]);
        let ev = &mut storage.0;
        let mut out = Input::None;
        unsafe {
            while (self.f_poll_event)(ev.as_mut_ptr() as *mut c_void) != 0 {
                let etype = u32::from_ne_bytes([ev[0], ev[1], ev[2], ev[3]]);
                if etype == SDL_QUIT {
                    out = Input::Quit;
                } else if etype == SDL_KEYDOWN {
                    // SDL_KeyboardEvent layout: type(4) + timestamp(4) +
                    // windowID(4) + state/repeat/padding(4), then SDL_Keysym at
                    // offset 16 whose first field is the scancode; the key
                    // constants above are SDL scancode values.
                    let sc = i32::from_ne_bytes([ev[16], ev[17], ev[18], ev[19]]);
                    let mods = u16::from_ne_bytes([ev[24], ev[25]]);
                    if ev[13] == 0 && fullscreen_shortcut(sc, mods) {
                        self.set_fullscreen(!self.is_fullscreen());
                        continue;
                    }
                    match sc {
                        SDLK_ESCAPE if self.is_fullscreen() => { self.set_fullscreen(false); }
                        SDLK_ESCAPE | SDLK_q => out = Input::Quit,
                        SDLK_F11 => {},
                        SDLK_LEFT => out = Input::Left,
                        SDLK_UP => out = Input::Up,
                        SDLK_PAGEUP => out = Input::Rewind,
                        SDLK_SPACE | SDLK_RETURN => out = Input::Advance,
                        SDLK_RIGHT => out = Input::Right,
                        SDLK_DOWN => out = Input::Down,
                        SDLK_1 => out = Input::Choice(1),
                        SDLK_2 => out = Input::Choice(2),
                        SDLK_3 => out = Input::Choice(3),
                        SDLK_4 => out = Input::Choice(4),
                        SDLK_5 => out = Input::Choice(5),
                        SDLK_6 => out = Input::Choice(6),
                        SDLK_7 => out = Input::Choice(7),
                        SDLK_8 => out = Input::Choice(8),
                        SDLK_9 => out = Input::Choice(9),
                        SDLK_HOME => out = Input::Nav(NavKey::Home),
                        SDLK_END => out = Input::Nav(NavKey::End),
                        SDLK_TAB => out = Input::Nav(NavKey::Tab),
                        SDLK_r => out = Input::Nav(NavKey::Reset),
                        SDLK_f => out = Input::Nav(NavKey::Focus),
                        SDLK_t => out = Input::Nav(NavKey::Filter),
                        _ => {}
                    }
                } else if etype == SDL_MOUSEMOTION {
                    // SDL_MouseMotionEvent: type,timestamp,windowID,which,state
                    // (20 bytes) then Sint32 x, y. `state` bit 0 is the left
                    // button, which is how a drag is recognised.
                    // Motion does not replace a click or key already taken
                    // from this same queue.
                    if matches!(out, Input::None | Input::Pointer { pressed: false, .. } | Input::Drag { .. }) {
                        let state = u32::from_ne_bytes([ev[16], ev[17], ev[18], ev[19]]);
                        let x = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                        let y = i32::from_ne_bytes([ev[24], ev[25], ev[26], ev[27]]);
                        self.set_cursor_visual(state & 1 != 0, state & 1 == 0);
                        out = if state & 1 != 0 {
                            Input::Drag { x, y }
                        } else {
                            Input::Pointer { x, y, pressed: false }
                        };
                    }
                } else if etype == SDL_MOUSEWHEEL {
                    // SDL_MouseWheelEvent: x at 16, y at 20. Positive y is
                    // scroll up. `direction` (2.0.4+) at 24 is 1 when the
                    // platform reports flipped axes.
                    let mut wy = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                    if ev[24] == 1 {
                        wy = -wy;
                    }
                    if wy != 0 && matches!(out, Input::None | Input::Pointer { pressed: false, .. } | Input::Drag { .. } | Input::Wheel { .. }) {
                        let prev = match out {
                            Input::Wheel { y } => y,
                            _ => 0,
                        };
                        out = Input::Wheel { y: prev + wy };
                    }
                } else if etype == SDL_MOUSEBUTTONDOWN {
                    // SDL_MouseButtonEvent: type..which (16), button,state,clicks,
                    // padding (4), then Sint32 x, y.
                    let button = ev[16];
                    let x = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                    let y = i32::from_ne_bytes([ev[24], ev[25], ev[26], ev[27]]);
                    self.set_cursor_visual(true, false);
                    out = if button == 3 {
                        Input::RightClick
                    } else {
                        Input::Pointer { x, y, pressed: true }
                    };
                } else if etype == SDL_MOUSEBUTTONUP {
                    // Same layout as the button-down event. A click is this
                    // release without a drag in between.
                    let button = ev[16];
                    self.set_cursor_visual(false, false);
                    if button == 1
                        && matches!(
                            out,
                            Input::None
                                | Input::Pointer { pressed: false, .. }
                                | Input::Drag { .. }
                                | Input::Release { .. }
                        )
                    {
                        let x = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                        let y = i32::from_ne_bytes([ev[24], ev[25], ev[26], ev[27]]);
                        out = Input::Release { x, y };
                    }
                }
            }
        }
        if out != Input::None && std::env::var_os("WA2_DEBUG_INPUT").is_some() {
            eprintln!("input: {:?}", out);
        }
        out
    }

/// Switch the window between windowed and fullscreen (the 「画面模式」
/// setting). Fullscreen covers the display the window is on.
///
/// `SDL_SetWindowFullscreen` updates SDL's own size immediately, but without
/// a window manager the real X window stays 1280×720, so the picture and the
/// pointer never change. After the request we move that window ourselves.
pub fn set_fullscreen(&self, on: bool) {
    let Some(f) = FULLSCREEN.get() else { return };
    if on == self.is_fullscreen() { return; }
    #[cfg(target_os = "macos")]
    {
        // Cocoa owns Spaces/fullscreen animations and restores window geometry.
        // Linux/X11 manual border/size hacks break the native green button.
        let rc = unsafe { (f)(self.window, if on { SDL_WINDOW_FULLSCREEN_DESKTOP } else { 0 }) };
        if rc != 0 { eprintln!("sdl: fullscreen change failed: {}", sdl_get_error()); }
        self.pump();
        IS_FULLSCREEN.store(self.is_fullscreen(), std::sync::atomic::Ordering::Relaxed);
        return;
    }
    #[cfg(not(target_os = "macos"))]
    if on {
        self.remember_windowed();
        let rc = unsafe { (f)(self.window, SDL_WINDOW_FULLSCREEN_DESKTOP) };
        if rc != 0 {
            eprintln!("sdl: set fullscreen failed");
        }
        if let Some((x, y, w, h)) = self.display_bounds() {
            self.set_bordered(false);
            self.x11_move_resize(x, y, w as u32, h as u32);
            self.sdl_set_pos(x, y);
            self.sdl_set_size(w, h);
        }
    } else {
        let rc = unsafe { (f)(self.window, 0) };
        if rc != 0 {
            eprintln!("sdl: leave fullscreen failed");
        }
        self.set_bordered(true);
        if let Some((x, y, w, h)) = take_windowed() {
            self.x11_move_resize(x, y, w as u32, h as u32);
            self.sdl_set_size(w, h);
            self.sdl_set_pos(x, y);
        }
    }
    self.pump();
    IS_FULLSCREEN.store(on, std::sync::atomic::Ordering::Relaxed);
    if std::env::var_os("WA2_DEBUG_INPUT").is_some() {
        let (w, h) = self.window_size();
        eprintln!("sdl: fullscreen {} -> {}x{}", on, w, h);
    }
}

/// Query the actual OS window flags, including native title-bar changes.
pub fn is_fullscreen(&self) -> bool {
    let Some(f) = self.fn_ptr::<unsafe extern "C" fn(*mut c_void) -> u32>("SDL_GetWindowFlags") else { return false; };
    unsafe { f(self.window) & 1 != 0 }
}

pub fn is_resizable(&self) -> bool {
    let Some(f) = self.fn_ptr::<unsafe extern "C" fn(*mut c_void) -> u32>("SDL_GetWindowFlags") else { return false; };
    unsafe { f(self.window) & SDL_WINDOW_RESIZABLE != 0 }
}

/// Used by bounded display diagnostics; interactive resizing is owned by Cocoa.
pub fn resize_window(&self, w: i32, h: i32) { self.sdl_set_size(w.max(320), h.max(180)); }

/// True while the window manager has the window maximized. The title-bar
/// maximize button is the quick way to a screen-filling window, and the art
/// decoder treats it like fullscreen (8x master) so that path gets the same
/// picture quality.
pub fn is_maximized(&self) -> bool {
    /// SDL_WINDOW_MAXIMIZED
    const SDL_WINDOW_MAXIMIZED: u32 = 0x0000_0080;
    let Some(f) = self.fn_ptr::<unsafe extern "C" fn(*mut c_void) -> u32>("SDL_GetWindowFlags")
    else {
        return false;
    };
    unsafe { f(self.window) & SDL_WINDOW_MAXIMIZED != 0 }
}

fn remember_windowed(&self) {
    let mut slot = match WINDOWED_GEOM.lock() {
        Ok(g) => g,
        Err(_) => return,
    };
    if slot.is_some() {
        return;
    }
    let (x, y) = self.window_pos();
    let (w, h) = self.window_size();
    *slot = Some((x, y, w, h));
}

fn window_pos(&self) -> (i32, i32) {
    let Some(f) = self.fn_ptr::<unsafe extern "C" fn(*mut c_void, *mut i32, *mut i32)>(
        "SDL_GetWindowPosition",
    ) else {
        return (0, 0);
    };
    let (mut x, mut y) = (0i32, 0i32);
    unsafe { f(self.window, &mut x, &mut y) };
    (x, y)
}

fn display_bounds(&self) -> Option<(i32, i32, i32, i32)> {
    let index = self.fn_ptr::<unsafe extern "C" fn(*mut c_void) -> i32>("SDL_GetWindowDisplayIndex")?;
    let bounds = self.fn_ptr::<unsafe extern "C" fn(i32, *mut i32) -> i32>("SDL_GetDisplayBounds")?;
    let idx = unsafe { index(self.window) };
    if idx < 0 {
        return None;
    }
    let mut rect = [0i32; 4];
    if unsafe { bounds(idx, rect.as_mut_ptr()) } != 0 {
        return None;
    }
    if rect[2] <= 0 || rect[3] <= 0 {
        return None;
    }
    Some((rect[0], rect[1], rect[2], rect[3]))
}

fn set_bordered(&self, on: bool) {
    let Some(f) = self.fn_ptr::<unsafe extern "C" fn(*mut c_void, i32)>("SDL_SetWindowBordered") else {
        return;
    };
    unsafe { f(self.window, if on { 1 } else { 0 }) };
}

fn sdl_set_size(&self, w: i32, h: i32) {
    let Some(f) = self.fn_ptr::<unsafe extern "C" fn(*mut c_void, i32, i32)>("SDL_SetWindowSize") else {
        return;
    };
    unsafe { f(self.window, w, h) };
}

fn sdl_set_pos(&self, x: i32, y: i32) {
    let Some(f) = self.fn_ptr::<unsafe extern "C" fn(*mut c_void, i32, i32)>("SDL_SetWindowPosition") else {
        return;
    };
    unsafe { f(self.window, x, y) };
}

fn pump(&self) {
    let Some(f) = self.fn_ptr::<unsafe extern "C" fn()>("SDL_PumpEvents") else { return };
    unsafe { f() };
}

fn fn_ptr<T>(&self, name: &str) -> Option<T> {
    let c = CString::new(name).ok()?;
    let p = unsafe { dlsym(self._lib, c.as_ptr()) };
    if p.is_null() {
        None
    } else {
        Some(unsafe { std::mem::transmute_copy(&p) })
    }
}

/// SDL's window id on X11, then move that window. `SDL_GetWindowWMInfo` needs
/// the library version filled in or it refuses.
fn x11_move_resize(&self, x: i32, y: i32, w: u32, h: u32) {
    let Some(get_ver) = self.fn_ptr::<unsafe extern "C" fn(*mut u8)>("SDL_GetVersion") else { return };
    let Some(get_info) =
        self.fn_ptr::<unsafe extern "C" fn(*mut c_void, *mut u8) -> i32>("SDL_GetWindowWMInfo")
    else {
        return;
    };
    let mut storage = SdlRawBuffer([0u8; 128]);
    let info = &mut storage.0;
    unsafe { get_ver(info.as_mut_ptr()) };
    if unsafe { get_info(self.window, info.as_mut_ptr()) } != 1 {
        return;
    }
    // subsystem sits after the 3-byte version, aligned to 4. X11 is 2.
    let subsystem = i32::from_ne_bytes([info[4], info[5], info[6], info[7]]);
    if subsystem != 2 {
        return;
    }
    let display = usize::from_ne_bytes(info[8..16].try_into().unwrap()) as *mut c_void;
    let xwin = u64::from_ne_bytes(info[16..24].try_into().unwrap());
    if display.is_null() || xwin == 0 {
        return;
    }
    unsafe {
        let c = CString::new("libX11.so.6").unwrap();
        let lib = dlopen(c.as_ptr(), RTLD_NOW);
        if lib.is_null() {
            return;
        }
        let lock = dlsym(lib, CString::new("XLockDisplay").unwrap().as_ptr());
        let unlock = dlsym(lib, CString::new("XUnlockDisplay").unwrap().as_ptr());
        let mv = dlsym(lib, CString::new("XMoveResizeWindow").unwrap().as_ptr());
        let fl = dlsym(lib, CString::new("XFlush").unwrap().as_ptr());
        if !lock.is_null() {
            let lock: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(lock);
            lock(display);
        }
        if !mv.is_null() {
            let mv: unsafe extern "C" fn(*mut c_void, u64, i32, i32, u32, u32) -> i32 =
                std::mem::transmute(mv);
            mv(display, xwin, x, y, w, h);
        }
        if !fl.is_null() {
            let fl: unsafe extern "C" fn(*mut c_void) -> i32 = std::mem::transmute(fl);
            fl(display);
        }
        if !unlock.is_null() {
            let unlock: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(unlock);
            unlock(display);
        }
    }
}

    /// Current window size in logical screen coordinates, used for mouse events
    /// and restoring window geometry. On Retina this is NOT the render size.
    pub fn window_size(&self) -> (i32, i32) {
        let (mut w, mut h) = (0i32, 0i32);
        unsafe { (self.f_get_window_size)(self.window, &mut w, &mut h); }
        (w.max(1), h.max(1))
    }

    /// Complete renderer output, including letterbox bars, in physical pixels.
    pub fn output_size(&self) -> (i32, i32) {
        let (mut w, mut h) = (0, 0);
        let rc = unsafe { (self.f_get_renderer_output_size)(self.renderer, &mut w, &mut h) };
        if rc == 0 && w > 0 && h > 0 { (w, h) } else { self.window_size() }
    }

    /// Game content pixels: preserve the authored 16:9 aspect on arbitrary windows.
    pub fn drawable_size(&self) -> (i32, i32) {
        let (w, h) = self.output_size();
        let r = fitted_rect(w, h, 1280, 720);
        (r[2], r[3])
    }

    /// Map logical window coordinates through the physical letterbox viewport.
    /// Outside bars return negative coordinates, never a clickable edge control.
    pub fn surface_pos(&self, x: i32, y: i32, sw: u32, sh: u32) -> (i32, i32) {
        let (ww, wh) = self.window_size();
        let (ow, oh) = self.output_size();
        let r = fitted_rect(ow, oh, 1280, 720);
        let px = x as i64 * ow as i64 / ww as i64;
        let py = y as i64 * oh as i64 / wh as i64;
        if px < r[0] as i64 || py < r[1] as i64 || px >= (r[0]+r[2]) as i64 || py >= (r[1]+r[3]) as i64 {
            return (-1, -1);
        }
        (((px-r[0] as i64)*sw as i64/r[2] as i64) as i32,
         ((py-r[1] as i64)*sh as i64/r[3] as i64) as i32)
    }

    /// Read back the current renderer target as RGBA (for verification),
    /// independent of the windowing/compositor path.
    pub fn read_back(&self, w: u32, h: u32) -> Vec<u8> {
        if w == 0 || h == 0 || w > 16384 || h > 16384 { return Vec::new(); }
        let mut buf = vec![0u8; w as usize * h as usize * 4];
        let pitch = (w * 4) as c_int;
        let rect = [0i32, 0, w as i32, h as i32];
        unsafe {
            if (self.f_render_read_pixels)(
                self.renderer,
                rect.as_ptr().cast(),
                SDL_PIXELFORMAT_RGBA32,
                buf.as_mut_ptr() as *mut c_void,
                pitch,
            ) != 0 {
                eprintln!("SDL readback failed: {}", sdl_get_error());
                return Vec::new();
            }
        }
        buf
    }

    /// Human-readable name of an SDL pixel format (diagnostic).
    pub fn format_name(&self, fmt: u32) -> String {
        unsafe {
            let p = (self.f_get_pixel_format_name)(fmt);
            if p.is_null() {
                return String::from("?");
            }
            std::ffi::CStr::from_ptr(p).to_string_lossy().into_owned()
        }
    }

    pub fn delay(&self, ms: u32) {
        unsafe { (self.f_delay)(ms); }
    }
}

impl Drop for Sdl {
    fn drop(&mut self) {
        unsafe {
            if let Some(free) = self.f_free_cursor {
                for cursor in [self.cursor_normal, self.cursor_hover, self.cursor_pressed] {
                    if !cursor.is_null() {
                        free(cursor);
                    }
                }
            }
            (self.f_destroy_texture)(self.texture);
            (self.f_destroy_renderer)(self.renderer);
            (self.f_destroy_window)(self.window);
        }
    }
}
// ---- Audio device (global fn table so the mixer thread can call SDL) ----
const AUDIO_S16SYS: u32 = 0x8010;

#[repr(C)]
struct SdlAudioSpec {
    freq: c_int,
    format: u16,
    channels: u8,
    silence: u8,
    samples: u16,
    padding: u16,
    size: u32,
    callback: Option<unsafe extern "C" fn(*mut c_void, *mut u8, c_int)>,
    userdata: *mut c_void,
}

struct AudioFns {
    open: unsafe extern "C" fn(*const c_char, c_int, *const SdlAudioSpec, *mut SdlAudioSpec, c_int) -> u32,
    queue: unsafe extern "C" fn(u32, *const c_void, u32) -> c_int,
    pause: unsafe extern "C" fn(u32, i32),
    queued_size: unsafe extern "C" fn(u32) -> u32,
    close: unsafe extern "C" fn(u32),
    get_error: unsafe extern "C" fn() -> *const c_char,
}

static AUDIO: OnceLock<AudioFns> = OnceLock::new();
static FULLSCREEN: OnceLock<unsafe extern "C" fn(*mut c_void, u32) -> c_int> = OnceLock::new();
/// Last requested display mode. The artwork decoder picks the 8x master in
/// fullscreen and the 4x tree in a window, so it needs to know.
static IS_FULLSCREEN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// Position and size to restore when leaving fullscreen. Absent while windowed.
static WINDOWED_GEOM: Mutex<Option<(i32, i32, i32, i32)>> = Mutex::new(None);

fn take_windowed() -> Option<(i32, i32, i32, i32)> {
    WINDOWED_GEOM.lock().ok().and_then(|mut g| g.take())
}

/// Open a playback device (48kHz stereo, 1024-frame buffer). 0 on failure.
pub fn sdl_open_audio(rate: u32, ch: u16) -> u32 {
    let f = match AUDIO.get() { Some(f) => f, None => return 0 };
    let wanted = SdlAudioSpec {
        freq: rate as c_int,
        format: AUDIO_S16SYS as u16,
        channels: ch as u8,
        silence: 0,
        samples: 1024,
        padding: 0,
        size: 0,
        callback: None, // queued audio uses SDL_QueueAudio
        userdata: std::ptr::null_mut(),
    };
    let dev = unsafe { (f.open)(std::ptr::null(), 0, &wanted, std::ptr::null_mut(), 0) };
    if dev == 0 {
        let err = unsafe { std::ffi::CStr::from_ptr((f.get_error)()) }.to_string_lossy();
        eprintln!("SDL audio device open failed: {err}");
    }
    dev
}

/// Push interleaved s16le bytes to the audio device.
pub fn sdl_queue_audio(dev: u32, bytes: &[u8]) -> bool {
    let Some(f) = AUDIO.get() else { return false };
    if dev == 0 || bytes.len() > u32::MAX as usize { return false; }
    unsafe { (f.queue)(dev, bytes.as_ptr().cast(), bytes.len() as u32) == 0 }
}

/// Bytes waiting in SDL's device queue (not yet consumed by the backend).
pub fn sdl_queued_audio_size(dev: u32) -> u32 {
    AUDIO.get().map(|f| unsafe { (f.queued_size)(dev) }).unwrap_or(0)
}

/// Close only after the mixer worker has stopped using this device.
pub fn sdl_close_audio(dev: u32) {
    if dev != 0 {
        if let Some(f) = AUDIO.get() { unsafe { (f.close)(dev); } }
    }
}

/// Resume (paused=false) or pause the device.
pub fn sdl_pause_audio(dev: u32, paused: bool) {
    if let Some(f) = AUDIO.get() {
        unsafe { (f.pause)(dev, if paused { 1 } else { 0 }); }
    }
}

/// Name of the active SDL audio driver (diagnostic), e.g. "pulse" / "alsa" / "dummy".
pub fn sdl_audio_driver() -> String {
    unsafe {
        let lib = open_sdl_library();
        if lib.is_null() { return "?".into(); }
        let p = dlsym(lib, CString::new("SDL_GetCurrentAudioDriver").unwrap().as_ptr());
        if p.is_null() { return "?".into(); }
        let f: unsafe extern "C" fn() -> *const c_char = std::mem::transmute(p);
        let n = f();
        if n.is_null() { "none".into() } else { std::ffi::CStr::from_ptr(n).to_string_lossy().into_owned() }
    }
}

/// Name of the active SDL video driver (diagnostic), e.g. "cocoa" / "x11".
pub fn sdl_video_driver() -> String {
    unsafe {
        let lib = open_sdl_library();
        if lib.is_null() { return "?".into(); }
        let Some(p) = sym(lib, "SDL_GetCurrentVideoDriver") else { return "?".into() };
        let f: unsafe extern "C" fn() -> *const c_char = std::mem::transmute(p);
        let n = f();
        if n.is_null() { "none".into() } else { std::ffi::CStr::from_ptr(n).to_string_lossy().into_owned() }
    }
}

/// SDL_GetError() string (diagnostic).
pub fn sdl_get_error() -> String {
    unsafe {
        let lib = open_sdl_library();
        if lib.is_null() { return "?".into(); }
        let p = dlsym(lib, CString::new("SDL_GetError").unwrap().as_ptr());
        if p.is_null() { return "?".into(); }
        let f: unsafe extern "C" fn() -> *const c_char = std::mem::transmute(p);
        let e = f();
        if e.is_null() { "".into() } else { std::ffi::CStr::from_ptr(e).to_string_lossy().into_owned() }
    }
}

#[cfg(test)]
mod tests {
    use super::{cursor_bitmap, default_audio_driver, SDL_LIBRARIES};

    #[test]
    fn letterbox_preserves_game_aspect_for_square_wide_and_retina_outputs() {
        assert_eq!(super::fitted_rect(1600, 1600, 1280, 720), [0,350,1600,900]);
        assert_eq!(super::fitted_rect(2200, 1000, 1280, 720), [211,0,1777,1000]);
        assert_eq!(super::fitted_rect(3024, 1890, 1280, 720), [0,94,3024,1701]);
        assert_eq!(super::fitted_rect(2560, 1440, 1280, 720), [0,0,2560,1440]);
    }

    #[test]
    fn standard_fullscreen_shortcuts_do_not_consume_plain_typing() {
        assert!(super::fullscreen_shortcut(super::SDLK_F11, 0));
        assert!(super::fullscreen_shortcut(super::SDLK_f, 0x0440));
        assert!(super::fullscreen_shortcut(super::SDLK_RETURN, 0x0100));
        assert!(!super::fullscreen_shortcut(super::SDLK_f, 0));
        assert!(!super::fullscreen_shortcut(super::SDLK_f, 0x0040));
        assert!(!super::fullscreen_shortcut(super::SDLK_RETURN, 0));
    }

    #[test]
    fn raw_ffi_buffer_has_sdl_struct_capacity_and_alignment() {
        use std::mem::{align_of, size_of};
        assert_eq!(align_of::<super::SdlRawBuffer>(), 8);
        assert_eq!(size_of::<super::SdlRawBuffer>(), 128);
        let storage = super::SdlRawBuffer([0u8; 128]);
        assert_eq!(storage.0.as_ptr() as usize % 8, 0);
        assert!(storage.0.len() >= 56); // SDL_Event on LP64
        assert!(storage.0.len() >= 72); // SDL_SysWMinfo on LP64
    }

    #[test]
    #[cfg(target_pointer_width = "64")]
    fn audio_spec_matches_sdl_lp64_layout() {
        use std::mem::{align_of, offset_of, size_of};
        use super::SdlAudioSpec;
        assert_eq!(align_of::<SdlAudioSpec>(), 8);
        assert_eq!(size_of::<SdlAudioSpec>(), 32);
        assert_eq!(offset_of!(SdlAudioSpec, freq), 0);
        assert_eq!(offset_of!(SdlAudioSpec, format), 4);
        assert_eq!(offset_of!(SdlAudioSpec, channels), 6);
        assert_eq!(offset_of!(SdlAudioSpec, silence), 7);
        assert_eq!(offset_of!(SdlAudioSpec, samples), 8);
        assert_eq!(offset_of!(SdlAudioSpec, padding), 10);
        assert_eq!(offset_of!(SdlAudioSpec, size), 12);
        assert_eq!(offset_of!(SdlAudioSpec, callback), 16);
        assert_eq!(offset_of!(SdlAudioSpec, userdata), 24);
    }

    #[test]
    fn rgba_upload_rejects_short_empty_and_overflowing_buffers() {
        assert!(super::valid_rgba(2, 3, 24));
        assert!(!super::valid_rgba(2, 3, 23));
        assert!(!super::valid_rgba(0, 3, 0));
        assert!(!super::valid_rgba(u32::MAX, u32::MAX, 0));
        assert!(!super::valid_rgba(i32::MAX as u32, 1, i32::MAX as usize * 4));
    }

    #[test]
    fn library_candidates_match_the_platform() {
        #[cfg(target_os = "macos")]
        {
            assert!(SDL_LIBRARIES.contains(&"libSDL2.dylib"));
            assert!(SDL_LIBRARIES.contains(&"/opt/homebrew/opt/sdl2/lib/libSDL2.dylib"));
            assert!(SDL_LIBRARIES.contains(&"/usr/local/opt/sdl2/lib/libSDL2.dylib"));
            assert!(SDL_LIBRARIES.contains(&"/Library/Frameworks/SDL2.framework/SDL2"));
            assert!(SDL_LIBRARIES.iter().all(|s| !s.contains(".so")));
        }
        #[cfg(target_os = "linux")]
        assert_eq!(SDL_LIBRARIES, &[
            "libSDL2-2.0.so.0", "libSDL2.so.0", "libSDL2.so", "libSDL2-2.0.so",
        ]);
    }

    #[test]
    fn audio_default_is_platform_appropriate() {
        #[cfg(target_os = "linux")]
        assert_eq!(default_audio_driver(), Some("pulse"));
        #[cfg(not(target_os = "linux"))]
        assert_eq!(default_audio_driver(), None);
    }

    #[test]
    fn game_cursor_has_three_visible_states() {
        let normal = cursor_bitmap(0);
        let hover = cursor_bitmap(1);
        let pressed = cursor_bitmap(2);
        assert!(normal.1.iter().any(|&b| b != 0));
        assert!(hover.1.iter().any(|&b| b != 0));
        assert!(pressed.1.iter().any(|&b| b != 0));
        assert_ne!(normal, hover);
        assert_ne!(normal, pressed);
    }
}
