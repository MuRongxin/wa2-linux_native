//! The save / load screen (「セーブ」/「ロード」).
//!
//! Like the option screen everything is drawn from the shipped layers, which
//! are pixel aligned with each other:
//!
//! ```text
//!   sys_05000        top banner (1280x36)
//!   sys_05900        one slot panel, 272x160, normal
//!   sys_05901        the same panel lit (the slot under the cursor)
//!   sys_05903        the empty slot (fully transparent)
//!   sys_05100        gold frame drawn over the slot under the cursor
//!   sys_05110        drop shadow, drawn under a panel
//!   sys_05021        footer widget, 288x40
//! ```
//!
//! The screen is a paged grid of twelve 272x160 cells, which is what fits on
//! 1280x720; the original addresses up to 100 slots (`save_%02d.sav`), so
//! there are nine pages.

use crate::codec::tga::TgaImage;
use crate::render::Surface;
use crate::save::{self, SaveState, PER_PAGE, SLOT_COUNT};
use crate::sdl2::{self, Sdl};
use std::path::Path;

const W: u32 = 1280;
const H: u32 = 720;

const CELL_W: i32 = 272;
const CELL_H: i32 = 160;
const GAP: i32 = 16;
const COLS: i32 = 4;
const ROWS: i32 = 3;
const GRID_W: i32 = COLS * CELL_W + (COLS - 1) * GAP;
const GRID_X: i32 = (W as i32 - GRID_W) / 2;
const GRID_Y: i32 = 60;

/// Usable area inside the gold frame (sys_05100's border is ~8 px).
const FRAME_X: i32 = 8;
const FRAME_Y: i32 = 8;
const FRAME_W: i32 = 259;
const FRAME_H: i32 = 136;
/// The screen is 16:9, the frame is not: a full frame is letterboxed instead
/// of stretched, so 242x136 with a small pillar on each side.
const THUMB_W: i32 = 242;
const THUMB_H: i32 = FRAME_H;

const FOOTER_X: i32 = (W as i32 - 288) / 2;
const FOOTER_Y: i32 = H as i32 - 56;
/// Caption strip over the bottom of the thumbnail.
const CAP_H: i32 = 22;
const HINT_W: i32 = 420;
const HINT_Y: i32 = 584;

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Mode {
    Save,
    Load,
}

struct Assets {
    header: Option<TgaImage>,
    panel: Option<TgaImage>,
    panel_hi: Option<TgaImage>,
    frame: Option<TgaImage>,
    shadow: Option<TgaImage>,
    footer: Option<TgaImage>,
}

fn load(dirs: &[&Path], name: &str) -> Option<TgaImage> {
    for dir in dirs {
        for cand in crate::codec::image::candidate_names(name) {
            if let Ok(img) = crate::codec::image::decode_ui_file(&dir.join(cand)) {
                return Some(img);
            }
        }
    }
    eprintln!("saveload: {} NOT FOUND", name);
    None
}

impl Assets {
    fn load(dirs: &[&Path]) -> Assets {
        Assets {
            header: load(dirs, "sys_05000.tga"),
            panel: load(dirs, "sys_05900.tga"),
            panel_hi: load(dirs, "sys_05901.tga"),
            frame: load(dirs, "sys_05100.tga"),
            shadow: load(dirs, "sys_05110.tga"),
            footer: load(dirs, "sys_05021.tga"),
        }
    }
}

struct Screen {
    page: usize,
    cursor: usize,
}

fn cell_pos(i: usize) -> (i32, i32) {
    let c = (i as i32) % COLS;
    let r = (i as i32) / COLS;
    (GRID_X + c * (CELL_W + GAP), GRID_Y + r * (CELL_H + GAP))
}

/// Which cell, if any, a screen position falls on.
fn cell_at(sx: i32, sy: i32, page: usize) -> Option<usize> {
    for i in 0..save::cells_on_page(page) {
        let (x, y) = cell_pos(i);
        if sx >= x && sx < x + CELL_W && sy >= y && sy < y + CELL_H {
            return Some(i);
        }
    }
    None
}

fn caption_of(i: usize, st: Option<&SaveState>) -> String {
    match st {
        Some(s) => format!("{:02} {} {}", i, s.time, s.caption()),
        None => format!("{:02} -", i),
    }
}

