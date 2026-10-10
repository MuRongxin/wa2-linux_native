//! Minimal SDL2 frontend via runtime dynamic loading (dlopen/dlsym).
//!
//! The environment has the SDL2 runtime (`libSDL2-2.0.so.0`) but no dev
//! headers / `sdl2.pc`, and crates.io is unreachable. So instead of a build-time
//! dependency we load SDL2 at runtime and call only the handful of functions we
//! need to blit our offscreen `Surface` to a real window. If SDL2 is missing the
//! app degrades gracefully (`Window::new` returns `None`).

use crate::render::Surface;
use std::ffi::CString;
use std::os::raw::{c_char, c_int, c_void};
use std::sync::OnceLock;

extern "C" {
    fn dlopen(filename: *const c_char, flag: c_int) -> *mut c_void;
    fn dlsym(handle: *mut c_void, symbol: *const c_char) -> *mut c_void;
}

const RTLD_NOW: c_int = 2;

// SDL constants
const SDL_INIT_VIDEO: u32 = 0x0000_0020;
const SDL_WINDOW_SHOWN: u32 = 0x0000_0004;
const SDL_WINDOW_RESIZABLE: u32 = 0x0000_0020;
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
const SDLK_ESCAPE: i32 = 41;
const SDLK_q: i32 = 20;
const SDLK_SPACE: i32 = 44;
const SDLK_RETURN: i32 = 40;
const SDLK_RIGHT: i32 = 79;
const SDLK_DOWN: i32 = 81;
const SDLK_LEFT: i32 = 80;
const SDLK_UP: i32 = 82;
const SDLK_F11: i32 = 95;
const SDLK_1: i32 = 30;
const SDLK_2: i32 = 31;
const SDLK_3: i32 = 32;
const SDLK_4: i32 = 33;
const SDLK_5: i32 = 34;
const SDLK_6: i32 = 35;
const SDLK_7: i32 = 36;
const SDLK_8: i32 = 37;
const SDLK_9: i32 = 38;

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
    f_poll_event: unsafe extern "C" fn(*mut c_void) -> c_int,
    f_destroy_texture: unsafe extern "C" fn(*mut c_void),
    f_destroy_renderer: unsafe extern "C" fn(*mut c_void),
    f_destroy_window: unsafe extern "C" fn(*mut c_void),
    f_delay: unsafe extern "C" fn(u32),
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Input {
    None,
    Advance,
    Back,
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
}

unsafe fn sym(lib: *mut c_void, name: &str) -> Option<*mut c_void> {
    let c = CString::new(name).ok()?;
    let p = dlsym(lib, c.as_ptr());
    if p.is_null() { None } else { Some(p) }
}