/// Write one slot: the state file plus the thumbnail shown on the grid.
fn write_slot(i: usize, st: &SaveState, frame: &Surface) -> std::io::Result<()> {
    st.save_to(&save::slot_path(i))?;
    let t = frame.thumbnail(THUMB_W as u32, THUMB_H as u32);
    let path = save::thumb_path(i);
    if let Some(p) = path.parent() {
        std::fs::create_dir_all(p)?;
    }
    let mut out = Surface::new(t.width, t.height, [0, 0, 0, 255]);
    out.blit(&t, 0, 0);
    out.save_tga(&path.to_string_lossy())
}

/// Run the screen. Returns the state to resume, i.e. `Some` only when the
/// player loaded a slot.
///
/// `frame` is the screen as it looked when the screen was opened: it is
/// dimmed behind the grid and used as the thumbnail when saving.
pub fn run(
    win: &mut Sdl,
    dirs: &[&Path],
    mode: Mode,
    snap: Option<SaveState>,
    frame: Option<&Surface>,
) -> Option<SaveState> {
    let assets = Assets::load(dirs);
    let mut mode = mode;
    if mode == Mode::Save && (snap.is_none() || frame.is_none()) {
        mode = Mode::Load;
    }
    let pages = save::page_count();
    let mut slots: Vec<Option<SaveState>> = save::list();
    let mut thumbs: Vec<Option<TgaImage>> = (0..SLOT_COUNT)
        .map(|i| {
            std::fs::read(save::thumb_path(i)).ok().and_then(|d| crate::codec::tga::decode(&d).ok())
        })
        .collect();
    let mut scr = Screen { page: 0, cursor: 0 };
    // Captions go through the system font, so they are rasterized once per
    // page instead of once per frame.
    let mut caps: Option<(u32, u32, Vec<u8>)> = None;
    let mut hint: Option<(u32, u32, Vec<u8>)> = None;
    let mut caps_key = (usize::MAX, Mode::Save);

    loop {
        let key = (scr.page, mode);
        if key != caps_key {
            let (c, h) = render_captions(&scr, &slots, mode, pages);
            caps = c;
            hint = h;
            caps_key = key;
        }
        let n = save::cells_on_page(scr.page);
        let page_slots: Vec<Option<&SaveState>> = (0..n)
            .map(|i| slots[scr.page * PER_PAGE + i].as_ref())
            .collect();
        let mut s = match frame {
            Some(f) => Surface { width: f.width, height: f.height, pixels: f.pixels.clone() },
            None => Surface::new(W, H, [0, 0, 0, 255]),
        };
        compose(&mut s, &assets, &scr, &page_slots, &thumbs[scr.page * PER_PAGE..], caps.as_ref(), hint.as_ref());
        win.present(&s);

        let mut activate: Option<usize> = None;
        match win.poll_input() {
            sdl2::Input::Quit => return None,
            sdl2::Input::Back => return None,
            sdl2::Input::RightClick => return None,
            // 1 / 2 switch between saving and loading; the shipped banner only
            // carries the Japanese word for whichever mode was opened.
            sdl2::Input::Choice(1) if snap.is_some() && frame.is_some() => mode = Mode::Save,
            sdl2::Input::Choice(2) => mode = Mode::Load,
            sdl2::Input::Up => {
                if scr.cursor == 0 {
                    scr.page = (scr.page + pages - 1) % pages;
                    scr.cursor = save::cells_on_page(scr.page).saturating_sub(1);
                } else {
                    scr.cursor -= 1;
                }
            }
            sdl2::Input::Down => {
                if scr.cursor + 1 >= save::cells_on_page(scr.page) {
                    scr.page = (scr.page + 1) % pages;
                    scr.cursor = 0;
                } else {
                    scr.cursor += 1;
                }
            }
            sdl2::Input::Advance => activate = Some(scr.cursor),
            sdl2::Input::Pointer { x, y, pressed } => {
                let (sx, sy) = win.surface_pos(x, y, W, H);
                if let Some(i) = cell_at(sx, sy, scr.page) {
                    scr.cursor = i;
                    if pressed {
                        activate = Some(i);
                    }
                }
            }
            _ => {}
        }

        if let Some(i) = activate {
            let Some(idx) = save::slot_index(scr.page, i) else {
                continue;
            };
            match mode {
                Mode::Save => {
                    if let (Some(st), Some(f)) = (snap.as_ref(), frame) {
                        match write_slot(idx, st, f) {
                            Ok(()) => {
                                eprintln!("save: slot {} <- scene {} pc {}", idx, st.scene, st.vm.pc);
                                slots[idx] = SaveState::load_from(&save::slot_path(idx));
                                thumbs[idx] = std::fs::read(save::thumb_path(idx))
                                    .ok()
                                    .and_then(|d| crate::codec::tga::decode(&d).ok());
                                caps_key = (usize::MAX, Mode::Save);
                            }
                            Err(e) => eprintln!("save: slot {}: {}", idx, e),
                        }
                    }
                }
                Mode::Load => {
                    if let Some(st) = SaveState::load_from(&save::slot_path(idx)) {
                        return Some(st);
                    }
                }
            }
        }
        win.delay(16);
    }
}