impl Sdl {
    /// Try to open a window + streaming texture. Returns None if SDL2 is
    /// unavailable or window creation fails (e.g. no display).
    pub fn new(title: &str, tw: u32, th: u32) -> Option<Sdl> {
        unsafe {
            for cand in ["libSDL2-2.0.so.0", "libSDL2.so.0", "libSDL2.so", "libSDL2-2.0.so"] {
                let c = CString::new(cand).unwrap();
                let lib = dlopen(c.as_ptr(), RTLD_NOW);
                if !lib.is_null() {
                    return Sdl::init_from(lib, title, tw, th);
                }
            }
            None
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
        let f_open_audio: unsafe extern "C" fn(*const c_char, i32, u32, i32, i32, i32, i32) -> u32 = std::mem::transmute(sym(lib, "SDL_OpenAudioDevice")?);
        let f_queue_audio: unsafe extern "C" fn(u32, *const c_void, u32) -> u32 = std::mem::transmute(sym(lib, "SDL_QueueAudio")?);
        let f_poll_event: unsafe extern "C" fn(*mut c_void) -> c_int = std::mem::transmute(sym(lib, "SDL_PollEvent")?);
        let f_destroy_texture: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(sym(lib, "SDL_DestroyTexture")?);
        let f_destroy_renderer: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(sym(lib, "SDL_DestroyRenderer")?);
        let f_destroy_window: unsafe extern "C" fn(*mut c_void) = std::mem::transmute(sym(lib, "SDL_DestroyWindow")?);
        let f_delay: unsafe extern "C" fn(u32) = std::mem::transmute(sym(lib, "SDL_Delay")?);

        // Choose the pulse audio driver BEFORE any SDL_Init (PipeWire system):
        // SDL picks the driver at init time from SDL_AUDIODRIVER.
        if std::env::var_os("SDL_AUDIODRIVER").is_none() {
            std::env::set_var("SDL_AUDIODRIVER", "pulse");
        }
        let init: unsafe extern "C" fn(u32) -> c_int = std::mem::transmute(s_init);
        // init video+audio together; if that fails, fall back to video only
        let r = init(SDL_INIT_VIDEO | SDL_INIT_AUDIO);
        if r != 0 { let _ = init(SDL_INIT_VIDEO); }
        let t = CString::new(title).ok()?;
        let window = f_create_window(
            t.as_ptr(),
            SDL_WINDOWPOS_CENTERED,
            SDL_WINDOWPOS_CENTERED,
            tw as c_int,
            th as c_int,
            SDL_WINDOW_SHOWN | SDL_WINDOW_RESIZABLE,
        );
        if window.is_null() {
            return None;
        }
        // Prefer the software renderer: it blits via CPU into an SHM surface,
        // which works under X11/Wayland uniformly and is capturable. Fall back
        // to accelerated if software is unavailable.
        let mut renderer = f_create_renderer(window, -1, SDL_RENDERER_SOFTWARE);
        if renderer.is_null() {
            renderer = f_create_renderer(
                window,
                -1,
                SDL_RENDERER_ACCELERATED | SDL_RENDERER_PRESENTVSYNC,
            );
        }
        if renderer.is_null() {
            return None;
        }
        let texture = f_create_texture(renderer, SDL_PIXELFORMAT_RGBA32, SDL_TEXTUREACCESS_STATIC, tw as c_int, th as c_int);
        if texture.is_null() {
            return None;
        }
        Some(Sdl {
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
            f_poll_event,
            f_destroy_texture,
            f_destroy_renderer,
            f_destroy_window,
            f_delay,
        })
    }

    /// Upload `surf` pixels to the texture and present, scaled to the window.
    pub fn present(&mut self, surf: &Surface) {
        unsafe {
            if surf.width != self.tex_w || surf.height != self.tex_h {
                (self.f_destroy_texture)(self.texture);
                let t = (self.f_create_texture)(
                    self.renderer,
                    SDL_PIXELFORMAT_RGBA32,
                    SDL_TEXTUREACCESS_STATIC,
                    surf.width as c_int,
                    surf.height as c_int,
                );
                if t.is_null() {
                    return;
                }
                self.texture = t;
                self.tex_w = surf.width;
                self.tex_h = surf.height;
            }
            let pitch = (surf.width * 4) as c_int;
            (self.f_update_texture)(self.texture, std::ptr::null(), surf.pixels.as_ptr() as *const c_void, pitch);
            (self.f_render_clear)(self.renderer);
            (self.f_render_copy)(self.renderer, self.texture, std::ptr::null(), std::ptr::null());
            (self.f_render_present)(self.renderer);
        }
    }

    /// Upload a raw RGBA buffer (w*h*4 bytes) to the texture and present,
    /// stretched to the window. Used for video frames.
    pub fn present_raw(&mut self, rgba: &[u8], w: u32, h: u32) {
        unsafe {
            if w != self.tex_w || h != self.tex_h {
                (self.f_destroy_texture)(self.texture);
                let t = (self.f_create_texture)(self.renderer, SDL_PIXELFORMAT_RGBA32, SDL_TEXTUREACCESS_STATIC, w as c_int, h as c_int);
                if t.is_null() { return; }
                self.texture = t;
                self.tex_w = w;
                self.tex_h = h;
            }
            (self.f_update_texture)(self.texture, std::ptr::null(), rgba.as_ptr() as *const c_void, (w * 4) as c_int);
            (self.f_render_clear)(self.renderer);
            (self.f_render_copy)(self.renderer, self.texture, std::ptr::null(), std::ptr::null());
            (self.f_render_present)(self.renderer);
        }
    }

    /// Drain SDL events; return the first actionable input (advance/back/quit).
    pub fn poll_input(&mut self) -> Input {
        let mut ev = [0u8; 128];
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
                    match sc {
                        SDLK_ESCAPE | SDLK_q => out = Input::Quit,
                        SDLK_F11 => out = Input::Fullscreen,
                        SDLK_LEFT => out = Input::Back,
                        SDLK_UP => out = Input::Up,
                        SDLK_SPACE | SDLK_RETURN | SDLK_RIGHT => out = Input::Advance,
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
                        _ => {}
                    }
                } else if etype == SDL_MOUSEMOTION {
                    // SDL_MouseMotionEvent: type,timestamp,windowID,which,state
                    // (20 bytes) then Sint32 x, y.
                    let x = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                    let y = i32::from_ne_bytes([ev[24], ev[25], ev[26], ev[27]]);
                    out = Input::Pointer { x, y, pressed: false };
                } else if etype == SDL_MOUSEBUTTONDOWN {
                    // SDL_MouseButtonEvent: type..which (16), button,state,clicks,
                    // padding (4), then Sint32 x, y.
                    let x = i32::from_ne_bytes([ev[20], ev[21], ev[22], ev[23]]);
                    let y = i32::from_ne_bytes([ev[24], ev[25], ev[26], ev[27]]);
                    out = Input::Pointer { x, y, pressed: true };
                }
            }
        }
        if out != Input::None && std::env::var_os("WA2_DEBUG_INPUT").is_some() {
            eprintln!("input: {:?}", out);
        }
        out
    }

    /// Current window size in pixels (the surface is stretched to it).
    pub fn window_size(&self) -> (i32, i32) {
        let (mut w, mut h) = (0i32, 0i32);
        unsafe { (self.f_get_window_size)(self.window, &mut w, &mut h); }
        (w.max(1), h.max(1))
    }

    /// Map a pointer event's window coordinates onto the game surface
    /// (`present` stretches the surface over the whole window, so this is a
    /// plain linear scale).
    pub fn surface_pos(&self, x: i32, y: i32, sw: u32, sh: u32) -> (i32, i32) {
        let (ww, wh) = self.window_size();
        (x * sw as i32 / ww, y * sh as i32 / wh)
    }

    /// Read back the current renderer target as RGBA (for verification),
    /// independent of the windowing/compositor path.
    pub fn read_back(&self, w: u32, h: u32) -> Vec<u8> {
        let mut buf = vec![0u8; (w * h * 4) as usize];
        let pitch = (w * 4) as c_int;
        unsafe {
            (self.f_render_read_pixels)(
                self.renderer,
                std::ptr::null(),
                SDL_PIXELFORMAT_RGBA32,
                buf.as_mut_ptr() as *mut c_void,
                pitch,
            );
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
            (self.f_destroy_texture)(self.texture);
            (self.f_destroy_renderer)(self.renderer);
            (self.f_destroy_window)(self.window);
        }
    }
}
// ---- Audio device (global fn table so the mixer thread can call SDL) ----
const SDL_AUDIOSIZE_SAMPLES: i32 = 1024;
const AUDIO_S16SYS: u32 = 0x8010;
const SDL_INIT_AUDIO: u32 = 0x0000_0010;