/// One system-font line per slot, plus a key hint under the grid.
fn render_captions(
    scr: &Screen,
    slots: &[Option<SaveState>],
    mode: Mode,
    pages: usize,
) -> (
    Option<(u32, u32, Vec<u8>)>,
    Option<(u32, u32, Vec<u8>)>,
) {
    let n = save::cells_on_page(scr.page);
    let lines: Vec<String> = (0..n)
        .map(|i| caption_of(scr.page * PER_PAGE + i, slots[scr.page * PER_PAGE + i].as_ref()))
        .collect();
    let caps = crate::render::text::with_ui_font(14, |f| {
        f.block_rgba(&lines, FRAME_W as u32, (n as u32) * CAP_H as u32, 14)
    })
    .flatten();
    let hint_text = format!(
        "[1]保存 [2]读档   现在:[{}]   [Enter]确定  [Esc]返回   {}/{} 页",
        if mode == Mode::Save { "保存" } else { "读档" },
        scr.page + 1,
        pages
    );
    let hint = crate::render::text::with_ui_font(14, |f| {
        f.block_rgba(&[hint_text], HINT_W as u32, CAP_H as u32, 14)
    })
    .flatten();
    (caps, hint)
}
fn compose(
    s: &mut Surface,
    a: &Assets,
    scr: &Screen,
    slots: &[Option<&SaveState>],
    thumbs: &[Option<TgaImage>],
    caps: Option<&(u32, u32, Vec<u8>)>,
    hint: Option<&(u32, u32, Vec<u8>)>,
) {
    s.fill_rect(0, 0, W, H, [0, 0, 0, 190]);
    if let Some(h) = a.header.as_ref() {
        s.blit(h, 0, 0);
    }
    for i in 0..slots.len() {
        let (cx, cy) = cell_pos(i);
        if let Some(sh) = a.shadow.as_ref() {
            s.blit(sh, cx, cy);
        }
        // Every slot keeps its panel even when empty: `sys_05903` is fully
        // transparent, so an empty cell would otherwise be invisible and there
        // would be nothing to aim at.
        let panel = if scr.cursor == i { a.panel_hi.as_ref() } else { a.panel.as_ref() };
        if let Some(p) = panel {
            s.blit(p, cx, cy);
        }
        // Thumbnail, letterboxed inside the frame.
        if let Some(t) = thumbs.get(i).and_then(|t| t.as_ref()) {
            let x = cx + FRAME_X + (FRAME_W - t.width as i32) / 2;
            let y = cy + FRAME_Y + (FRAME_H - t.height as i32) / 2;
            s.blit(t, x, y);
        }
        // Caption over the bottom of the thumbnail.
        if let Some((cw, _, rgba)) = caps {
            let sy = (i as u32) * CAP_H as u32;
            let mut strip = Vec::with_capacity((*cw as usize) * CAP_H as usize * 4);
            for row in sy..sy + CAP_H as u32 {
                let off = (row * *cw * 4) as usize;
                strip.extend_from_slice(&rgba[off..off + (*cw as usize) * 4]);
            }
            s.fill_rect(cx + FRAME_X, cy + FRAME_Y + FRAME_H - CAP_H, FRAME_W as u32, CAP_H as u32, [0, 0, 0, 170]);
            s.blit_rgba(&strip, *cw, CAP_H as u32, cx + FRAME_X, cy + FRAME_Y + FRAME_H - CAP_H);
        }
        if scr.cursor == i {
            if let Some(f) = a.frame.as_ref() {
                s.blit(f, cx, cy);
            }
        }
    }
    if let Some((hw, _, rgba)) = hint {
        let x = (W as i32 - HINT_W) / 2;
        s.fill_rect(x - 10, HINT_Y - 2, (HINT_W + 20) as u32, (CAP_H + 4) as u32, [0, 0, 0, 170]);
        s.blit_rgba(rgba, *hw, CAP_H as u32, x, HINT_Y);
    }
    if let Some(f) = a.footer.as_ref() {
        s.blit(f, FOOTER_X, FOOTER_Y);
    }
}