struct AudioFns {
    initsub: unsafe extern "C" fn(u32) -> c_int,
    open: unsafe extern "C" fn(*const c_char, i32, i32, i32, *const c_void) -> u32,
    queue: unsafe extern "C" fn(u32, *const c_void, u32) -> u32,
    pause: unsafe extern "C" fn(u32, i32),
}

static AUDIO: OnceLock<AudioFns> = OnceLock::new();

/// Open a playback device (48kHz stereo, 1024-frame buffer). 0 on failure.
pub fn sdl_open_audio(rate: u32, ch: u16) -> u32 {
    let f = match AUDIO.get() { Some(f) => f, None => return 0 };
    unsafe { (f.initsub)(SDL_INIT_AUDIO); } // initialise the audio subsystem
    unsafe {
        (f.open)(
            std::ptr::null(),   // device
            0,                   // is32bit (unused, must be 0)
            rate as i32,// freq in Hz
            0,                   // allowed_changes
            std::ptr::null(),   // devid
        )
    }
}

/// Push interleaved s16le bytes to the audio device.
pub fn sdl_queue_audio(dev: u32, bytes: &[u8]) {
    if let Some(f) = AUDIO.get() {
        unsafe { (f.queue)(dev, bytes.as_ptr() as *const c_void, bytes.len() as u32); }
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
        let lib = dlopen(CString::new("libSDL2-2.0.so.0").unwrap().as_ptr(), RTLD_NOW);
        if lib.is_null() { return "?".into(); }
        let p = dlsym(lib, CString::new("SDL_GetCurrentAudioDriver").unwrap().as_ptr());
        if p.is_null() { return "?".into(); }
        let f: unsafe extern "C" fn() -> *const c_char = std::mem::transmute(p);
        let n = f();
        if n.is_null() { "none".into() } else { std::ffi::CStr::from_ptr(n).to_string_lossy().into_owned() }
    }
}

/// SDL_GetError() string (diagnostic).
pub fn sdl_get_error() -> String {
    unsafe {
        let lib = dlopen(CString::new("libSDL2-2.0.so.0").unwrap().as_ptr(), RTLD_NOW);
        if lib.is_null() { return "?".into(); }
        let p = dlsym(lib, CString::new("SDL_GetError").unwrap().as_ptr());
        if p.is_null() { return "?".into(); }
        let f: unsafe extern "C" fn() -> *const c_char = std::mem::transmute(p);
        let e = f();
        if e.is_null() { "".into() } else { std::ffi::CStr::from_ptr(e).to_string_lossy().into_owned() }
    }
}
