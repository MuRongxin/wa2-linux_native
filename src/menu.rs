//! The game's own option screen (「系统设置」/「オプション」).
//!
//! Everything on screen comes from the shipped layers, which are pixel
//! aligned with each other:
//!
//! ```text
//!   sys_03100/03101/03102   page base: grid, labels and silver widgets
//!   sys_03200/03210/03220   the same widgets in gold (a setting's current value)
//!   sys_03201/03211/03221   the same widgets in cyan (the widget under the cursor)
//!   sys_03000               top banner (1280x36)
//!   sys_03011               page tabs, 3 tabs x 3 states (192x32 each)
//!   sys_03040/03041/03042   one help line per setting row, 24 px each
//!   sys_03020               bottom bar: 3 arrow states plus the label strip
//!   sys_03030               "restore defaults" button, 3 states of 208x24
//! ```
//!
//! Widget rectangles are *not* taken from a table: they were recovered from
//! the images themselves (connected components of the widget border colour),
//! so no exe coordinates are needed. The Chinese patch ships its own copies of
//! 0310X / 0320X / 0321X / 0304X / 03020 / 03030, and `bg_dirs()` looks in the
//! CN directory first, so Chinese mode needs no extra work.
//!
//! Which overlay a widget gets is inferred: every widget is drawn from the
//! base layer, the option a setting currently holds comes from the gold layer,
//! and the widget under the cursor comes from the cyan layer.

use crate::codec::tga::TgaImage;
use crate::render::Surface;
use crate::sdl2::{self, Sdl};
use std::path::{Path, PathBuf};

const W: u32 = 1280;
const H: u32 = 720;

const PAGE_W: i32 = 1024;
const PAGE_H: i32 = 464;
const PAGE_X: i32 = (W as i32 - PAGE_W) / 2;
const PAGE_Y: i32 = 84;

const HEADER_H: i32 = 36;
const TAB_W: i32 = 192;
const TAB_H: i32 = 32;
const TABS_Y: i32 = 42;
const TABS_X: i32 = (W as i32 - TAB_W * 3) / 2;

const HELP_W: i32 = 944;
const HELP_H: i32 = 24;
const HELP_X: i32 = (W as i32 - HELP_W) / 2;
/// The guide-message bar is baked into the bottom of the page art (page rows
/// 438..458) and the strip text is pre-centred for it (text centre at strip
/// x 471 = bar centre), so the line belongs inside the bar, not below the page.
const HELP_Y: i32 = PAGE_Y + 435;
/// The 「向导信息 GUIDE MESSAGE」 label baked into the bar is covered while a
/// help line is showing, so long lines never run over it.
const GUIDE_X: i32 = PAGE_X + 36;
const GUIDE_Y: i32 = PAGE_Y + 440;
const GUIDE_W: i32 = 956;
const GUIDE_H: i32 = 19;

/// `sys_03020` is 432×128. The top 64 rows are three left-arrow states (48 px
/// apart) and then the label strip from x = 144. The bottom 64 rows are the
/// three right-arrow states. Drawing the whole top row piles those three states
/// on top of each other; the screen wants one arrow on each side of the strip.
const DEF_W: i32 = 208;
const DEF_H: i32 = 24;
/// 恢复默认 sits on its own. The page navigation that shared the bar with it
/// is gone, and the page grew into the space that freed up - see
/// [`FONT_BLOCK_H`].
const DEF_X: i32 = W as i32 - 128 - DEF_W;
const DEF_Y: i32 = 664;

/// Height of the font block at the top of page 1. The page artwork is a black
/// plate with no visible frame, so the rows the artwork laid out can simply be
/// blitted this much lower and the gap filled with the font rows.
const FONT_BLOCK_H: i32 = 108;
/// Where the artwork is cut: everything above stays put, everything below
/// moves down by [`FONT_BLOCK_H`]. Zero because the strip the artwork puts at
/// the top of the page is a divider, and the font rows own the top now.
const FONT_SPLIT: i32 = 0;

/// The three font rows, on the grid the rest of the page uses: the label
/// column ends at 448 and the controls run from 458 to 966.
const FROW_H: i32 = 30;
const FROW_PITCH: i32 = 36;
const FROW_Y0: i32 = 4;
const FROW_LABEL_X: i32 = 48;
const FROW_LABEL_W: i32 = 400;
const FROW_BOX_X: i32 = 458;
const FROW_BOX_W: i32 = 300;
const FROW_ARROW_L: i32 = 766;
const FROW_ARROW_R: i32 = 802;
const FROW_ARROW_W: i32 = 30;
const FROW_STYLE_X: i32 = 846;
const FROW_STYLE_W: i32 = 120;

/// Screen y of a widget that the artwork placed at page y.
fn page_y(page: usize, y: i32) -> i32 {
    PAGE_Y + y + page_shift(page)
}

/// The help line follows the page down, or it would sit in the middle of it.
fn help_y(page: usize) -> i32 {
    HELP_Y + page_shift(page)
}

fn guide_y(page: usize) -> i32 {
    GUIDE_Y + page_shift(page)
}

/// Geometry of a segmented slider bar, identical on every row: 21 segments of
/// 14 px on a 16 px pitch, the first one starting at x = 545 of the page image.
/// The sprite itself (dark in `sys_0310X`, lit in `sys_0320X`) is 334 px wide
/// and 24 px tall, so both copies are pixel aligned.
const BAR_X0: i32 = 545;
const BAR_X1: i32 = 879;
const BAR_H: i32 = 24;
const BAR_SEGMENTS: i32 = 21;
const BAR_PITCH: i32 = 16;

// ---------------------------------------------------------------------------
// settings model
// ---------------------------------------------------------------------------

/// Slider levels. The bars in the artwork are drawn with 20-ish segments; the
/// scripts' own settings use 0..10, which is what the original config stores.
pub const LEVELS: i32 = 10;

/// How long auto mode holds one line, in milliseconds.
///
/// `speed` is the 「自动模式速度」 slider: 0 is the fast end, [`LEVELS`] the
/// slow end. A longer line is held longer. Voiced lines are held at least
/// this long, and longer still while the voice is playing — that wait lives
/// in the game loop, which also checks the mixer.
pub fn auto_hold_ms(speed: i32, chars: usize) -> u32 {
    let speed = speed.clamp(0, LEVELS) as u32;
    let per_char = 30 + speed * 9;
    let floor = 200 + speed * 60;
    floor + (chars as u32).saturating_mul(per_char)
}

#[derive(Clone, PartialEq, Eq, Debug)]
pub struct Settings {
    /// 0 = windowed, 1 = fullscreen.
    pub win_disp: i32,
    /// 0 instant, 1 fast, 2 normal, 3 smooth.
    pub text_speed: i32,
    /// 0 = skip everything, 1 = skip only already-read text.
    pub text_skip: i32,
    /// 0..=LEVELS, 0 = FAST, LEVELS = SLOW. See [`auto_hold_ms`].
    pub auto_speed: i32,
    /// 0 instant, 1 normal.
    pub effect: i32,
    /// 0..=LEVELS opacity of the message window (normal scene).
    pub alpha_novel: i32,
    /// 0..=LEVELS opacity of the message window (CG scene).
    pub alpha_cg: i32,
    /// 0..=LEVELS opacity of the black backdrop in novel scenes.
    pub alpha_back: i32,
    /// 0..=LEVELS opacity of the command icons.
    pub alpha_command: i32,
    /// 0 = no yes/no confirmation, 1 = confirm.
    pub yes_no: i32,
    /// 0 hide window, 1 open the settings menu, 2 save.
    pub r_click: i32,
    /// all, bgm, se, voice gains (0..=LEVELS).
    pub vol: [i32; 4],
    /// 0 = let the voice finish, 1 = cut it when the page turns.
    pub page_voice: i32,
    /// 0 = play nothing, 1 = everything, 2 = female characters only.
    pub ero_voice: i32,
    /// Per character voice switch, one per row of page 3.
    pub char_voice: [i32; 10],
    /// Font catalogue ids for the three roles. Ids, never paths: the same
    /// value has to name the same font on another machine.
    pub font_home: i32,
    pub font_history: i32,
    pub font_game: i32,
    /// Dialogue body: size in 720p pixels, outline radius (0 follows the
    /// size), colour index, and whether lines carry a rule under them.
    pub dlg_size: i32,
    pub dlg_edge: i32,
    pub dlg_color: i32,
    pub dlg_under: i32,
}

impl Default for Settings {
    fn default() -> Settings {
        Settings {
            win_disp: 0,
            text_speed: 2,
            text_skip: 1,
            auto_speed: 5,
            effect: 1,
            alpha_novel: LEVELS,
            alpha_cg: LEVELS,
            // 「バックグラウンド不透明度（ノベルシーン時）」 defaults to 0 in
            // the original exe (WA2.exe reads win_color with default 0), i.e.
            // the background is not dimmed unless the player raises it.
            alpha_back: 0,
            alpha_command: LEVELS,
            yes_no: 1,
            r_click: 1,
            vol: [LEVELS, LEVELS, LEVELS, LEVELS],
            page_voice: 1,
            ero_voice: 2,
            char_voice: [1; 10],
            font_home: 0,
            font_history: 0,
            font_game: 0,
            dlg_size: crate::render::text::DialogStyle::default_style().size as i32,
            dlg_edge: 0,
            dlg_color: 0,
            dlg_under: 0,
        }
    }
}

/// The field a font key writes to, so `set` can range-check ids instead of
/// treating them as slider levels.
fn font_slot<'a>(st: &'a mut Settings, key: &str) -> Option<&'a mut i32> {
    match key {
        "font_home" => Some(&mut st.font_home),
        "font_history" => Some(&mut st.font_history),
        "font_game" => Some(&mut st.font_game),
        _ => None,
    }
}

/// The field a dialogue style key writes to.
fn dialog_slot<'a>(st: &'a mut Settings, key: &str) -> Option<&'a mut i32> {
    match key {
        "dlg_size" => Some(&mut st.dlg_size),
        "dlg_edge" => Some(&mut st.dlg_edge),
        "dlg_color" => Some(&mut st.dlg_color),
        "dlg_under" => Some(&mut st.dlg_under),
        _ => None,
    }
}

/// The dialogue style these settings describe, already ranged: the values
/// are stored as written and clamped here, so a hand-edited config still gives
/// a drawable size instead of a 1-pixel one.
pub fn dialog_style(st: &Settings) -> crate::render::text::DialogStyle {
    crate::render::text::DialogStyle {
        size: st.dlg_size.max(0) as u32,
        edge: st.dlg_edge.max(0) as u32,
        color: st.dlg_color.max(0) as u32,
        underline: st.dlg_under > 0,
    }
    .clamp()
}

/// `(ini key, getter-setter name)` pairs; keys are the ones the original exe
/// uses where they are known (`win_disp`, `wait`, `auto_max`, `win_alpha_*`,
/// `all_vol`, `page_voice`, `char_voice0..9`, ...).
impl Settings {
    fn get(&self, key: &str) -> Option<i32> {
        Some(match key {
            "win_disp" => self.win_disp,
            "wait" => self.text_speed,
            "msg_cut_optin" => self.text_skip,
            "auto_max" => self.auto_speed,
            "effect" => self.effect,
            "win_alpha_novel" => self.alpha_novel,
            "win_alpha_vis" => self.alpha_cg,
            "win_color" => self.alpha_back,
            "win_alpha_command" => self.alpha_command,
            "yes_no" => self.yes_no,
            "R_click" => self.r_click,
            "all_vol" => self.vol[0],
            "bgm_vol" => self.vol[1],
            "se_vol" => self.vol[2],
            "voice_vol" => self.vol[3],
            "page_voice" => self.page_voice,
            "ero_voice" => self.ero_voice,
            k if k.starts_with("char_voice") => {
                let i: usize = k["char_voice".len()..].parse().ok()?;
                *self.char_voice.get(i)?
            }
            "font_home" => self.font_home,
            "font_history" => self.font_history,
            "font_game" => self.font_game,
            "dlg_size" => self.dlg_size,
            "dlg_edge" => self.dlg_edge,
            "dlg_color" => self.dlg_color,
            "dlg_under" => self.dlg_under,
            _ => return None,
        })
    }

    fn set(&mut self, key: &str, v: i32) {
        // Font ids are hashes and the dialogue sizes are pixels: neither is a
        // slider level, so they are ranged by the layer that reads them
        // instead of by the generic clamp below.
        if let Some(slot) = font_slot(self, key) {
            *slot = v.max(0);
            return;
        }
        if let Some(slot) = dialog_slot(self, key) {
            *slot = v.max(0);
            return;
        }
        let v = v.clamp(0, LEVELS);
        match key {
            "win_disp" => self.win_disp = v.min(1),
            "wait" => self.text_speed = v.min(3),
            "msg_cut_optin" => self.text_skip = v.min(1),
            "auto_max" => self.auto_speed = v,
            "effect" => self.effect = v.min(1),
            "win_alpha_novel" => self.alpha_novel = v,
            "win_alpha_vis" => self.alpha_cg = v,
            "win_color" => self.alpha_back = v,
            "win_alpha_command" => self.alpha_command = v,
            "yes_no" => self.yes_no = v.min(1),
            "R_click" => self.r_click = v.min(2),
            "all_vol" => self.vol[0] = v,
            "bgm_vol" => self.vol[1] = v,
            "se_vol" => self.vol[2] = v,
            "voice_vol" => self.vol[3] = v,
            "page_voice" => self.page_voice = v.min(1),
            "ero_voice" => self.ero_voice = v.min(2),
            k if k.starts_with("char_voice") => {
                if let Ok(i) = k["char_voice".len()..].parse::<usize>() {
                    if i < self.char_voice.len() {
                        self.char_voice[i] = v.min(1);
                    }
                }
            }
            _ => {}
        }
    }

    fn all_keys() -> Vec<String> {
        let mut v = [
            "win_disp",
            "wait",
            "msg_cut_optin",
            "auto_max",
            "effect",
            "win_alpha_novel",
            "win_alpha_vis",
            "win_color",
            "win_alpha_command",
            "yes_no",
            "R_click",
            "all_vol",
            "bgm_vol",
            "se_vol",
            "voice_vol",
            "page_voice",
            "ero_voice",
            "font_home",
            "font_history",
            "font_game",
            "dlg_size",
            "dlg_edge",
            "dlg_color",
            "dlg_under",
        ]
        .iter()
        .map(|s| s.to_string())
        .collect::<Vec<_>>();
        for i in 0..10 {
            v.push(format!("char_voice{}", i));
        }
        v
    }

    /// `WA2_INI`, else `wa2.ini` next to the data directory.
    pub fn path() -> PathBuf {
        if let Some(p) = std::env::var_os("WA2_INI") {
            return PathBuf::from(p);
        }
        match crate::data::Data::resolve() {
            Some(d) => d.root.join("wa2.ini"),
            None => PathBuf::from("wa2.ini"),
        }
    }

    pub fn load() -> Settings {
        let mut st = Settings::default();
        let path = Settings::path();
        let text = match std::fs::read(&path) {
            Ok(d) => d,
            Err(_) => {
                eprintln!("settings: {} not found, using defaults", path.display());
                return st;
            }
        };
        let text = String::from_utf8_lossy(&text).into_owned();
        for line in text.lines() {
            let line = line.trim();
            if line.is_empty() || line.starts_with(';') || line.starts_with('#') || line.starts_with('[') {
                continue;
            }
            let (k, v) = match line.split_once('=') {
                Some(kv) => kv,
                None => continue,
            };
            if let Ok(n) = v.trim().parse::<i32>() {
                st.set(k.trim(), n);
            }
        }
        eprintln!("settings: loaded {}", path.display());
        // Fonts have to be in place before the first screen draws, not when
        // the option screen is opened.
        st.apply_text();
        st
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = Settings::path();
        let mut out = String::from("; WHITE ALBUM 2 native port\n; keys follow the original wa2.ini\n");
        for k in Settings::all_keys() {
            if let Some(v) = self.get(&k) {
                out.push_str(&format!("{}={}\n", k, v));
            }
        }
        // Write a sibling and rename it over the target: a process that dies
        // mid-write leaves the old file behind rather than a half-written one.
        let tmp = path.with_extension("ini.tmp");
        std::fs::write(&tmp, out)?;
        match std::fs::rename(&tmp, &path) {
            Ok(()) => Ok(()),
            Err(e) => {
                let _ = std::fs::remove_file(&tmp);
                Err(e)
            }
        }
    }

    /// Hand the three font ids and the dialogue style to the font layer.
    /// Every cache whose role changed is dropped there, so no glyph of the
    /// previous font survives into the new one.
    pub fn apply_text(&self) {
        let changed = crate::render::text::set_role_ids([self.font_home, self.font_history, self.font_game]);
        crate::render::text::set_dialog_style(dialog_style(self));
        if changed {
            eprintln!(
                "settings: fonts home={} history={} game={}",
                self.font_home, self.font_history, self.font_game
            );
        }
        // A saved id whose font is gone is a silent fallback otherwise: the
        // player would think the choice took effect.
        for role in crate::render::text::FONT_ROLES {
            if crate::render::text::role_fallback(role) {
                eprintln!(
                    "settings: {} font profile {} is unavailable here, using the default",
                    role.tag(),
                    crate::render::text::role_id(role)
                );
            }
        }
    }

    /// Linear gain for a mixer category, from the 0..=LEVELS slider value.
    pub fn gain(&self, cat: usize) -> f32 {
        let master = self.vol[0] as f32 / LEVELS as f32;
        let own = self.vol.get(cat).copied().unwrap_or(LEVELS) as f32 / LEVELS as f32;
        master * own
    }
}

// ---------------------------------------------------------------------------
// widget table
// ---------------------------------------------------------------------------

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Act {
    /// Set setting `key` to `val` (a radio button).
    Opt(&'static str, i32),
    /// Step a slider.
    Step(&'static str, i32),
    /// Click on the slider track: the value follows the pointer.
    Track,
    /// Jump to the slider's OFF position (0) - the 「窗口OFF」/「音量OFF」 button.
    Off(&'static str),
    /// Play a sample so the volume can be judged.
    Test,
    /// Restore every setting to its default.
    Reset,
    /// Switch to a page.
    Tab(usize),
    /// Page navigation.
    Page(i32),
    /// Open the dialogue style panel, from the font row.
    FontStyle,
    /// Step row "i"'s font by "dir" (-1 back, 1 forward).
    FontStep(usize, i32),
    /// Leave the option screen.
    Close,
}

#[derive(Clone, Copy)]
pub struct Hot {
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub act: Act,
    /// Which help line of `sys_0304X` to show while hovering this widget.
    pub row: usize,
    /// Slider this widget belongs to, for drawing the value indicator.
    pub slider: Option<&'static Slider>,
}

#[derive(Clone, Copy)]
pub struct Slider {
    /// Left edge of the first segment, in page coordinates.
    pub x0: i32,
    /// Right edge of the last segment.
    pub x1: i32,
    /// Top row of the bar.
    pub y: i32,
    pub key: &'static str,
}

const fn hot(x: i32, y: i32, w: i32, h: i32, act: Act, row: usize) -> Hot {
    Hot { x, y, w, h, act, row, slider: None }
}

const fn hot_sl(x: i32, y: i32, w: i32, h: i32, act: Act, row: usize, s: &'static Slider) -> Hot {
    Hot { x, y, w, h, act, row, slider: Some(s) }
}

// Page 1 - 画面设置. One help line per row (sys_03040 has 9).
const SL_AUTO: Slider = Slider { x0: BAR_X0, x1: BAR_X1, y: 158, key: "auto_max" };
const SL_ANOVEL: Slider = Slider { x0: BAR_X0, x1: BAR_X1, y: 262, key: "win_alpha_novel" };
const SL_ACG: Slider = Slider { x0: BAR_X0, x1: BAR_X1, y: 302, key: "win_alpha_vis" };
const SL_ABACK: Slider = Slider { x0: BAR_X0, x1: BAR_X1, y: 342, key: "win_color" };
const SL_ACMD: Slider = Slider { x0: BAR_X0, x1: BAR_X1, y: 382, key: "win_alpha_command" };

/// Widget index that asks for no help line. The page's help sheet has one
/// row per original setting and no row for the font panel, so hovering it
/// must not light up someone else's explanation.
const NO_ROW: usize = usize::MAX;

const PAGE1: &[Hot] = &[
    // 画面模式
    hot(458, 14, 252, 36, Act::Opt("win_disp", 0), 0),
    hot(714, 14, 252, 36, Act::Opt("win_disp", 1), 0),
    // 文本显示速度
    // The rects must cover the button frame: the gold "selected" version in
    // sys_0320X differs from the base only in the 2 px border, so a rect that
    // is inset by 4 px hides the highlight entirely.
    hot(458, 72, 124, 45, Act::Opt("wait", 0), 1),
    hot(586, 72, 124, 45, Act::Opt("wait", 1), 1),
    hot(714, 72, 124, 45, Act::Opt("wait", 2), 1),
    hot(842, 72, 124, 45, Act::Opt("wait", 3), 1),
    // 文本快进模式
    hot(458, 118, 252, 36, Act::Opt("msg_cut_optin", 0), 2),
    hot(714, 118, 252, 36, Act::Opt("msg_cut_optin", 1), 2),
    // 自动模式速度
    hot_sl(486, 162, 44, 28, Act::Step("auto_max", -1), 3, &SL_AUTO),
    hot_sl(894, 162, 44, 28, Act::Step("auto_max", 1), 3, &SL_AUTO),
    hot_sl(531, 156, 361, 36, Act::Track, 3, &SL_AUTO),
    // 画面特效
    hot(458, 222, 252, 36, Act::Opt("effect", 0), 4),
    hot(714, 222, 252, 36, Act::Opt("effect", 1), 4),
    // 窗口透明度(普通场景)
    hot(388, 272, 80, 26, Act::Off("win_alpha_novel"), 5),
    hot_sl(486, 266, 44, 28, Act::Step("win_alpha_novel", -1), 5, &SL_ANOVEL),
    hot_sl(894, 266, 44, 28, Act::Step("win_alpha_novel", 1), 5, &SL_ANOVEL),
    hot_sl(531, 261, 361, 36, Act::Track, 5, &SL_ANOVEL),
    // 窗口透明度(CG场景)
    hot(388, 302, 80, 26, Act::Off("win_alpha_vis"), 6),
    hot_sl(486, 306, 44, 28, Act::Step("win_alpha_vis", -1), 6, &SL_ACG),
    hot_sl(894, 306, 44, 28, Act::Step("win_alpha_vis", 1), 6, &SL_ACG),
    hot_sl(531, 300, 361, 36, Act::Track, 6, &SL_ACG),
    // 背景透明度(小说场景)
    hot_sl(486, 346, 44, 28, Act::Step("win_color", -1), 7, &SL_ABACK),
    hot_sl(894, 346, 44, 28, Act::Step("win_color", 1), 7, &SL_ABACK),
    hot_sl(531, 340, 361, 36, Act::Track, 7, &SL_ABACK),
    // 指令图标透明度
    hot_sl(486, 386, 44, 28, Act::Step("win_alpha_command", -1), 8, &SL_ACMD),
    hot_sl(894, 386, 44, 28, Act::Step("win_alpha_command", 1), 8, &SL_ACMD),
    hot_sl(531, 381, 361, 36, Act::Track, 8, &SL_ACMD),
];

// Page 2 - 系统设置 / 声音设置 / 暂停设置. sys_03041 has 5 help lines:
// 0 yes/no, 1 right click, 2 volume, 3 page voice, 4 H scene voice.
const SL_VALL: Slider = Slider { x0: BAR_X0, x1: BAR_X1, y: 118, key: "all_vol" };
const SL_VBGM: Slider = Slider { x0: BAR_X0, x1: BAR_X1, y: 158, key: "bgm_vol" };
const SL_VSE: Slider = Slider { x0: BAR_X0, x1: BAR_X1, y: 198, key: "se_vol" };
const SL_VVOICE: Slider = Slider { x0: BAR_X0, x1: BAR_X1, y: 238, key: "voice_vol" };

const PAGE2: &[Hot] = &[
    // 是/否确认: the left button is 「确认」, i.e. the feature is ON = 1.
    hot(458, 14, 252, 36, Act::Opt("yes_no", 1), 0),
    hot(714, 14, 252, 36, Act::Opt("yes_no", 0), 0),
    // 滑鼠右键功能
    hot(458, 48, 164, 47, Act::Opt("R_click", 0), 1),
    hot(630, 48, 164, 47, Act::Opt("R_click", 1), 1),
    hot(802, 48, 164, 47, Act::Opt("R_click", 2), 1),
    // 全体音量
    hot(390, 118, 80, 28, Act::Off("all_vol"), 2),
    hot_sl(486, 122, 44, 28, Act::Step("all_vol", -1), 2, &SL_VALL),
    hot_sl(894, 122, 44, 28, Act::Step("all_vol", 1), 2, &SL_VALL),
    hot_sl(531, 117, 361, 36, Act::Track, 2, &SL_VALL),
    // BGM 音量
    hot(390, 158, 80, 28, Act::Off("bgm_vol"), 2),
    hot_sl(486, 162, 44, 28, Act::Step("bgm_vol", -1), 2, &SL_VBGM),
    hot_sl(894, 162, 44, 28, Act::Step("bgm_vol", 1), 2, &SL_VBGM),
    hot_sl(531, 157, 361, 36, Act::Track, 2, &SL_VBGM),
    // SE 音量
    hot(390, 198, 80, 28, Act::Off("se_vol"), 2),
    hot_sl(486, 202, 44, 28, Act::Step("se_vol", -1), 2, &SL_VSE),
    hot_sl(894, 202, 44, 28, Act::Step("se_vol", 1), 2, &SL_VSE),
    hot_sl(531, 197, 361, 36, Act::Track, 2, &SL_VSE),
    hot(950, 202, 44, 28, Act::Test, 2),
    // VOICE 音量
    hot(390, 238, 80, 28, Act::Off("voice_vol"), 2),
    hot_sl(486, 242, 44, 28, Act::Step("voice_vol", -1), 2, &SL_VVOICE),
    hot_sl(894, 242, 44, 28, Act::Step("voice_vol", 1), 2, &SL_VVOICE),
    hot_sl(531, 237, 361, 36, Act::Track, 2, &SL_VVOICE),
    hot(950, 242, 44, 28, Act::Test, 2),
    // 翻页时中断语音: the left button is 「中断」= 1.
    hot(458, 302, 252, 36, Act::Opt("page_voice", 1), 3),
    hot(714, 302, 252, 36, Act::Opt("page_voice", 0), 3),
    // H 场景时播放语音: 「全部」= 2, 「仅限女性角色」= 1.
    hot(458, 342, 252, 36, Act::Opt("ero_voice", 2), 4),
    hot(714, 342, 252, 36, Act::Opt("ero_voice", 1), 4),
];

/// Page 3 has ten identical rows; built at run time because each one names a
/// different `char_voiceN` key.
fn page3() -> Vec<Hot> {
    let keys: [&'static str; 10] = [
        "char_voice0",
        "char_voice1",
        "char_voice2",
        "char_voice3",
        "char_voice4",
        "char_voice5",
        "char_voice6",
        "char_voice7",
        "char_voice8",
        "char_voice9",
    ];
    let mut v = Vec::with_capacity(20);
    for (i, k) in keys.iter().enumerate() {
        let y = 14 + i as i32 * 40;
        // Left button is 「语音开」 = 1, right is 「语音关」 = 0.
        v.push(hot(458, y, 252, 36, Act::Opt(k, 1), 0));
        v.push(hot(714, y, 252, 36, Act::Opt(k, 0), 0));
    }
    v
}

fn page_hots(page: usize) -> Vec<Hot> {
    match page {
        0 => PAGE1.to_vec(),
        1 => PAGE2.to_vec(),
        _ => page3(),
    }
}

/// Menu index of page 3 -> engine character id. `-1` is "other male voices"
/// and `-2` "other female voices"; those have no standing character here.
pub const CHAR_ROW_ID: [i32; 10] = [0, 2, 1, 3, 4, 5, 10, 11, -1, -2];

pub fn char_row_of(char_id: u32) -> Option<usize> {
    CHAR_ROW_ID.iter().position(|c| *c == char_id as i32)
}

// ---------------------------------------------------------------------------
// assets and drawing
// ---------------------------------------------------------------------------

/// Last dumped state, so `WA2_MENU_DUMP` only writes on changes.
static DUMP_SIG: std::sync::Mutex<String> = std::sync::Mutex::new(String::new());

pub struct Assets {
    base: [Option<TgaImage>; 3],
    /// Caption-free page art baked by `menu-clean`. When a page has one, the
    /// runtime blits it and skips the per-frame wipe entirely.
    clean: [Option<TgaImage>; 3],
    /// Caption-free copy of the hover/selected sheet (`sys_0321X`). It only
    /// contributes the state rim now: its plate was rebuilt from the clean
    /// page, so the baked captions of the original sheet cannot show through
    /// the vector text.
    hover: [Option<TgaImage>; 3],
    gold: [Option<TgaImage>; 3],
    cyan: [Option<TgaImage>; 3],
    header: Option<TgaImage>,
    tabs: Option<TgaImage>,
    nav: Option<TgaImage>,
    defbtn: Option<TgaImage>,
    help: [Option<TgaImage>; 3],
}

/// `Some("4x")` for a sibling art tree like `bg_4x`, `None` for an original.
fn tree_scale_suffix(dir: &Path) -> Option<&'static str> {
    let name = dir.file_name()?.to_str()?;
    for s in ["1x", "2x", "4x", "8x"] {
        if name.ends_with(&format!("_{}", s)) {
            return Some(match s {
                "1x" => "1x",
                "2x" => "2x",
                "4x" => "4x",
                _ => "8x",
            });
        }
    }
    None
}

fn load(dirs: &[&Path], name: &str) -> Option<TgaImage> {
    for dir in dirs {
        for cand in crate::codec::image::candidate_names(name) {
            let p = dir.join(&cand);
            if let Ok(img) = crate::codec::image::decode_ui_file(&p) {
                    return Some(img);
            }
        }
        // case-insensitive scan as a last resort
        if let Ok(rd) = std::fs::read_dir(dir) {
            let wants: Vec<String> = crate::codec::image::candidate_names(name)
                .into_iter().map(|n| n.to_ascii_lowercase()).collect();
            for e in rd.flatten() {
                if let Ok(n) = e.file_name().into_string() {
                    if wants.iter().any(|w| n.to_ascii_lowercase() == *w) {
                        if let Ok(img) = crate::codec::image::decode_ui_file(&e.path()) {
                            return Some(img);
                        }
                    }
                }
            }
        }
    }
    eprintln!("menu: {} NOT FOUND", name);
    None
}

impl Assets {
    pub fn load(dirs: &[&Path]) -> Assets {
        // The menu sheets are drawn at their logical 1280x720 size, and the
        // patch's Chinese artwork lives in the 1x trees. The 4x tree is a
        // runtime art tree produced by the upscaler: on the sound page its
        // AI-upgraded sheets lost the 音量ON/OFF glyphs, so the originals come
        // first here and 4x is only a last resort.
        let mut ordered: Vec<&Path> = Vec::new();
        ordered.extend(dirs.iter().copied().filter(|d| tree_scale_suffix(d).is_none()));
        ordered.extend(
            dirs.iter()
                .copied()
                .filter(|d| tree_scale_suffix(d).is_some_and(|s| s != "4x")),
        );
        ordered.extend(
            dirs.iter()
                .copied()
                .filter(|d| tree_scale_suffix(d) == Some("4x")),
        );
        let dirs: &[&Path] = &ordered;
        let cache = clean_dir();
        let clean = [
            read_clean(&cache, 0),
            read_clean(&cache, 1),
            read_clean(&cache, 2),
        ];
        let hover = [
            read_hover(&cache, 0),
            read_hover(&cache, 1),
            read_hover(&cache, 2),
        ];
        Assets {
            base: [
                load(dirs, "sys_03100.tga"),
                load(dirs, "sys_03101.tga"),
                load(dirs, "sys_03102.tga"),
            ],
            clean,
            hover,
            gold: [
                load(dirs, "sys_03200.tga"),
                load(dirs, "sys_03210.tga"),
                load(dirs, "sys_03220.tga"),
            ],
            cyan: [
                load(dirs, "sys_03201.tga"),
                load(dirs, "sys_03211.tga"),
                load(dirs, "sys_03221.tga"),
            ],
            header: load(dirs, "sys_03000.tga"),
            tabs: load(dirs, "sys_03011.tga"),
            nav: load(dirs, "sys_03020.tga"),
            defbtn: load(dirs, "sys_03030.tga"),
            help: [
                load(dirs, "sys_03040.tga"),
                load(dirs, "sys_03041.tga"),
                load(dirs, "sys_03042.tga"),
            ],
        }
    }

    pub fn available(&self) -> bool {
        self.base.iter().any(|b| b.is_some())
    }
}

/// Which overlay a widget should be drawn from.
#[derive(Clone, Copy, PartialEq, Eq)]
enum Style {
    Base,
    Gold,
    Cyan,
}

pub struct Screen {
    pub page: usize,
    /// Index into `page_hots(page)` of the widget under the cursor.
    pub hover: Option<usize>,
    /// Row whose help line is shown.
    pub help_row: Option<usize>,
    /// Hovering the navigation bar / default button.
    pub nav_hover: Option<usize>,
    /// Part of the font block under the cursor: a row, one of its arrows, or
    /// the style button.
    pub font_hover: Option<usize>,
    /// The font row the arrow keys move.
    pub font_sel: usize,
}

fn draw_page(
    s: &mut Surface,
    a: &Assets,
    page: usize,
    hots: &[Hot],
    st: &Settings,
    styles: &[Style],
) {
    // A baked clean page replaces the art and the wipe with one blit.
    let shift = page_shift(page);
    let src = a.clean[page].as_ref().or_else(|| a.base[page].as_ref());
    if let Some(b) = src {
        if shift > 0 {
            // Page 1 carries the font rows, so the artwork is cut at the split:
            // the strip above stays, everything below moves down, and the band
            // that opens up is theirs. The plate is black and so is the screen
            // behind it, so the seam does not show.
            s.blit_region(b, 0, 0, PAGE_W as u32, FONT_SPLIT as u32, PAGE_X, PAGE_Y);
            s.blit_region(
                b,
                0,
                FONT_SPLIT,
                PAGE_W as u32,
                (PAGE_H - FONT_SPLIT) as u32,
                PAGE_X,
                PAGE_Y + FONT_SPLIT + shift,
            );
        } else {
            s.blit(b, PAGE_X, PAGE_Y);
        }
    }
    // The baked state sheet carries the original captions inside every plate.
    // The cached copy has the plates rebuilt from the clean page, so only its
    // rims are new.
    let state = a.hover[page].as_ref().or_else(|| a.cyan[page].as_ref());
    // Slider values: the gold layer is a "lit" copy of the whole bar, so the
    // current level is the part of it that gets blitted back over the dark bar.
    let mut done: Vec<&str> = Vec::new();
    for h in hots {
        let sl = match h.slider {
            Some(sl) => sl,
            None => continue,
        };
        if done.contains(&sl.key) {
            continue;
        }
        done.push(sl.key);
        if let (Some(src), Some(w)) = (state, fill_width(st, sl)) {
            s.blit_region(src, sl.x0, sl.y, w as u32, BAR_H as u32, PAGE_X + sl.x0, page_y(page, sl.y));
        }
    }
    for (i, h) in hots.iter().enumerate() {
        let style = styles[i];
        // The font entry is drawn flat in the panel's own style; the gold and
        // cyan sheets are button frames for rows the artwork laid out.
        if style == Style::Base {
            continue;
        }
        let src = match style {
            // The value sheet is gold. Its rim and the body wash both read as
            // yellow, which fights the cyan page. Selected and hovered options
            // use the cyan sheet; the body wash tells them apart.
            Style::Gold | Style::Cyan => state,
            Style::Base => None,
        };
        if let Some(src) = src {
            // A hovered bar is drawn from the bright layer instead of the gold
            // one, but still only as far as the value goes, so dragging it keeps
            // showing the level instead of lighting up the whole bar.
            if style == Style::Cyan && matches!(h.act, Act::Track) {
                if let Some(sl) = h.slider {
                    if let Some(w) = fill_width(st, sl) {
                        s.blit_region(src, sl.x0, sl.y, w as u32, BAR_H as u32, PAGE_X + sl.x0, page_y(page, sl.y));
                    }
                    continue;
                }
            }
            s.blit_region(src, h.x.max(0), h.y.max(0), h.w as u32, h.h as u32, PAGE_X + h.x, page_y(page, h.y));
            // The gold and cyan sheets only recolour the rim. The interior is
            // the same dark plate as the base, so a selected option would
            // otherwise read as a thin outline. Wash the body, and leave the
            // label (the bright glyphs) alone.
            if matches!(h.act, Act::Opt(_, _)) {
                // Selected is the stronger cyan. Hover is the same hue, lighter,
                // so the current value stays obvious under the cursor too.
                // Three distinct states: idle is the untouched plate,
                // selected is a mid cyan, hover is the same hue a little
                // lighter. The old 175 washed the button out under a white
                // caption and a bright rim.
                let (tint, alpha) = if style == Style::Gold {
                    ([48u8, 196, 208], 132u8)
                } else {
                    ([36, 176, 196], 80)
                };
                fill_option_body(s, PAGE_X + h.x, page_y(page, h.y), h.w, h.h, tint, alpha);
            }
        }
    }
}

/// Tint the inside of an option button. Pixels bright enough to be the label
/// are left as they are, so the caption stays white on the wash.
fn fill_option_body(s: &mut Surface, x: i32, y: i32, w: i32, h: i32, tint: [u8; 3], alpha: u8) {
    let inset = 5;
    let a = alpha as u32;
    let ia = 255 - a;
    for yy in (y + inset)..(y + h - inset) {
        for xx in (x + inset)..(x + w - inset) {
            if xx < 0 || yy < 0 || xx >= s.width as i32 || yy >= s.height as i32 {
                continue;
            }
            let i = ((yy as u32) * s.width + xx as u32) as usize * 4;
            // No brightness test here on purpose. The label is drawn after this
            // wash and the baked caption underneath is wiped, so guessing
            // "is this pixel text" only left the caption's own halo behind.
            for c in 0..3 {
                s.pixels[i + c] = ((tint[c] as u32 * a + s.pixels[i + c] as u32 * ia) / 255) as u8;
            }
        }
    }
}

/// How many pixels of a slider bar are lit at the setting's current level.
///
/// The fill always stops on a segment boundary, so a level never leaves a
/// half-lit segment behind. Level 0 leaves the bar dark, which is what the
/// 「音量OFF」 button produces.
fn fill_width(st: &Settings, sl: &Slider) -> Option<i32> {
    let level = st.get(sl.key).unwrap_or(0).clamp(0, LEVELS);
    let lit = (level * BAR_SEGMENTS + LEVELS / 2) / LEVELS;
    if lit <= 0 {
        None
    } else {
        // The last lit segment is followed by a 2 px gap that stays dark.
        Some((lit * BAR_PITCH - 2).min(sl.x1 - sl.x0))
    }
}

/// Compose the whole option screen.
pub fn compose(s: &mut Surface, a: &Assets, st: &Settings, scr: &Screen) {
    let hots = page_hots(scr.page);
    if let Some(h) = a.header.as_ref() {
        s.blit(h, 0, 0);
    }
    // Tabs: column = page, row = state (0 current, 1 hover, 2 idle).
    if let Some(t) = a.tabs.as_ref() {
        for i in 0..3 {
            let state = if i == scr.page {
                0
            } else if scr.nav_hover == Some(NAV_TAB + i) {
                1
            } else {
                2
            };
            s.blit_region(
                t,
                i as i32 * TAB_W,
                state * TAB_H,
                TAB_W as u32,
                TAB_H as u32,
                TABS_X + i as i32 * TAB_W,
                TABS_Y,
            );
        }
    }
    let page = scr.page;
    let hover = scr.hover;
    let styles: Vec<Style> = hots
        .iter()
        .enumerate()
        .map(|(i, h)| {
            if hover == Some(i) {
                return Style::Cyan;
            }
            match h.act {
                Act::Opt(k, v) if st.get(k) == Some(v) => Style::Gold,
                _ => Style::Base,
            }
        })
        .collect();
    draw_page(s, a, page, &hots, st, &styles);

    // Help line for the row under the cursor. It follows the page down, or it
    // would sit in the middle of the rows on page 1.
    let hr = scr.help_row
        .or_else(|| hover.and_then(|i| (hots[i].row != NO_ROW).then_some(hots[i].row)));
    if let (Some(img), Some(row)) = (a.help[page].as_ref(), hr) {
        let y = (row as i32 * HELP_H).min(img.height as i32 - HELP_H).max(0);
        s.fill_rect(GUIDE_X, guide_y(page), GUIDE_W as u32, GUIDE_H as u32, [0, 0, 0, 255]);
        s.blit_region(img, 0, y, HELP_W as u32, HELP_H as u32, HELP_X, help_y(page));
    }

    // 恢复默认, on its own under the page.
    if let Some(d) = a.defbtn.as_ref() {
        let state = if scr.nav_hover == Some(NAV_DEF) { 2 } else { 0 };
        s.blit_region(d, 0, state * DEF_H, DEF_W as u32, DEF_H as u32, DEF_X, DEF_Y);
    }
}

// ---------------------------------------------------------------------------
// interaction
// ---------------------------------------------------------------------------

/// Hit test in screen coordinates. Returns (index, is_nav) - nav widgets use
/// Chrome widgets live above `NAV_BASE` so they can never collide with a page
/// widget index (an earlier version used 10..20, which clashed with the
/// eleventh page widget and made the navigation bar think it was hovered).
const NAV_BASE: usize = 100;
const NAV_TAB: usize = NAV_BASE;
const NAV_PREV: usize = NAV_BASE + 3;
const NAV_MID: usize = NAV_BASE + 4;
const NAV_NEXT: usize = NAV_BASE + 5;
const NAV_DEF: usize = NAV_BASE + 20;

fn is_nav(i: usize) -> bool {
    i >= NAV_BASE
}

fn hit(scr_page: usize, hots: &[Hot], sx: i32, sy: i32) -> Option<usize> {
    for (i, h) in hots.iter().enumerate() {
        let (x, y) = (PAGE_X + h.x, page_y(scr_page, h.y));
        if sx >= x && sx < x + h.w && sy >= y && sy < y + h.h {
            return Some(i);
        }
    }
    // tabs
    if sy >= TABS_Y && sy < TABS_Y + TAB_H {
        for i in 0..3 {
            let x = TABS_X + i as i32 * TAB_W;
            if sx >= x && sx < x + TAB_W {
                // Clicking the tab of the page we are already on does nothing.
                return if i == scr_page { None } else { Some(NAV_TAB + i) };
            }
        }
    }
    if sy >= DEF_Y && sy < DEF_Y + DEF_H && sx >= DEF_X && sx < DEF_X + DEF_W {
        return Some(NAV_DEF);
    }
    None
}

/// The slider track under a screen position, if any.
fn track_at(page: usize, sx: i32, sy: i32) -> Option<&'static Slider> {
    page_hots(page).into_iter().find_map(|h| {
        if !matches!(h.act, Act::Track) {
            return None;
        }
        let (x, y) = (PAGE_X + h.x, PAGE_Y + h.y);
        if sx >= x && sx < x + h.w && sy >= y && sy < y + h.h {
            h.slider
        } else {
            None
        }
    })
}

fn nav_act(idx: usize) -> Option<Act> {
    match idx {
        i if (NAV_TAB..NAV_TAB + 3).contains(&i) => Some(Act::Tab(i - NAV_TAB)),
        NAV_DEF => Some(Act::Reset),
        _ => None,
    }
}

/// One caption drawn with the system font, in 1280×720 coordinates.
/// Menu ink, kept apart from the dialogue's: these captions sit on rules,
/// buttons and dark plates, so they are lighter and flatter than text that has
/// to survive on top of a CG.
mod ink {
    /// Group heading.
    pub const HEAD: [u8; 3] = [186, 230, 240];
    /// Left-hand row label.
    pub const LABEL: [u8; 3] = [222, 236, 242];
    /// Button main text.
    pub const BUTTON: [u8; 3] = [255, 255, 255];
    /// Button second line, and the parenthetical notes.
    pub const SUB: [u8; 3] = [134, 178, 190];
    /// Slider scale (0%, 100%, MIN, MAX, FAST, SLOW).
    pub const SCALE: [u8; 3] = [152, 198, 210];
    /// Bottom navigation and the tab strip.
    pub const NAV: [u8; 3] = [236, 246, 250];
}

struct Lab {
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    text: &'static str,
    sub: &'static str,
    size: u32,
    /// Size of the second line, when the sub text is itself Chinese.
    sub_size: u32,
    color: [u8; 3],
    sub_color: [u8; 3],
    /// Extra pixels between glyphs, in 1280x720 layout units. The original
    /// captions were letter-spaced; this is that spacing, done as tracking
    /// rather than as real spaces in the string (which would also change the
    /// measured width, the centring and the wrap).
    tracking: i32,
    center: bool,
    /// Pixels eaten off each edge before the baked glyphs are covered, so a
    /// button frame is not painted over.
    inset: i32,
    /// Fill the whole box. Used where the artwork has no dark pixel to sample
    /// (the tab face is brighter than its caption).
    fill: bool,
}

fn lab(
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    text: &'static str,
    sub: &'static str,
    size: u32,
    center: bool,
    inset: i32,
    fill: bool,
) -> Lab {
    Lab {
        x,
        y,
        w,
        h,
        text,
        sub,
        size,
        sub_size: (size * 2 / 3).max(11),
        color: ink::BUTTON,
        sub_color: ink::SUB,
        tracking: 0,
        center,
        inset,
        fill,
    }
}

/// The page whose labels are being built, so the helpers can add the same
/// shift the artwork got. Page 1 moves down for the font block; the others do
/// not move at all.
fn page_shift(page: usize) -> i32 {
    if page == 0 {
        FONT_BLOCK_H
    } else {
        0
    }
}

thread_local! {
    /// Set while `page_labs` runs, which is the one caller that knows the page.
    static LABEL_PAGE: std::cell::Cell<usize> = const { std::cell::Cell::new(1) };
}

fn label_page() -> usize {
    LABEL_PAGE.with(|c| c.get())
}

/// Where the label of a page-relative y lands.
fn label_y(y: i32) -> i32 {
    PAGE_Y + y + page_shift(label_page())
}

/// Group heading: 18px, a touch of tracking, the pale teal the section rules
/// are drawn in.
fn head(x: i32, y: i32, w: i32, text: &'static str) -> Lab {
    let mut l = lab(PAGE_X + x, label_y(y), w, 20, text, "", 18, false, 0, false);
    l.color = ink::HEAD;
    l.tracking = 1;
    l
}

/// Left-hand label of a row. 20px, no tracking: at 22 with a space between
/// every glyph it read looser than the button text it labels.
fn row(x: i32, y: i32, w: i32, h: i32, text: &'static str, sub: &'static str) -> Lab {
    let mut l = lab(PAGE_X + x, label_y(y), w, h, text, sub, 20, false, 0, false);
    l.color = ink::LABEL;
    l
}

/// Parenthetical note under a label.
fn note(x: i32, y: i32, w: i32, text: &'static str) -> Lab {
    let mut l = lab(PAGE_X + x, label_y(y), w, 16, text, "", 13, false, 0, false);
    l.color = ink::SUB;
    l
}

fn btn(x: i32, y: i32, w: i32, h: i32, text: &'static str, sub: &'static str) -> Lab {
    let mut l = lab(PAGE_X + x, label_y(y), w, h, text, sub, 17, true, 7, false);
    l.color = ink::BUTTON;
    l.sub_size = 11;
    l
}

/// Slider scale.
fn tiny(x: i32, y: i32, w: i32, h: i32, text: &'static str) -> Lab {
    let mut l = lab(PAGE_X + x, label_y(y), w, h, text, "", 10, true, 3, false);
    l.color = ink::SCALE;
    l
}

fn menu_px(layout_px: u32, window_h: i32) -> u32 {
    ((layout_px as i32 * window_h.max(1)) / 720).clamp(8, 64) as u32
}

fn page_labs(page: usize) -> Vec<Lab> {
    let mut v = Vec::new();
    LABEL_PAGE.with(|c| c.set(page));
    match page {
        0 => {
            v.push(head(36, 0, 220, "画面设置"));
            v.push(row(48, 20, 240, 28, "画面模式", ""));
            v.push(btn(458, 14, 252, 36, "瞬间显示", "Window"));
            v.push(btn(714, 14, 252, 36, "全屏模式", "Full Screen"));
            v.push(head(36, 54, 200, "文本设置"));
            v.push(row(40, 80, 250, 30, "文本显示速度", ""));
            v.push(btn(458, 72, 124, 45, "瞬间显示", "Moment"));
            v.push(btn(586, 72, 124, 45, "高速", "Fast"));
            v.push(btn(714, 72, 124, 45, "普通", "Normal"));
            v.push(btn(842, 72, 124, 45, "顺滑", "Smooth"));
            v.push(row(40, 122, 250, 30, "文本快进模式", ""));
            v.push(btn(458, 118, 252, 36, "全文", "All"));
            v.push(btn(714, 118, 252, 36, "仅限已读", "Already Read"));
            v.push(row(40, 162, 250, 28, "自动模式速度", ""));
            // Scales sit on their own row under the track, aligned to its
            // ends. They used to sit on the track's bottom edge.
            v.push(tiny(BAR_X0 - 16, SL_AUTO.y + BAR_H + 4, 44, 11, "FAST"));
            v.push(tiny(BAR_X1 - 44, SL_AUTO.y + BAR_H + 4, 48, 11, "SLOW"));
            v.push(head(36, 204, 200, "视觉设置"));
            v.push(row(48, 226, 240, 28, "画面特效", ""));
            v.push(btn(458, 222, 252, 36, "瞬间显示", "Fast"));
            v.push(btn(714, 222, 252, 36, "普通显示", "Normal"));
            v.push(row(40, 254, 220, 22, "窗口不透明度", ""));
            v.push(note(48, 278, 180, "（普通场景）"));
            v.push(row(40, 296, 220, 22, "窗口不透明度", ""));
            v.push(note(56, 320, 160, "（CG场景）"));
            v.push(row(40, 336, 220, 22, "背景不透明度", ""));
            v.push(note(48, 360, 180, "（小说场景）"));
            v.push(row(40, 386, 260, 26, "指令图标不透明度", ""));
            for sl in [SL_ANOVEL, SL_ACG, SL_ABACK, SL_ACMD] {
                let row_y = sl.y + BAR_H + 4;
                v.push(tiny(BAR_X0 - 16, row_y, 44, 11, "0%"));
                v.push(tiny(BAR_X1 - 44, row_y, 48, 11, "100%"));
            }
        }
        1 => {
            v.push(head(36, 0, 220, "系统设置"));
            v.push(row(40, 20, 240, 28, "是 / 否确认", ""));
            v.push(btn(458, 14, 252, 36, "确认", "Confirmation ON"));
            v.push(btn(714, 14, 252, 36, "不确认", "Confirmation OFF"));
            v.push(row(40, 56, 260, 30, "滑鼠右键功能", ""));
            v.push(btn(458, 48, 164, 47, "隐藏窗口", "Window OFF"));
            v.push(btn(630, 48, 164, 47, "设置菜单", "Option"));
            v.push(btn(802, 48, 164, 47, "存档", "Save"));
            v.push(head(36, 96, 200, "声音设置"));
            for (y, name) in [(118, "全体音量"), (158, "BGM音量"), (198, "SE音量"), (238, "VOICE音量")] {
                v.push(row(40, y + 2, 220, 28, name, ""));
                v.push(tiny(BAR_X0 - 16, y + BAR_H + 4, 44, 11, "MIN"));
                v.push(tiny(BAR_X1 - 44, y + BAR_H + 4, 48, 11, "MAX"));
            }
            v.push(head(36, 280, 200, "暂停设置"));
            v.push(row(40, 308, 270, 30, "翻页时中断语音", ""));
            v.push(btn(458, 302, 252, 36, "中断", "Stop"));
            v.push(btn(714, 302, 252, 36, "不中断", "Keep"));
            v.push(row(40, 348, 270, 30, "H场景时播放语音", ""));
            v.push(btn(458, 342, 252, 36, "全部", "Normal"));
            v.push(btn(714, 342, 252, 36, "仅限女性角色", "Female Voice Only"));
        }
        _ => {
            v.push(head(36, 0, 280, "角色语音设置"));
            let names = [
                "北原春希",
                "小木曾雪菜",
                "冬马和纱",
                "杉浦小春",
                "和泉千晶",
                "风冈麻理",
                "饭塚武也",
                "水泽依绪",
                "其他男性角色",
                "其他女性角色",
            ];
            for (i, name) in names.iter().enumerate() {
                let y = 14 + i as i32 * 40;
                v.push(row(70, y + 2, 260, 32, name, ""));
                v.push(btn(458, y, 252, 36, "语音开", "ON"));
                v.push(btn(714, y, 252, 36, "语音关", "OFF"));
            }
        }
    }
    v
}

fn chrome_labs() -> Vec<Lab> {
    // The page navigation bar is gone; 恢复默认 carries its own caption in the
    // artwork, so the chrome has nothing left to stamp.
    Vec::new()
}

fn help_lines(page: usize) -> &'static [&'static str] {
    match page {
        0 => &[
            "设置游戏时的画面模式。全屏模式时画面比例将固定为16:9",
            "设置文本窗口中文本的显示速度。设置为「瞬间显示」时将无特效。",
            "设置文本快进的条件。设置为「全文」时，未阅读的文本也能快进。",
            "设置开启自动模式时自动播放的速度。",
            "设置画面特效的表现方式。想要更快运行游戏请设置为「瞬间显示」。",
            "设置文本窗口的透明度。「窗口OFF」时文本窗口将完全消失。",
            "设置文本窗口的透明度。「窗口OFF」时文本窗口将完全消失。",
            "设置阅读小说时背景（黑色）的透明度。",
            "设置指令图标在通常情况下的透明度。",
        ],
        1 => &[
            "设置存档和读档时是否弹出确认窗口。",
            "设置滑鼠右键功能。",
            "设置各种音量。按下「音量OFF」键时，将维持当前音量设置并将音量降为0。可按「音符键」试听效果。",
            "设置语音播放的规则。设置为「中断」时，文本翻页时将强制中断语音。",
            "设置H场景中的语音播放状况。设置为「仅限女性角色」时，H场景中将不播放男性角色的语音。",
        ],
        _ => &["设置角色的语音是否开启。设置为OFF的情形下，和VOICE音量设置有关的角色的语音将一概不会播放。"],
    }
}

fn using_cn_art(dirs: &[&Path]) -> bool {
    dirs.iter().any(|d| d.file_name().and_then(|s| s.to_str()) == Some("bg_cn"))
}

fn px_of(img: &TgaImage, x: i32, y: i32) -> [u8; 3] {
    let i = ((y as u32 * img.width + x as u32) * 4) as usize;
    [img.rgba[i], img.rgba[i + 1], img.rgba[i + 2]]
}

/// Teal rules and the section ticks. Same colour family as the caption glow,
/// so they have to be recognised before any luminance test.
fn structural_teal(p: [u8; 3]) -> bool {
    p[0] < 60 && p[1] >= 75 && p[2] >= 75 && (p[1] as i32 - p[2] as i32).abs() < 30
}

fn bright_caption(p: [u8; 3]) -> bool {
    if structural_teal(p) {
        return false;
    }
    let m = p[0].max(p[1]).max(p[2]);
    if m >= 140 {
        return true;
    }
    // Dim Latin subtitles (SCREEN SETTING and the like). The rule is greener.
    p[2] > p[0] + 20 && p[2] + 5 > p[1] && p[1] < 75 && m >= 75
}

/// Button captions sit on the teal fill. Chinese is around (49,119,124) and
/// the English line under it is a little darker, around (34,107,111).
/// Snowflakes stay under r < 32.
fn button_caption(p: [u8; 3]) -> bool {
    p[0] >= 32 && p[0] < 180 && p[1] >= 96 && p[1] < 230 && p[2] >= 90
}

/// The English line under a button caption, around (26,103,105). Dimmer than
/// the Chinese stroke, still clearly above the teal plate.
fn dim_caption(p: [u8; 3]) -> bool {
    p[1] >= 98 && p[1] < 175 && p[2] >= 90 && p[0] < 90 && p[0] as i32 + 12 < p[1] as i32
}

/// Erase baked page captions, including the one-pixel halo around each stroke,
/// and put the colour underneath back. Rules, dotted leaders, snowflakes and
/// widget frames stay. The mask comes from the base art, so a selected
/// button's wash cannot hide the letters or get punched out.
/// Bake the caption-free page art once, instead of repairing the composed
/// frame on every state change.
///
/// `wipe_baked_captions` classifies pixels of the page art, grows the mask and
/// repaints each masked pixel from a neighbour. That is the right algorithm
/// but the wrong place for it: on every hover it walked the whole 1024x464
/// page and could still leave a halo where the classifier disagreed with the
/// artwork. Running it here, once, writes a clean page that the runtime only
/// blits; the wipe stays as the fallback for a data tree with no cache.
///
/// Returns the pages it wrote, so a caller can report what happened.
/// Where the baked caption-free pages live.
fn clean_dir() -> std::path::PathBuf {
    let root = std::env::var("WA2_DATA").unwrap_or_else(|_| "data".to_string());
    std::path::Path::new(&root).join("cache/ui/menu")
}

fn read_clean(dir: &Path, page: usize) -> Option<TgaImage> {
    let p = clean_page_path(dir, page);
    match crate::codec::image::decode_ui_file(&p) {
        Ok(img) => Some(img),
        Err(_) => None,
    }
}

fn read_hover(dir: &Path, page: usize) -> Option<TgaImage> {
    let p = hover_page_path(dir, page);
    match crate::codec::image::decode_ui_file(&p) {
        Ok(img) => Some(img),
        Err(_) => None,
    }
}

fn hover_page_path(out_dir: &Path, page: usize) -> std::path::PathBuf {
    out_dir.join(format!("page{}_hover.tga", page + 1))
}

/// The clean page for `page`: the art with its captions wiped, cropped to
/// the page rectangle. Split out from the file writing so a test can compare
/// it against the runtime path pixel for pixel.
pub fn bake_clean_page(base: &TgaImage, page: usize) -> Surface {
    // The wipe is written against the full screen with the page at
    // (PAGE_X, PAGE_Y), so bake on a full-size surface and crop the page
    // rectangle out of it. Doing it any other way would shift the art.
    let mut full = Surface::new(W, H, [0, 0, 0, 255]);
    full.blit(base, PAGE_X, PAGE_Y);
    wipe_baked_captions(&mut full, base, page);
    let mut out = Surface::new(PAGE_W as u32, PAGE_H as u32, [0, 0, 0, 255]);
    for y in 0..PAGE_H as u32 {
        let src = ((PAGE_Y as u32 + y) * W + PAGE_X as u32) as usize * 4;
        let dst = (y * PAGE_W as u32) as usize * 4;
        let n = PAGE_W as usize * 4;
        out.pixels[dst..dst + n].copy_from_slice(&full.pixels[src..src + n]);
    }
    // The wipe is a classifier; it is bound to miss pixels where the artwork
    // disagrees. The caption regions have a known background (black plates for
    // the label column and the scale rows, a smooth gradient inside a button),
    // so rebuild those from the model instead of guessing (plan §1).
    rebuild_caption_regions(&mut out, page);
    out
}

/// Replace the baked captions of one page with the page's own background
/// model. Runs once, in the bake; the runtime only blits the result.
fn rebuild_caption_regions(s: &mut Surface, page: usize) {
    // 1) Caption column: the art behind the labels and their dotted leaders is
    //    black. Keep the dim teal heading bars and section rules. The leaders
    //    run all the way to the buttons on every page; the ON/OFF pills sit in
    //    that strip on pages 1 and 2 and are drawn as-is, so their rectangles
    //    (plus their glow) are skipped.
    let right = 480;
    let pills: Vec<(i32, i32, i32, i32)> = page_hots(page)
        .iter()
        .filter(|h| matches!(h.act, Act::Off(_)))
        .map(|h| (h.x - 5, h.y - 5, h.x + h.w + 5, h.y + h.h + 5))
        .collect();
    for y in 0..s.height as i32 {
        for x in 8..right.min(s.width as i32) {
            if pills.iter().any(|&(x0, y0, x1, y1)| x >= x0 && x < x1 && y >= y0 && y < y1) {
                continue;
            }
            if structural_run(s, x, y) {
                continue;
            }
            let i = ((y as u32 * s.width + x as u32) * 4) as usize;
            s.pixels[i] = 0;
            s.pixels[i + 1] = 0;
            s.pixels[i + 2] = 0;
            s.pixels[i + 3] = 255;
        }
    }
    // 2) Scale labels under each slider track (FAST/SLOW/0%/100%/MIN/MAX).
    //    The band runs from the track's bottom edge to just above the next
    //    row's track; the arrow buttons own the band's ends (their rims are
    //    white too), so their rectangles are skipped.
    let hots = page_hots(page);
    let mut seen: Vec<&'static str> = Vec::new();
    for h in &hots {
        let Some(sl) = h.slider else { continue };
        if seen.contains(&sl.key) {
            continue;
        }
        seen.push(sl.key);
        let arrows: Vec<(i32, i32, i32, i32)> = hots
            .iter()
            .filter(|a| {
                a.slider.map(|s| s.key) == Some(sl.key) && matches!(a.act, Act::Step(_, _))
            })
            .map(|a| (a.x, a.y, a.x + a.w, a.y + a.h))
            .collect();
        let y0 = sl.y + BAR_H;
        // Run right up to the next row's track. Its top edge is a horizontal
        // run, so the structure test keeps it; every faint label/noise pixel
        // between the tracks is wiped.
        let y1 = (sl.y + 40).min(s.height as i32);
        for y in y0..y1 {
            for x in 500..940.min(s.width as i32) {
                if arrows.iter().any(|&(ax0, ay0, ax1, ay1)| {
                    x >= ax0 - 6 && x < ax1 + 6 && y >= ay0 - 6 && y < ay1 + 6
                }) {
                    continue;
                }
                // No structure lives inside the band: the track edges are
                // above and at the band's end, so everything here is a baked
                // label, its faint leader line or the old value bar.
                let i = ((y as u32 * s.width + x as u32) * 4) as usize;
                s.pixels[i] = 0;
                s.pixels[i + 1] = 0;
                s.pixels[i + 2] = 0;
                s.pixels[i + 3] = 255;
            }
        }
    }
    // 3) Button plates: their captions sit on a smooth gradient, so the
    //    per-column median is the plate colour and glyphs stand out from it.
    for h in page_hots(page) {
        // Only option buttons. The ON/OFF pills are `Act::Off` and their baked
        // captions are already Chinese and always visible; scrubbing them left
        // empty plates.
        if !matches!(h.act, Act::Opt(_, _)) {
            continue;
        }
        scrub_button_caption(s, &h);
    }
}

/// Dim teal that is structure, not ink. The heading bars and rules are darker
/// than both the white captions and their brighter teal anti-aliasing.
fn dim_teal_structure(p: [u8; 3]) -> bool {
    let r = p[0] as i32;
    let g = p[1] as i32;
    let b = p[2] as i32;
    r < 90 && g > r + 25 && b > r + 25 && g < 135 && b < 135
}

fn clear_to_black(s: &mut Surface, x: i32, y: i32) {
    if x < 0 || y < 0 || x >= s.width as i32 || y >= s.height as i32 {
        return;
    }
    let i = ((y as u32 * s.width + x as u32) * 4) as usize;
    let p = [s.pixels[i], s.pixels[i + 1], s.pixels[i + 2]];
    if dim_teal_structure(p) {
        return;
    }
    s.pixels[i] = 0;
    s.pixels[i + 1] = 0;
    s.pixels[i + 2] = 0;
    s.pixels[i + 3] = 255;
}

/// A dim teal pixel only counts as structure when it belongs to a bar or a
/// rule. The dotted leaders are dim teal too, but they are 2-3 px clusters, so
/// a short run test tells them apart from the 20 px heading bars and the long
/// section rules.
fn structural_run(s: &Surface, x: i32, y: i32) -> bool {
    let w = s.width as i32;
    let h = s.height as i32;
    if x < 0 || y < 0 || x >= w || y >= h {
        return false;
    }
    let teal = |xx: i32, yy: i32| -> bool {
        if xx < 0 || yy < 0 || xx >= w || yy >= h {
            return false;
        }
        let i = ((yy as u32 * s.width + xx as u32) * 4) as usize;
        dim_teal_structure([s.pixels[i], s.pixels[i + 1], s.pixels[i + 2]])
    };
    if !teal(x, y) {
        return false;
    }
    let mut run = 1;
    for d in 1..=8 {
        if teal(x + d, y) || teal(x - d, y) {
            run += 1;
        } else {
            break;
        }
    }
    if run >= 8 {
        return true;
    }
    let mut run = 1;
    for d in 1..=8 {
        if teal(x, y + d) || teal(x, y - d) {
            run += 1;
        } else {
            break;
        }
    }
    run >= 8
}

/// Erase one baked caption from a button plate.
fn scrub_button_caption(s: &mut Surface, h: &Hot) {
    let inset = 5;
    let x0 = (h.x + inset).max(0);
    let x1 = (h.x + h.w - inset).min(s.width as i32);
    let y0 = (h.y + inset).max(0);
    let y1 = (h.y + h.h - inset).min(s.height as i32);
    if x1 - x0 < 16 || y1 - y0 < 10 {
        return;
    }
    // The snow decorations sit in the outer eighth of the plate; only the
    // centre carries the caption.
    let cut = ((x1 - x0) / 8).max(4);
    for x in (x0 + cut)..(x1 - cut) {
        let n = (y1 - y0) as usize;
        // Median of the column: the plate gradient is vertical, a glyph only
        // covers a few rows of it.
        let mut col = [0u8; 3];
        for c in 0..3 {
            let mut vals: Vec<u8> = (y0..y1)
                .map(|y| s.pixels[((y as u32 * s.width + x as u32) * 4) as usize + c])
                .collect();
            vals.sort_unstable();
            col[c] = vals[n / 2];
        }
        for y in y0..y1 {
            let i = ((y as u32 * s.width + x as u32) * 4) as usize;
            let p = [s.pixels[i], s.pixels[i + 1], s.pixels[i + 2]];
            let d = (0..3)
                .map(|c| (p[c] as i32 - col[c] as i32).abs())
                .max()
                .unwrap_or(0);
            if d > 16 {
                s.pixels[i] = col[0];
                s.pixels[i + 1] = col[1];
                s.pixels[i + 2] = col[2];
                s.pixels[i + 3] = 255;
            }
        }
    }
}

/// Clean copy of the hover/selected sheet: keep its rims, take the plates from
/// the clean page so no baked caption survives.
fn bake_clean_state(state: &TgaImage, clean: &Surface, page: usize) -> Surface {
    let mut s = Surface::new(state.width, state.height, [0, 0, 0, 255]);
    s.pixels.copy_from_slice(&state.rgba);
    for h in page_hots(page) {
        // Only option buttons: the ON/OFF pills keep this sheet's baked
        // caption (already Chinese).
        if !matches!(h.act, Act::Opt(_, _)) {
            continue;
        }
        let inset = 3;
        let x0 = (h.x + inset).max(0);
        let x1 = (h.x + h.w - inset).min(s.width as i32);
        let y0 = (h.y + inset).max(0);
        let y1 = (h.y + h.h - inset).min(s.height as i32);
        for y in y0..y1 {
            for x in x0..x1 {
                let si = ((y as u32 * clean.width + x as u32) * 4) as usize;
                let di = ((y as u32 * s.width + x as u32) * 4) as usize;
                s.pixels[di..di + 4].copy_from_slice(&clean.pixels[si..si + 4]);
            }
        }
    }
    s
}

pub fn bake_clean_pages(dirs: &[&Path], out_dir: &Path) -> Vec<usize> {
    let assets = Assets::load(dirs);
    let mut written = Vec::new();
    if std::fs::create_dir_all(out_dir).is_err() {
        eprintln!("menu: cannot create {}", out_dir.display());
        return written;
    }
    for page in 0..3 {
        let Some(base) = assets.base[page].as_ref() else { continue };
        let page_surf = bake_clean_page(base, page);
        let path = clean_page_path(out_dir, page);
        let mut ok = true;
        if let Err(e) = page_surf.save_tga(&path.to_string_lossy()) {
            eprintln!("menu: {}: {}", path.display(), e);
            ok = false;
        }
        if let Some(state) = assets.cyan[page].as_ref() {
            let hover = bake_clean_state(state, &page_surf, page);
            let path = hover_page_path(out_dir, page);
            if let Err(e) = hover.save_tga(&path.to_string_lossy()) {
                eprintln!("menu: {}: {}", path.display(), e);
                ok = false;
            }
        }
        if ok {
            written.push(page);
        }
    }
    written
}

/// The baked page for `page`, or None when the cache has not been built.
fn clean_page_path(out_dir: &Path, page: usize) -> std::path::PathBuf {
    out_dir.join(format!("page{}_clean.tga", page + 1))
}

thread_local! {
    /// How much lower the page being wiped sits than its artwork does. Only
    /// page 1 moves, and only for the font block; the helpers below read
    /// page-image coordinates, so they learn the offset from here.
    static WIPE_SHIFT: std::cell::Cell<i32> = const { std::cell::Cell::new(0) };
}

/// Screen y of row y of the page being wiped.
fn wipe_y(y: i32) -> i32 {
    PAGE_Y + WIPE_SHIFT.with(|c| c.get()) + y
}

fn wipe_baked_captions(s: &mut Surface, base: &TgaImage, page: usize) {
    WIPE_SHIFT.with(|c| c.set(page_shift(page)));
    let pw = base.width as i32;
    let ph = base.height as i32;
    if pw <= 0 || ph <= 0 {
        return;
    }
    let n = (pw * ph) as usize;
    let mut mask = vec![false; n];
    let mut protect = vec![false; n];
    for y in 0..ph {
        for x in 0..pw {
            let p = px_of(base, x, y);
            let i = (y * pw + x) as usize;
            let m = p[0].max(p[1]).max(p[2]);
            // Near-white on the left is the caption itself. On the right it is
            // the widget frame, which has to survive.
            if structural_teal(p) || (x >= 430 && m >= 245) {
                protect[i] = true;
            }
        }
    }
    // The ON/OFF pills ship with Chinese captions and are drawn as-is; the
    // caption classifier below (which scans the left column) would punch holes
    // in them, so their rectangles are protected first.
    for h in page_hots(page) {
        if !matches!(h.act, Act::Off(_)) {
            continue;
        }
        let x0 = (h.x - 6).max(0);
        let y0 = (h.y - 6).max(0);
        let x1 = (h.x + h.w + 6).min(pw);
        let y1 = (h.y + h.h + 6).min(ph);
        for y in y0..y1 {
            for x in x0..x1 {
                protect[(y * pw + x) as usize] = true;
            }
        }
    }
    for y in 0..ph {
        for x in 0..(430.min(pw)) {
            let i = (y * pw + x) as usize;
            if !protect[i] && bright_caption(px_of(base, x, y)) {
                mask[i] = true;
            }
        }
    }
    for h in page_hots(page) {
        // Only option buttons. The ON/OFF pills (`Act::Off`) keep the baked
        // Chinese captions they already ship with.
        if !matches!(h.act, Act::Opt(_, _)) {
            continue;
        }
        let x0 = (h.x + 6).max(0);
        let y0 = (h.y + 6).max(0);
        let x1 = (h.x + h.w - 6).min(pw);
        let y1 = (h.y + h.h - 6).min(ph);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * pw + x) as usize;
                // The frame is outside this inset. A white pixel in here is a
                // caption, including on the gold and cyan sheets.
                protect[i] = false;
                let p = px_of(base, x, y);
                // Snowflakes sit on the left and right of the caption. The dim
                // English line is only in the middle, and its red channel drops
                // under the Chinese stroke, so the centre uses a wider test.
                let mid = x >= h.x + h.w / 6 && x < h.x + h.w - h.w / 6;
                if button_caption(p) || (mid && dim_caption(p)) {
                    mask[i] = true;
                }
            }
        }
    }
    // FAST / SLOW / 0% / 100% / MIN / MAX sit outside the option buttons.
    for lab in page_labs(page) {
        if !lab.sub.is_empty() || lab.w > 80 {
            continue;
        }
        let x0 = (lab.x - PAGE_X).max(0);
        let y0 = (lab.y - PAGE_Y).max(0);
        let x1 = (x0 + lab.w).min(pw);
        let y1 = (y0 + lab.h).min(ph);
        for y in y0..y1 {
            for x in x0..x1 {
                let i = (y * pw + x) as usize;
                let p = px_of(base, x, y);
                if !protect[i] && p[0] > 40 && p[1] > 80 && p[0].max(p[1]).max(p[2]) < 220 {
                    mask[i] = true;
                }
            }
        }
    }
    // Two pixels of halo, but not onto a rule or a frame.
    for _ in 0..3 {
        let prev = mask.clone();
        for y in 1..ph - 1 {
            for x in 1..pw - 1 {
                let i = (y * pw + x) as usize;
                if protect[i] || prev[i] {
                    continue;
                }
                let near = prev[i - 1] || prev[i + 1] || prev[(i as i32 - pw) as usize] || prev[(i as i32 + pw) as usize];
                if near {
                    mask[i] = true;
                }
            }
        }
    }
    for y in 0..ph {
        for x in 0..pw {
            if !mask[(y * pw + x) as usize] {
                continue;
            }
            let sx = PAGE_X + x;
            let sy = wipe_y(y);
            if let Some(c) = nearby_fill(s, &mask, pw, x, y) {
                if sx >= 0 && sy >= 0 && (sx as u32) < s.width && (sy as u32) < s.height {
                    let di = ((sy as u32 * s.width + sx as u32) * 4) as usize;
                    s.pixels[di] = c[0];
                    s.pixels[di + 1] = c[1];
                    s.pixels[di + 2] = c[2];
                    s.pixels[di + 3] = 255;
                }
            }
        }
    }
}

fn nearby_fill(s: &Surface, mask: &[bool], pw: i32, x: i32, y: i32) -> Option<[u8; 3]> {
    // Left of the widgets the page is black. Inside a button, take a pixel
    // from the same row so the vertical gradient stays and the caption is not
    // replaced by one flat dark rectangle.
    let want_black = x < 430;
    if !want_black {
        if let Some(c) = body_on_row(s, mask, pw, x, y) {
            return Some(c);
        }
    }
    let mut best: Option<(u32, [u8; 3])> = None;
    for rad in 1i32..24 {
        for dy in -rad..=rad {
            for dx in -rad..=rad {
                if dx.abs() != rad && dy.abs() != rad {
                    continue;
                }
                let nx = x + dx;
                let ny = y + dy;
                if nx < 0 || ny < 0 || nx >= pw || ny >= (mask.len() as i32 / pw) {
                    continue;
                }
                if mask[(ny * pw + nx) as usize] {
                    continue;
                }
                let sx = PAGE_X + nx;
                let sy = wipe_y(ny);
                if sx < 0 || sy < 0 || sx as u32 >= s.width || sy as u32 >= s.height {
                    continue;
                }
                let i = ((sy as u32 * s.width + sx as u32) * 4) as usize;
                let c = [s.pixels[i], s.pixels[i + 1], s.pixels[i + 2]];
                let sum = c[0] as u32 + c[1] as u32 + c[2] as u32;
                let good = if want_black { sum < 40 } else { (40..190).contains(&sum) };
                if good {
                    return Some(c);
                }
                if best.map(|(s0, _)| sum < s0).unwrap_or(true) {
                    best = Some((sum, c));
                }
            }
        }
    }
    best.map(|(_, c)| c)
}

/// Teal button fill on this row: darker than a caption, not the white frame.
fn body_on_row(s: &Surface, mask: &[bool], pw: i32, x: i32, y: i32) -> Option<[u8; 3]> {
    let rows = mask.len() as i32 / pw;
    let mut idle_hit = None;
    for dist in 1..160 {
        for nx in [x - dist, x + dist] {
            if nx < 0 || nx >= pw || y < 0 || y >= rows {
                continue;
            }
            if mask[(y * pw + nx) as usize] {
                continue;
            }
            let sx = PAGE_X + nx;
            let sy = wipe_y(y);
            if sx < 0 || sy < 0 || sx as u32 >= s.width || sy as u32 >= s.height {
                continue;
            }
            let i = ((sy as u32 * s.width + sx as u32) * 4) as usize;
            let c = [s.pixels[i], s.pixels[i + 1], s.pixels[i + 2]];
            let gb = (c[1] as i32 - c[2] as i32).abs();
            // A selected button's plate is the cyan wash. Prefer that over a
            // darker unwashed fringe, or the old English line is repainted as
            // a shadow.
            let washed = c[1] > 160 && c[0] < 90 && (c[0] as i32 + 40) < c[1] as i32 && gb < 30 && c[0].max(c[1]).max(c[2]) < 240;
            if washed {
                return Some(c);
            }
            let idle = c[0] < 30 && (45..95).contains(&c[1]) && gb < 25;
            if idle && idle_hit.is_none() {
                idle_hit = Some(c);
            }
        }
    }
    idle_hit
}

/// The chrome has nothing left to wipe: the tab strip ships the captions
/// the engine wants and the page navigation bar behind them is gone.
fn wipe_chrome_captions(_s: &mut Surface) {}

fn repaint_from_column(s: &mut Surface, src_x: i32, x: i32, y: i32, w: i32, h: i32) {
    if w <= 0 || h <= 0 {
        return;
    }
    for yy in y..y + h {
        let Some(bg) = surf_px(s, src_x, yy) else { continue };
        for xx in x..x + w {
            put_px(s, xx, yy, bg);
        }
    }
}

fn flatten_pill(s: &mut Surface, x: i32, y: i32, w: i32, h: i32) {
    if w <= 0 || h <= 0 {
        return;
    }
    let mut body = None;
    for yy in y..y + h {
        for xx in x..x + w {
            let Some(p) = surf_px(s, xx, yy) else { continue };
            if p[0] < 35 && p[1] > 50 && p[1] < 140 {
                body = Some(p);
                break;
            }
        }
        if body.is_some() {
            break;
        }
    }
    let body = body.unwrap_or([12, 70, 68]);
    s.fill_rect(x, y, w as u32, h as u32, [body[0], body[1], body[2], 255]);
}

fn surf_px(s: &Surface, x: i32, y: i32) -> Option<[u8; 3]> {
    if x < 0 || y < 0 || x >= s.width as i32 || y >= s.height as i32 {
        return None;
    }
    let i = ((y as u32 * s.width + x as u32) * 4) as usize;
    Some([s.pixels[i], s.pixels[i + 1], s.pixels[i + 2]])
}

fn put_px(s: &mut Surface, x: i32, y: i32, c: [u8; 3]) {
    if x < 0 || y < 0 || x >= s.width as i32 || y >= s.height as i32 {
        return;
    }
    let i = ((y as u32 * s.width + x as u32) * 4) as usize;
    s.pixels[i] = c[0];
    s.pixels[i + 1] = c[1];
    s.pixels[i + 2] = c[2];
}

struct MenuText {
    ww: i32,
    wh: i32,
    page: usize,
    /// Window-sized label layer, then one full-width row per help line, then
    /// the idle guide caption.
    buf: Vec<u8>,
    help_h: u32,
    help_n: usize,
}

fn build_menu_text(page: usize, ww: i32, wh: i32) -> Option<MenuText> {
    let help_h = menu_px(18, wh);
    let helps = help_lines(page);
    let extra = (helps.len() as u32 + 1) * help_h;
    let mut stamps = Vec::new();
    let mapx = |x: i32| x * ww / 1280;
    let mapy = |y: i32| y * wh / 720;
    let mapw = |x: i32, w: i32| (x + w) * ww / 1280 - x * ww / 1280;
    let maph = |y: i32, h: i32| (y + h) * wh / 720 - y * wh / 720;
    for lab in page_labs(page).iter().chain(chrome_labs().iter()) {
        let size = menu_px(lab.size, wh);
        let (bx, by, bw, bh) = (mapx(lab.x), mapy(lab.y), mapw(lab.x, lab.w), maph(lab.y, lab.h));
        // Tracking is authored in layout units and scaled with the window,
        // like every other measurement on this screen.
        let track = lab.tracking * wh / 720;
        // English under the Chinese (Window, Full Screen, …) is not drawn.
        // A second line is kept only when it is itself Chinese, such as 音量OFF.
        let sub_is_cjk = lab.sub.chars().any(|c| ('\u{3400}'..='\u{9fff}').contains(&c));
        if lab.sub.is_empty() || !sub_is_cjk {
            stamps.push(crate::render::text::Label {
                text: lab.text.to_string(),
                x: bx,
                y: by,
                w: bw,
                h: bh,
                size,
                center: lab.center,
                rgb: lab.color,
                tracking: track,
            });
        } else {
            // Main line on five eighths, sub line on the rest with a fixed gap
            // between them, so a two-line button does not drift with its height.
            let gap = (bh / 8).max(4);
            let top = ((bh - gap) * 5 / 8).max(1);
            let sub_h = (bh - top - gap).max(1);
            stamps.push(crate::render::text::Label {
                text: lab.text.to_string(),
                x: bx,
                y: by,
                w: bw,
                h: top,
                size,
                center: true,
                rgb: lab.color,
                tracking: track,
            });
            stamps.push(crate::render::text::Label {
                text: lab.sub.to_string(),
                x: bx,
                y: by + top + gap,
                w: bw,
                h: sub_h,
                size: menu_px(lab.sub_size, wh),
                center: true,
                rgb: lab.sub_color,
                tracking: 0,
            });
        }
    }
    // The help sheet (sys_0304X) already carries this text in Chinese, so
    // nothing is stamped over it. Drawing vector text on top produced two
    // different typefaces stacked on the same line.
    stamps.push(crate::render::text::Label {
        text: "向导信息".to_string(),
        x: mapx(GUIDE_X),
        y: wh + helps.len() as i32 * help_h as i32,
        w: 220,
        h: help_h as i32,
        size: menu_px(14, wh),
        center: false,
        rgb: ink::SUB,
        tracking: 0,
    });
    // One canvas for the whole page: the labels are re-tinted per frame, so
    // they have to come back as a buffer with the glyph coverage in alpha.
    let first = stamps.first().map(|l| l.size).unwrap_or(14);
    let buf = crate::render::text::with_ui_font(first, |f| {
        f.labels_rgba(ww as u32, wh as u32 + extra, &stamps)
    })??;
    Some(MenuText { ww, wh, page, buf, help_h, help_n: helps.len() })
}

/// Blit a label canvas with the colours it was built with.
///
/// The canvas is rendered once per page and window size, so the per-element
/// inks - pale teal for a heading, muted cyan-grey for a scale, white for a
/// button - travel in its own RGB channels. A single tint on the way out would
/// flatten all of them back to one colour, which is what made the page look
/// like it was assembled from two different screens.
fn blit_ink(surf: &mut Surface, rgba: &[u8], w: u32, h: u32, x: i32, y: i32) {
    for row in 0..h as i32 {
        for col in 0..w as i32 {
            let i = ((row as u32 * w + col as u32) * 4) as usize;
            if i + 3 >= rgba.len() {
                return;
            }
            let px = [rgba[i], rgba[i + 1], rgba[i + 2], rgba[i + 3]];
            if px[3] == 0 {
                continue;
            }
            surf.blend_pixel(x + col, y + row, px);
        }
    }
}

fn paint_menu_text(frame: &mut Surface, text: &MenuText, help_row: Option<usize>) {
    let ww = text.ww as u32;
    let wh = text.wh.max(0) as u32;
    let main = (wh as usize) * (ww as usize) * 4;
    if text.buf.len() >= main {
        blit_ink(frame, &text.buf[..main], ww, wh, 0, 0);
    }
    let row_bytes = (text.help_h as usize) * (ww as usize) * 4;
    let idx = match help_row {
        Some(r) if r < text.help_n => r,
        _ => text.help_n,
    };
    let start = main + idx * row_bytes;
    let end = start + row_bytes;
    if end <= text.buf.len() {
        let y = if help_row.is_some() {
            HELP_Y * text.wh / 720
        } else {
            GUIDE_Y * text.wh / 720
        };
        blit_ink(frame, &text.buf[start..end], ww, text.help_h, 0, y);
    }
}

// ---------------------------------------------------------------------------
// font rows on page 1
// ---------------------------------------------------------------------------

/// Page y of font row i.
fn font_row_y(i: usize) -> i32 {
    FROW_Y0 + i as i32 * FROW_PITCH
}

/// The rasterized text of the font block: one canvas for the three rows, so a
/// redraw that only moved the cursor does not re-stamp six labels.
struct FontRows {
    sig: String,
    w: u32,
    h: u32,
    buf: Vec<u8>,
}

fn font_rows_sig(st: &Settings, scr: &Screen) -> String {
    format!(
        "{}|{}|{}|{:?}|{:?}|{}",
        st.font_game, st.font_history, st.font_home, scr.font_hover, scr.font_sel, st.dlg_size
    )
}

/// Name of a catalogue entry, or the fallback note when the font it named is
/// no longer installed.
fn font_name(id: i32) -> String {
    match crate::render::text::font_profile(id) {
        Some(p) if p.path.is_some() => p.name.clone(),
        Some(p) => format!("{}（未安装）", p.name),
        None => "默认（未安装）".to_string(),
    }
}

fn build_font_rows(st: &Settings, scr: &Screen, ww: i32, wh: i32) -> Option<FontRows> {
    let px = |v: i32| (v * ww) / 1280;
    let py = |v: i32| (v * wh) / 720;
    let w = px(PAGE_W).max(1) as u32;
    let h = py(FONT_BLOCK_H).max(1) as u32;
    let mut labels: Vec<crate::render::text::Label> = Vec::new();
    let ids = [st.font_game, st.font_history, st.font_home];
    for (i, role) in crate::render::text::FONT_ROLES.iter().enumerate() {
        let y = font_row_y(i);
        let on = scr.font_sel == i || scr.font_hover == Some(i);
        labels.push(crate::render::text::Label {
            text: role.label().to_string(),
            x: px(FROW_LABEL_X),
            y: py(y),
            w: px(FROW_LABEL_W),
            h: py(FROW_H),
            size: menu_px(19, wh),
            center: false,
            rgb: if on { ink::HEAD } else { ink::LABEL },
            tracking: 0,
        });
        labels.push(crate::render::text::Label {
            text: font_name(ids[i]),
            x: px(FROW_BOX_X),
            y: py(y),
            w: px(FROW_BOX_W),
            h: py(FROW_H),
            size: menu_px(17, wh),
            center: true,
            rgb: ink::BUTTON,
            tracking: 0,
        });
        for (ax, ch) in [(FROW_ARROW_L, "<"), (FROW_ARROW_R, ">")] {
            labels.push(crate::render::text::Label {
                text: ch.to_string(),
                x: px(ax),
                y: py(y),
                w: px(FROW_ARROW_W),
                h: py(FROW_H),
                size: menu_px(17, wh),
                center: true,
                rgb: ink::SCALE,
                tracking: 0,
            });
        }
        if i == 0 {
            labels.push(crate::render::text::Label {
                text: format!("样式 {}px", st.dlg_size),
                x: px(FROW_STYLE_X),
                y: py(y),
                w: px(FROW_STYLE_W),
                h: py(FROW_H),
                size: menu_px(15, wh),
                center: true,
                rgb: ink::SUB,
                tracking: 0,
            });
        }
    }
    let size = menu_px(19, wh);
    let buf = crate::render::text::with_role_font(crate::render::text::FontRole::Home, size, |f| {
        f.labels_rgba(w, h, &labels)
    })??;
    Some(FontRows { sig: font_rows_sig(st, scr), w, h, buf })
}

/// Plate for one control: dark at rest, a cyan wash under the cursor.
fn flat_plate(frame: &mut Surface, x: i32, y: i32, w: i32, h: i32, hovered: bool, on: bool, border: [u8; 4]) {
    let body = if hovered {
        [36, 176, 196, 255]
    } else if on {
        [26, 62, 74, 255]
    } else {
        [20, 26, 34, 255]
    };
    frame.fill_rect(x, y, w.max(0) as u32, h.max(0) as u32, body);
    frame.fill_rect(x, y, w.max(0) as u32, 1, border);
    frame.fill_rect(x, y + h - 1, w.max(0) as u32, 1, border);
    frame.fill_rect(x, y, 1, h.max(0) as u32, border);
    frame.fill_rect(x + w - 1, y, 1, h.max(0) as u32, border);
}

/// The row itself is hover index i; its two arrows and the style button live
/// past the three rows in the same field.
const FROW_ARROW_HIT: usize = 3;
const FONT_STYLE_ROW: usize = 9;

/// Screen-space rects of the parts of the font block that can be clicked, in
/// page coordinates: arrows first so they win over the row behind them.
fn font_row_hits() -> Vec<(i32, i32, i32, i32, usize)> {
    let mut v = Vec::new();
    for i in 0..3 {
        let y = font_row_y(i);
        v.push((FROW_ARROW_L, y, FROW_ARROW_W, FROW_H, FROW_ARROW_HIT + i * 2));
        v.push((FROW_ARROW_R, y, FROW_ARROW_W, FROW_H, FROW_ARROW_HIT + i * 2 + 1));
        if i == 0 {
            v.push((FROW_STYLE_X, y, FROW_STYLE_W, FROW_H, FONT_STYLE_ROW));
        }
        v.push((FROW_LABEL_X, y, FROW_BOX_X + FROW_BOX_W - FROW_LABEL_X, FROW_H, i));
    }
    v
}

/// What clicking a part of the font block does. Fonts apply as they are
/// picked, the way every other control on this page does.
enum FontHit {
    /// Select row i without changing it.
    Row(usize),
    /// Step row i's font.
    Step(usize, i32),
    /// Open the dialogue style panel.
    Style,
}

fn font_hit_kind(h: usize) -> Option<FontHit> {
    Some(match h {
        FONT_STYLE_ROW => FontHit::Style,
        i if i >= FROW_ARROW_HIT => {
            let a = i - FROW_ARROW_HIT;
            FontHit::Step(a / 2, if a % 2 == 0 { -1 } else { 1 })
        }
        i if i < 3 => FontHit::Row(i),
        _ => return None,
    })
}

/// Move one role's font on. Returns true when it changed.
fn step_role_font(st: &mut Settings, row: usize, dir: i32) -> bool {
    let Some(role) = crate::render::text::FONT_ROLES.get(row).copied() else {
        return false;
    };
    let slot = match role {
        crate::render::text::FontRole::Game => &mut st.font_game,
        crate::render::text::FontRole::History => &mut st.font_history,
        crate::render::text::FontRole::Home => &mut st.font_home,
    };
    let next = crate::render::text::cycle_profile_id(*slot, dir);
    if next == *slot {
        return false;
    }
    *slot = next;
    true
}

/// Draw the three font rows into the band above the artwork's own rows.
fn paint_font_rows(
    frame: &mut Surface,
    ww: i32,
    wh: i32,
    st: &Settings,
    scr: &Screen,
    rows: Option<&FontRows>,
) {
    let px = |v: i32| (v * ww) / 1280;
    let py = |v: i32| (v * wh) / 720;
    let ox = px(PAGE_X);
    let oy = py(PAGE_Y);
    let dim = [64, 96, 108, 255];
    let lit = [96, 196, 212, 255];
    for i in 0..3 {
        let y = font_row_y(i);
        let on = scr.font_sel == i || scr.font_hover == Some(i);
        flat_plate(
            frame,
            ox + px(FROW_BOX_X),
            oy + py(y),
            px(FROW_BOX_W),
            py(FROW_H),
            scr.font_hover == Some(i),
            on,
            if on { lit } else { dim },
        );
        for (k, ax) in [FROW_ARROW_L, FROW_ARROW_R].iter().enumerate() {
            flat_plate(
                frame,
                ox + px(*ax),
                oy + py(y),
                px(FROW_ARROW_W),
                py(FROW_H),
                scr.font_hover == Some(FROW_ARROW_HIT + i * 2 + k),
                false,
                dim,
            );
        }
        if i == 0 {
            let sh = scr.font_hover == Some(FONT_STYLE_ROW);
            flat_plate(
                frame,
                ox + px(FROW_STYLE_X),
                oy + py(y),
                px(FROW_STYLE_W),
                py(FROW_H),
                sh,
                false,
                if sh { lit } else { dim },
            );
        }
    }
    if let Some(r) = rows {
        blit_ink(frame, &r.buf, r.w, r.h, ox, oy);
    }
    let _ = st;
}

// ---------------------------------------------------------------------------
// dialogue style panel
// ---------------------------------------------------------------------------

const SP_W: i32 = 600;
const SP_H: i32 = 296;
const SP_X: i32 = (W as i32 - SP_W) / 2;
const SP_Y: i32 = (H as i32 - SP_H) / 2;
const SP_ROW_Y: i32 = SP_Y + 66;
const SP_ROW_PITCH: i32 = 44;
const SP_ROW_H: i32 = 32;
const SP_LABEL_X: i32 = SP_X + 36;
const SP_LABEL_W: i32 = 150;
const SP_BOX_X: i32 = SP_X + 200;
const SP_BOX_W: i32 = 150;
const SP_ARROW_L: i32 = SP_BOX_X + SP_BOX_W + 8;
const SP_ARROW_R: i32 = SP_ARROW_L + 34;
const SP_ARROW_W: i32 = 30;
const SP_BTN_Y: i32 = SP_Y + SP_H - 52;
const SP_BTN_H: i32 = 32;
const SP_RESET_X: i32 = SP_X + 36;
const SP_CLOSE_X: i32 = SP_X + SP_W - 36 - 110;

/// Widget indices: four rows each with a left and a right arrow, then the two
/// buttons.
const SP_PREV0: usize = 0;
const SP_NEXT0: usize = 4;
const SP_RESET: usize = 8;
const SP_CLOSE: usize = 9;

/// The dialogue style being edited.
struct StylePanel {
    sel: usize,
    hover: Option<usize>,
    style: crate::render::text::DialogStyle,
}

impl StylePanel {
    fn new(st: &Settings) -> StylePanel {
        StylePanel { sel: 0, hover: None, style: dialog_style(st) }
    }

    fn sig(&self) -> String {
        format!("{:?}|{:?}|{}", self.style, self.hover, self.sel)
    }

    /// Step the value of row i, wrapping.
    fn step(&mut self, i: usize, dir: i32) {
        use crate::render::text::{DIALOG_COLORS, DIALOG_EDGE_MAX, DIALOG_SIZE_MAX, DIALOG_SIZE_MIN};
        match i {
            0 => {
                let n = DIALOG_SIZE_MAX - DIALOG_SIZE_MIN + 1;
                let cur = self.style.size - DIALOG_SIZE_MIN;
                let next = (cur as i32 + dir + n as i32) % n as i32;
                self.style.size = DIALOG_SIZE_MIN + next as u32;
            }
            1 => {
                let n = DIALOG_EDGE_MAX as i32 + 1;
                let next = (self.style.edge as i32 + dir + n) % n;
                self.style.edge = next as u32;
            }
            2 => {
                let n = DIALOG_COLORS.len() as i32;
                let next = (self.style.color as i32 + dir + n) % n;
                self.style.color = next as u32;
            }
            _ => self.style.underline = !self.style.underline,
        }
    }
}

fn style_panel_hits() -> Vec<(i32, i32, i32, i32, usize)> {
    let mut v = Vec::new();
    for i in 0..4 {
        let y = SP_ROW_Y + i as i32 * SP_ROW_PITCH;
        v.push((SP_ARROW_L, y, SP_ARROW_W, SP_ROW_H, SP_PREV0 + i));
        v.push((SP_ARROW_R, y, SP_ARROW_W, SP_ROW_H, SP_NEXT0 + i));
        v.push((SP_LABEL_X, y, SP_BOX_X + SP_BOX_W - SP_LABEL_X, SP_ROW_H, i));
    }
    v.push((SP_RESET_X, SP_BTN_Y, 130, SP_BTN_H, SP_RESET));
    v.push((SP_CLOSE_X, SP_BTN_Y, 110, SP_BTN_H, SP_CLOSE));
    v
}

/// What the panel decided this frame.
enum StyleAct {
    None,
    Close,
    /// Write the style back to the settings.
    Apply(crate::render::text::DialogStyle),
}

fn style_input(p: &mut StylePanel, input: sdl2::Input, win: &Sdl) -> StyleAct {
    match input {
        sdl2::Input::Up => p.sel = (p.sel + 3) % 4,
        sdl2::Input::Down => p.sel = (p.sel + 1) % 4,
        sdl2::Input::Right => p.step(p.sel, 1),
        sdl2::Input::Left => p.step(p.sel, -1),
        sdl2::Input::Advance => return StyleAct::Apply(p.style),
        sdl2::Input::Back | sdl2::Input::RightClick | sdl2::Input::Quit => return StyleAct::Close,
        sdl2::Input::Pointer { x, y, pressed } => {
            let (sx, sy) = win.surface_pos(x, y, W, H);
            let w = style_panel_hits()
                .iter()
                .find(|(hx, hy, hw, hh, _)| sx >= *hx && sx < *hx + *hw && sy >= *hy && sy < *hy + *hh)
                .map(|r| r.4);
            p.hover = w;
            if let (Some(w), true) = (w, pressed) {
                match w {
                    i if (SP_PREV0..SP_PREV0 + 4).contains(&i) => {
                        p.sel = i - SP_PREV0;
                        p.step(p.sel, -1);
                    }
                    i if (SP_NEXT0..SP_NEXT0 + 4).contains(&i) => {
                        p.sel = i - SP_NEXT0;
                        p.step(p.sel, 1);
                    }
                    i if i < 4 => {
                        p.sel = i;
                        p.step(i, 1);
                    }
                    SP_RESET => {
                        p.style = crate::render::text::DialogStyle::default_style();
                        return StyleAct::Apply(p.style);
                    }
                    SP_CLOSE => return StyleAct::Close,
                    _ => {}
                }
            }
        }
        _ => {}
    }
    StyleAct::None
}

struct StyleText {
    sig: String,
    w: u32,
    h: u32,
    buf: Vec<u8>,
}

fn build_style_text(p: &StylePanel, ww: i32, wh: i32) -> Option<StyleText> {
    use crate::render::text::{DIALOG_COLOR_NAMES, DIALOG_COLORS};
    let px = |v: i32| (v * ww) / 1280;
    let py = |v: i32| (v * wh) / 720;
    let w = px(SP_W).max(1) as u32;
    let h = py(SP_H).max(1) as u32;
    let names = ["字号", "描边", "颜色", "下划线"];
    let values = [
        format!("{} px", p.style.size),
        if p.style.edge == 0 {
            "自动".to_string()
        } else {
            format!("{} px", p.style.edge)
        },
        DIALOG_COLOR_NAMES[(p.style.color as usize).min(DIALOG_COLORS.len() - 1)].to_string(),
        if p.style.underline { "开".to_string() } else { "关".to_string() },
    ];
    let mut labels = Vec::new();
    labels.push(crate::render::text::Label {
        text: "对话文本样式".to_string(),
        x: 0,
        y: py(20),
        w: w as i32,
        h: py(28),
        size: menu_px(22, wh),
        center: true,
        rgb: ink::HEAD,
        tracking: 0,
    });
    for i in 0..4 {
        let y = SP_ROW_Y + i as i32 * SP_ROW_PITCH;
        let on = p.sel == i;
        labels.push(crate::render::text::Label {
            text: names[i].to_string(),
            x: px(SP_LABEL_X - SP_X),
            y: py(y - SP_Y),
            w: px(SP_LABEL_W),
            h: py(SP_ROW_H),
            size: menu_px(18, wh),
            center: false,
            rgb: if on { ink::HEAD } else { ink::LABEL },
            tracking: 0,
        });
        labels.push(crate::render::text::Label {
            text: values[i].clone(),
            x: px(SP_BOX_X - SP_X),
            y: py(y - SP_Y),
            w: px(SP_BOX_W),
            h: py(SP_ROW_H),
            size: menu_px(17, wh),
            center: true,
            rgb: ink::BUTTON,
            tracking: 0,
        });
        for (ax, ch) in [(SP_ARROW_L, "<"), (SP_ARROW_R, ">")] {
            labels.push(crate::render::text::Label {
                text: ch.to_string(),
                x: px(ax - SP_X),
                y: py(y - SP_Y),
                w: px(SP_ARROW_W),
                h: py(SP_ROW_H),
                size: menu_px(17, wh),
                center: true,
                rgb: ink::SCALE,
                tracking: 0,
            });
        }
    }
    for (text, bx, bw) in [("恢复默认", SP_RESET_X, 130), ("关闭", SP_CLOSE_X, 110)] {
        labels.push(crate::render::text::Label {
            text: text.to_string(),
            x: px(bx - SP_X),
            y: py(SP_BTN_Y - SP_Y),
            w: px(bw),
            h: py(SP_BTN_H),
            size: menu_px(17, wh),
            center: true,
            rgb: ink::LABEL,
            tracking: 0,
        });
    }
    let size = menu_px(22, wh);
    let buf = crate::render::text::with_role_font(crate::render::text::FontRole::Home, size, |f| {
        f.labels_rgba(w, h, &labels)
    })??;
    Some(StyleText { sig: p.sig(), w, h, buf })
}

fn paint_style_panel(frame: &mut Surface, ww: i32, wh: i32, p: &StylePanel, text: Option<&StyleText>) {
    use crate::render::text::DIALOG_COLORS;
    let px = |v: i32| (v * ww) / 1280;
    let py = |v: i32| (v * wh) / 720;
    frame.fill_rect(0, 0, ww as u32, wh as u32, [0, 0, 0, 170]);
    let (x, y, w, h) = (px(SP_X), py(SP_Y), px(SP_W), py(SP_H));
    frame.fill_rect(x, y, w as u32, h as u32, [14, 18, 26, 246]);
    let rim = [96, 196, 212, 255];
    frame.fill_rect(x, y, w as u32, 1, rim);
    frame.fill_rect(x, y + h - 1, w as u32, 1, rim);
    frame.fill_rect(x, y, 1, h as u32, rim);
    frame.fill_rect(x + w - 1, y, 1, h as u32, rim);
    let dim = [64, 96, 108, 255];
    for i in 0..4 {
        let ry = SP_ROW_Y + i as i32 * SP_ROW_PITCH;
        let on = p.sel == i;
        flat_plate(
            frame,
            px(SP_BOX_X),
            py(ry),
            px(SP_BOX_W),
            py(SP_ROW_H),
            p.hover == Some(i),
            on,
            if on { rim } else { dim },
        );
        flat_plate(
            frame,
            px(SP_ARROW_L),
            py(ry),
            px(SP_ARROW_W),
            py(SP_ROW_H),
            p.hover == Some(SP_PREV0 + i),
            false,
            dim,
        );
        flat_plate(
            frame,
            px(SP_ARROW_R),
            py(ry),
            px(SP_ARROW_W),
            py(SP_ROW_H),
            p.hover == Some(SP_NEXT0 + i),
            false,
            dim,
        );
    }
    flat_plate(
        frame,
        px(SP_RESET_X),
        py(SP_BTN_Y),
        px(130),
        py(SP_BTN_H),
        p.hover == Some(SP_RESET),
        false,
        dim,
    );
    flat_plate(
        frame,
        px(SP_CLOSE_X),
        py(SP_BTN_Y),
        px(110),
        py(SP_BTN_H),
        p.hover == Some(SP_CLOSE),
        false,
        dim,
    );
    // A swatch of the colour next to its name, so the four rows read as one
    // thing being edited rather than four numbers.
    let sw = DIALOG_COLORS[(p.style.color as usize).min(DIALOG_COLORS.len() - 1)];
    let sy = SP_ROW_Y + 2 * SP_ROW_PITCH;
    frame.fill_rect(
        px(SP_BOX_X) - px(20),
        py(sy) + py(8),
        px(14).max(1) as u32,
        py(SP_ROW_H - 16).max(1) as u32,
        [sw[0], sw[1], sw[2], 255],
    );
    if let Some(t) = text {
        blit_ink(frame, &t.buf, t.w, t.h, x, y);
    }
}

/// Run the option screen until the player leaves it.
///
/// `mx` is used for the little sample sound behind the volume rows; when no
/// mixer is available those buttons just log.
pub fn run(win: &mut Sdl, dirs: &[&Path], st: &mut Settings, mx: Option<&crate::audio::mixer::Mixer>) {
    let assets = Assets::load(dirs);
    if !assets.available() {
        eprintln!("menu: option screen artwork missing, cannot open");
        return;
    }
    let mut scr = Screen { page: 0, hover: None, help_row: None, nav_hover: None, font_hover: None, font_sel: 0 };
    let mut dirty = false;
    let cn = using_cn_art(dirs);
    let mut menu_text: Option<MenuText> = None;
    let mut shown: Option<(String, Surface)> = None;
    let mut font_rows: Option<FontRows> = None;
    let mut style_panel: Option<StylePanel> = None;
    let mut style_text: Option<StyleText> = None;
    // Verification runs: open the style panel without a mouse.
    if std::env::var_os("WA2_FONT_PANEL").is_some() {
        style_panel = Some(StylePanel::new(st));
    }

    loop {
        let mut activate: Option<Act> = None;
        // Where the click happened, for the slider tracks.
        let mut click_at: (i32, i32) = (0, 0);
        let input = win.poll_input();
        // The style panel is modal: while it is open it owns the keyboard
        // and the page underneath stops seeing clicks.
        let mut applied: Option<crate::render::text::DialogStyle> = None;
        if let Some(p) = style_panel.as_mut() {
            match style_input(p, input, win) {
                StyleAct::None => {}
                StyleAct::Apply(s) => applied = Some(s),
                StyleAct::Close => style_panel = None,
            }
        }
        if let Some(s) = applied {
            st.dlg_size = s.size as i32;
            st.dlg_edge = s.edge as i32;
            st.dlg_color = s.color as i32;
            st.dlg_under = if s.underline { 1 } else { 0 };
            if let Err(e) = st.save() {
                eprintln!("settings: could not write {}: {}", Settings::path().display(), e);
            }
            st.apply_text();
            apply(st, mx);
            menu_text = None;
            font_rows = None;
        }
        // While the panel is open the page underneath sees nothing at all.
        match if style_panel.is_some() { sdl2::Input::None } else { input } {
            sdl2::Input::Quit => {
                win.set_fullscreen(st.win_disp == 1);
                break;
            }
            sdl2::Input::Up => {
                scr.help_row = Some(match scr.help_row {
                    Some(r) if r > 0 => r - 1,
                    _ => 0,
                });
            }
            sdl2::Input::Down => {
                scr.help_row = Some(scr.help_row.map(|r| r + 1).unwrap_or(0));
            }
            sdl2::Input::Choice(k) => {
                if let Some(p) = (1..=3).contains(&k).then(|| k as usize - 1) {
                    activate = Some(Act::Tab(p));
                }
            }
            sdl2::Input::Drag { x, y } => {
                // Dragging a slider updates it continuously.
                let (sx, sy) = win.surface_pos(x, y, W, H);
                if let Some(sl) = track_at(scr.page, sx, sy) {
                    let span = (sl.x1 - sl.x0).max(1);
                    let rel = (sx - (PAGE_X + sl.x0)).clamp(0, span);
                    st.set(sl.key, (rel * LEVELS + span / 2) / span);
                    dirty = true;
                }
            }
            sdl2::Input::Back | sdl2::Input::Left => activate = Some(Act::Close),
            sdl2::Input::Advance | sdl2::Input::Right => {
                // Left and right step the font the row cursor is on, which
                // is what the arrows drawn on screen do.
                activate = Some(Act::FontStep(scr.font_sel, 1));
            }
            sdl2::Input::Pointer { x, y, pressed } => {
                let (sx, sy) = win.surface_pos(x, y, W, H);
                let hots = page_hots(scr.page);
                let h = hit(scr.page, &hots, sx, sy);
                scr.hover = h.filter(|i| !is_nav(*i));
                scr.nav_hover = h.filter(|i| is_nav(*i));
                if let Some(i) = h {
                    if pressed {
                        activate = if is_nav(i) { nav_act(i) } else { Some(hots[i].act) };
                        click_at = (sx, sy);
                    }
                }
                // The font block owns the strip above the artwork rows, so
                // it is checked on its own.
                scr.font_hover = None;
                if scr.page == 0 {
                    if let Some(which) = font_row_hits()
                        .iter()
                        .find(|(hx, hy, hw, hh, _)| {
                            let (x, y) = (PAGE_X + hx, PAGE_Y + hy);
                            sx >= x && sx < x + hw && sy >= y && sy < y + hh
                        })
                        .map(|(_, _, _, _, w)| *w)
                    {
                        scr.font_hover = Some(which);
                        if pressed {
                            match font_hit_kind(which) {
                                Some(FontHit::Row(i)) => scr.font_sel = i,
                                Some(FontHit::Step(i, d)) =>
                                    activate = Some(Act::FontStep(i, d)),
                                Some(FontHit::Style) =>
                                    activate = Some(Act::FontStyle),
                                None => {}
                            }
                        }
                    }
                }
                // Clicking empty space does nothing: leaving the screen is the
                // job of the left arrow key or Escape.
            }
            _ => {}
        }

        if let Some(a) = activate {
            if a == Act::Close {
                win.set_fullscreen(st.win_disp == 1);
                break;
            }
            match a {
                Act::Opt(k, v) => {
                    st.set(k, v);
                    dirty = true;
                    // Apply while the page is still open, so the gold frame and
                    // the window change together.
                    if k == "win_disp" {
                        win.set_fullscreen(st.win_disp == 1);
                    }
                }
                Act::Step(k, d) => {
                    let cur = st.get(k).unwrap_or(0);
                    st.set(k, cur + d);
                    dirty = true;
                }
                Act::Track => {
                    // The value follows where the player clicked on the bar.
                    let sl = page_hots(scr.page).iter().find_map(|h| {
                        if !matches!(h.act, Act::Track) {
                            return None;
                        }
                        let (x, y) = (PAGE_X + h.x, page_y(scr.page, h.y));
                        if click_at.0 >= x && click_at.0 < x + h.w && click_at.1 >= y && click_at.1 < y + h.h {
                            h.slider
                        } else {
                            None
                        }
                    });
                    if let Some(sl) = sl {
                        let span = (sl.x1 - sl.x0).max(1);
                        let rel = (click_at.0 - (PAGE_X + sl.x0)).clamp(0, span);
                        let lv = ((rel * LEVELS) + span / 2) / span;
                        st.set(sl.key, lv);
                        dirty = true;
                    }
                }
                Act::Off(k) => {
                    st.set(k, 0);
                    dirty = true;
                }
                Act::Test => {
                    let g_se = st.gain(crate::audio::mixer::CAT_SE);
                    if let Some(mx) = mx {
                        let path = dirs
                            .iter()
                            .map(|d| d.join("SE_9702.OGG"))
                            .find(|p| p.is_file());
                        if let Some(p) = path {
                            mx.play_cat(
                                &p.to_string_lossy(),
                                g_se,
                                false,
                                crate::audio::mixer::CAT_SE,
                            );
                        }
                    }
                }
                Act::FontStep(row, d) => {
                    if step_role_font(st, row, d) {
                        dirty = true;
                        // The page captions were rasterised with the old
                        // menu font, so they have to be built again.
                        menu_text = None;
                        font_rows = None;
                    }
                }
                Act::FontStyle => {
                    style_panel = Some(StylePanel::new(st));
                    style_text = None;
                }
                Act::Reset => {
                    *st = Settings::default();
                    dirty = true;
                    win.set_fullscreen(false);
                }
                Act::Tab(p) => {
                    if std::env::var_os("WA2_DEBUG_INPUT").is_some() {
                        eprintln!("menu: tab -> page {}", p);
                    }
                    scr.page = p.min(2);
                    scr.hover = None;
                    scr.help_row = None;
                }
                Act::Page(d) => {
                    let p = scr.page as i32 + d;
                    scr.page = p.clamp(0, 2) as usize;
                    scr.hover = None;
                    scr.help_row = None;
                }
                Act::Close => break,
            }
        }
        // Flush whenever anything changed - dragging a slider sets `dirty`
        // without producing an `activate`.
        if dirty {
            if let Err(e) = st.save() {
                eprintln!("settings: could not write {}: {}", Settings::path().display(), e);
            }
            apply(st, mx);
            dirty = false;
        }

        let (ww, wh) = win.window_size();
        let ww = ww.max(1);
        let wh = wh.max(1);
        let sig = format!(
            "{:?}|{:?}|{:?}|{:?}|{}|{}|{}",
            scr.page,
            scr.hover,
            scr.help_row,
            st,
            ww,
            wh,
            style_panel.as_ref().map(|p| p.sig()).unwrap_or_default()
                + &font_rows.as_ref().map(|r| r.sig.clone()).unwrap_or_default()
        );
        let reuse = shown.as_ref().map(|(k, _)| k == &sig).unwrap_or(false);
        if !reuse {
            if cn {
                let stale = menu_text
                    .as_ref()
                    .map(|t| t.ww != ww || t.wh != wh || t.page != scr.page)
                    .unwrap_or(true);
                if stale {
                    menu_text = build_menu_text(scr.page, ww, wh);
                }
            }
            let mut s = Surface::new(W, H, [0, 0, 0, 255]);
            compose(&mut s, &assets, st, &scr);
            if cn && menu_text.is_some() {
                s.fill_rect(GUIDE_X, guide_y(scr.page), GUIDE_W as u32, GUIDE_H as u32, [0, 0, 0, 255]);
                // Only the page art needs the wipe, and only when it has not
                // been baked already: with a clean page in the cache this is
                // the one branch that does no pixel work at all.
                if assets.clean[scr.page].is_none() {
                    if let Some(base) = assets.base[scr.page].as_ref() {
                        wipe_baked_captions(&mut s, base, scr.page);
                    }
                }
                wipe_chrome_captions(&mut s);
            }
            let mut frame = if ww as u32 == W && wh as u32 == H {
                s
            } else {
                s.scaled_to(ww as u32, wh as u32)
            };
            if let Some(text) = menu_text.as_ref() {
                paint_menu_text(&mut frame, text, scr.help_row);
            }
            // The font rows and the style panel are the two things on this
            // screen the page artwork does not provide, so they are drawn
            // last, over everything the artwork owns.
            if scr.page == 0 {
                let stale = font_rows
                    .as_ref()
                    .map(|r| r.sig != font_rows_sig(st, &scr))
                    .unwrap_or(true);
                if stale {
                    font_rows = build_font_rows(st, &scr, ww, wh);
                }
                paint_font_rows(&mut frame, ww, wh, st, &scr, font_rows.as_ref());
            }
            if let Some(p) = style_panel.as_ref() {
                let pw = (SP_W * ww) / 1280;
                let ph = (SP_H * wh) / 720;
                let stale = style_text
                    .as_ref()
                    .map(|t| t.sig != p.sig() || t.w as i32 != pw || t.h as i32 != ph)
                    .unwrap_or(true);
                if stale {
                    style_text = build_style_text(p, ww, wh);
                }
                paint_style_panel(&mut frame, ww, wh, p, style_text.as_ref());
            }
            // `WA2_MENU_DUMP` writes the frame the player sees, labels included.
            // The write goes to a temporary file and is renamed, so a reader
            // never sees a half-written image.
            if let Ok(path) = std::env::var("WA2_MENU_DUMP") {
                let changed = DUMP_SIG.lock().map(|g| *g != sig).unwrap_or(true);
                if changed {
                    let tmp = format!("{}.tmp", path);
                    if frame.save_tga(&tmp).is_ok() {
                        let _ = std::fs::rename(&tmp, &path);
                    }
                    if let Ok(mut g) = DUMP_SIG.lock() {
                        *g = sig.clone();
                    }
                }
            }
            shown = Some((sig, frame));
        }
        if let Some((_, frame)) = shown.as_ref() {
            win.present(frame);
        }
        win.delay(16);
    }
}

/// Push the settings the rest of the engine can honour right now.
///
/// The mixer applies `gain[ALL] * gain[category]` to every stream, so the four
/// sliders map straight onto it.
pub fn apply(st: &Settings, mx: Option<&crate::audio::mixer::Mixer>) {
    use crate::audio::mixer::{CAT_ALL, CAT_BGM, CAT_SE, CAT_VOICE};
    let g = |v: i32| v.clamp(0, LEVELS) as f32 / LEVELS as f32;
    st.apply_text();
    if let Some(mx) = mx {
        mx.set_gain(CAT_ALL, g(st.vol[0]));
        mx.set_gain(CAT_BGM, g(st.vol[1]));
        mx.set_gain(CAT_SE, g(st.vol[2]));
        mx.set_gain(CAT_VOICE, g(st.vol[3]));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The three font keys have to survive a save/load round trip, and an id
    /// that is not in the catalogue must fall back to the default rather than
    /// name a font file that does not exist.
    #[test]
    fn font_keys_round_trip_and_reject_unknown_ids() {
        let mut st = Settings::default();
        assert_eq!((st.get("font_home"), st.get("font_history"), st.get("font_game")), (Some(0), Some(0), Some(0)));
        // A real catalogue id has to stick, and one this machine does not
        // have has to fall back to the default rather than to a file that
        // is not there.
        let Some(real) = crate::render::text::font_catalog()
            .iter()
            .find(|p| p.id != 0 && p.path.is_some())
        else {
            return
        };
        st.set("font_game", real.id);
        assert_eq!(st.font_game, real.id);
        st.set("font_home", 99);
        crate::render::text::set_role_ids([st.font_home, 0, 0]);
        assert_eq!(crate::render::text::role_ids()[0], 0, "an unknown id must not stick");
        // Every role's key is one the settings file writes, or a choice made
        // in the panel would be gone after a restart.
        let keys = Settings::all_keys();
        for role in crate::render::text::FONT_ROLES {
            assert!(keys.iter().any(|k| k == role.ini_key()), "{} is not saved", role.ini_key());
        }
    }

    /// 字体设置 opens the panel; the entry is on the text page and its rect has
    /// to sit inside the page art.
    #[test]
    #[test]
    fn clean_page_needs_no_wipe() {
        let dirs = [
            Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let mut assets = Assets::load(&dirs);
        let Some(base) = assets.base[0].as_ref() else { return };
        // Build the clean page from the same art, so the test does not depend
        // on the derived cache being generated in this checkout.
        let baked = bake_clean_page(base, 0);
        assets.clean = [
            Some(TgaImage {
                width: baked.width,
                height: baked.height,
                rgba: baked.pixels.clone(),
            }),
            None,
            None,
        ];
        let st = Settings::default();
        let scr = Screen { page: 0, hover: None, help_row: None, nav_hover: None, font_hover: None, font_sel: 0 };
        let mut s = Surface::new(W, H, [0, 0, 0, 255]);
        compose(&mut s, &assets, &st, &scr);
        let at = |s: &Surface, x: u32, y: u32| {
            let i = ((y * s.width + x) * 4) as usize;
            s.pixels[i] as u32 + s.pixels[i + 1] as u32 + s.pixels[i + 2] as u32
        };
        // The same caption area the fallback test inspects: after a clean
        // compose there must be no bright baked glyph left on the left plate.
        let mut bright = 0u32;
        for y in (PAGE_Y as u32 + 20)..(PAGE_Y as u32 + 50) {
            for x in (PAGE_X as u32 + 40)..(PAGE_X as u32 + 280) {
                let sum = at(&s, x, y);
                if sum > 400 {
                    bright += 1;
                }
            }
        }
        assert_eq!(bright, 0, "clean page still shows the baked caption");
    }

    #[test]
    fn every_label_stays_inside_its_box() {
        let Some(f) = crate::render::text::VectorFont::open(
            crate::render::text::DEFAULT_FONT,
            crate::render::DIALOG_FONT_720,
        ) else {
            return;
        };
        let mut checked = 0usize;
        for page in 0..3 {
            for lab in page_labs(page).iter().chain(chrome_labs().iter()) {
                if lab.text.is_empty() {
                    continue;
                }
                assert!(f.set_size(lab.size), "size {} refused", lab.size);
                let m = f.metrics();
                let line = m.ascender - m.descender;
                let (above, below) = f.ink_bounds(lab.text);
                // Same rule labels_rgba uses.
                let baseline = if line <= lab.h {
                    lab.y + (lab.h - line) / 2 + m.ascender
                } else {
                    lab.y + (lab.h - (above + below)) / 2 + above
                };
                let top = baseline - above;
                let bottom = baseline + below;
                assert!(
                    top >= lab.y - 2,
                    "page {page} {:?}: ink top {top} above box {} (baseline {baseline})",
                    lab.text,
                    lab.y
                );
                assert!(
                    bottom <= lab.y + lab.h + 2,
                    "page {page} {:?}: ink bottom {bottom} below box {} (h {})",
                    lab.text,
                    lab.y + lab.h,
                    lab.h
                );
                let w = f.measure_tracked(lab.text, lab.tracking);
                assert!(
                    w <= lab.w,
                    "page {page} {:?}: width {w} over box {}",
                    lab.text,
                    lab.w
                );
                checked += 1;
            }
        }
        assert!(checked > 60, "only {checked} labels checked");
    }

    /// Tracking is real letter spacing, not spaces in the string: the label
    /// text itself must stay free of padding characters, and the rendered
    /// width must grow by exactly tracking * (chars - 1).
    /// The bake is the clean base: no caption ink may survive in the label
    /// column, while the heading bars and section rules must.
    #[test]
    fn baked_page_drops_captions_keeps_structure() {
        let dirs = [
            Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let assets = Assets::load(&dirs);
        let Some(base) = assets.base[0].as_ref() else { return };
        let baked = bake_clean_page(base, 0);
        // 1) Caption column: white-ish ink is gone (white captions would be
        //    ~700 summed; the structures are dim teal, under 300).
        // The ON/OFF pills in that strip keep their baked captions on purpose;
        // their white rims would otherwise count as ink.
        let pills: Vec<(i32, i32, i32, i32)> = page_hots(0)
            .iter()
            .filter(|h| matches!(h.act, Act::Off(_)))
            .map(|h| (h.x - 6, h.y - 6, h.x + h.w + 6, h.y + h.h + 6))
            .collect();
        let mut ink = 0u32;
        for y in 0..PAGE_H as u32 {
            for x in 8..478u32 {
                if pills
                    .iter()
                    .any(|&(x0, y0, x1, y1)| {
                        let (px, py) = (x as i32, y as i32);
                        px >= x0 && px < x1 && py >= y0 && py < y1
                    })
                {
                    continue;
                }
                let i = ((y * baked.width + x) * 4) as usize;
                let p = [baked.pixels[i], baked.pixels[i + 1], baked.pixels[i + 2]];
                let sum = p[0] as u32 + p[1] as u32 + p[2] as u32;
                if sum > 400 && !dim_teal_structure(p) {
                    ink += 1;
                }
            }
        }
        assert_eq!(ink, 0, "baked label column still has caption ink ({ink} px)");
        // 2) The heading bar next to 画面设置 survives.
        let bar = |x: u32, y: u32| {
            let i = ((y * baked.width + x) * 4) as usize;
            dim_teal_structure([baked.pixels[i], baked.pixels[i + 1], baked.pixels[i + 2]])
        };
        let bars = (0..140u32).filter(|&y| bar(28, y)).count();
        assert!(bars > 10, "heading bar was eaten ({bars} px)");
        // 3) A section rule survives: the one right of 画面设置.
        let rules = (300..1000u32).filter(|&x| bar(x, 28)).count();
        assert!(rules > 100, "section rule was eaten ({rules} px)");
    }

    #[test]
    fn tracking_widens_without_polluting_the_string() {
        let Some(f) = crate::render::text::VectorFont::open(
            crate::render::text::DEFAULT_FONT,
            18,
        ) else {
            return;
        };
        let labs = page_labs(0);
        let head = labs.iter().find(|l| l.text == "画面设置").expect("group head");
        assert_eq!(head.tracking, 1, "headings get a little air");
        assert!(!head.text.contains(' '), "no spaces in the label text");
        let plain = f.measure("画面设置");
        let tracked = f.measure_tracked("画面设置", head.tracking);
        assert_eq!(tracked, plain + head.tracking * ("画面设置".chars().count() as i32 - 1));
        // A left label is not letter-spaced: at 22px with a space between every
        // glyph it read looser than the button text it labels.
        let row = labs.iter().find(|l| l.text == "画面模式").expect("row label");
        assert_eq!(row.tracking, 0, "row labels stay tight");
    }

    /// The fallback path: with no baked clean page (a data tree without the
    /// cache), the runtime still has to remove the original captions from the
    /// composed page. The normal path - blitting a clean page - is covered by
    /// `clean_page_needs_no_wipe`.
    #[test]
    fn baked_page_captions_are_replaced_by_the_background() {
        let dirs = [
            Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let mut assets = Assets::load(&dirs);
        // Force the old path even if the cache exists in this checkout.
        assets.clean = [None, None, None];
        let base = assets.base[0].as_ref().expect("page art");
        let st = Settings::default();
        let scr = Screen { page: 0, hover: None, help_row: None, nav_hover: None, font_hover: None, font_sel: 0 };
        let mut s = Surface::new(W, H, [0, 0, 0, 255]);
        compose(&mut s, &assets, &st, &scr);
        // A stroke of 「画面模式」, page (140, 32).
        let at = |s: &Surface, x: u32, y: u32| {
            let i = ((y * s.width + x) * 4) as usize;
            s.pixels[i] as u32 + s.pixels[i + 1] as u32 + s.pixels[i + 2] as u32
        };
        // Page 1 starts with the font block now, so the artwork's own rows sit
        // FONT_BLOCK_H lower.
        let top = (PAGE_Y + page_shift(0)) as u32;
        let mut found = None;
        for y in top..(top + 50) {
            for x in (PAGE_X as u32 + 40)..(PAGE_X as u32 + 280) {
                let sum = at(&s, x, y);
                if sum > 400 {
                    found = Some((x, y, sum));
                    break;
                }
            }
            if found.is_some() {
                break;
            }
        }
        let (gx, gy, before) = found.expect("no bright caption pixel on the left");
        assert!(before > 400, "expected a baked glyph, sum {before}");
        wipe_baked_captions(&mut s, base, 0);
        let after = at(&s, gx, gy);
        assert!(after < 80, "glyph should be gone, sum {after}");
        let mut leftover = 0u32;
        let mut worst = (0u32, 0u32, 0u32, [0u8; 3]);
        for y in (PAGE_Y as u32 + 20)..(PAGE_Y as u32 + 50) {
            for x in (PAGE_X as u32 + 40)..(PAGE_X as u32 + 250) {
                let i = ((y * s.width + x) * 4) as usize;
                let p = [s.pixels[i], s.pixels[i + 1], s.pixels[i + 2]];
                if structural_teal(p) {
                    continue;
                }
                let sum = p[0] as u32 + p[1] as u32 + p[2] as u32;
                if sum > 80 {
                    leftover += 1;
                    if sum > worst.0 {
                        worst = (sum, x, y, p);
                    }
                }
            }
        }
        assert!(
            leftover < 40,
            "caption halo still visible: {leftover} px, worst {worst:?}"
        );
        // The button frame stays white, and the caption inside it does not.
        let frame = at(&s, (PAGE_X as u32) + 465, (page_y(0, 0) + 16) as u32);
        assert!(frame > 600, "frame was eaten, sum {frame}");
        let mut btn_left = 0u32;
        for y in (PAGE_Y as u32 + 22)..(PAGE_Y as u32 + 40) {
            for x in (PAGE_X as u32 + 560)..(PAGE_X as u32 + 640) {
                if at(&s, x, y) > 500 {
                    btn_left += 1;
                }
            }
        }
        assert!(btn_left < 24, "button caption still bright: {btn_left} px");
        wipe_chrome_captions(&mut s);
        // The tab strip is blitted untouched: its baked English captions match
        // the labels the engine wants, so the idle tab must still show them.
        let margin = at(&s, (TABS_X + TAB_W + 16) as u32, (TABS_Y + 16) as u32);
        let mut tab_ink = 0u32;
        for y in (TABS_Y as u32 + 12)..(TABS_Y as u32 + 26) {
            for x in (TABS_X as u32 + TAB_W as u32 + 20)..(TABS_X as u32 + TAB_W as u32 * 2 - 20) {
                if at(&s, x, y) > margin + 50 {
                    tab_ink += 1;
                }
            }
        }
        assert!(tab_ink > 20, "idle tab lost its baked caption: {tab_ink} px");
    }

    #[test]
    fn auto_hold_follows_the_slider_and_the_line() {
        let fast = auto_hold_ms(0, 20);
        let slow = auto_hold_ms(LEVELS, 20);
        assert!(slow > fast * 2, "slow {slow} should be well above fast {fast}");
        assert!(auto_hold_ms(5, 40) > auto_hold_ms(5, 8));
        assert!(fast >= 200);
    }

    #[test]
    fn selected_option_is_filled_not_just_an_edge() {
        let dirs = [
            Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let assets = Assets::load(&dirs);
        assert!(assets.available());
        let mut st = Settings::default();
        st.text_speed = 1;
        let scr = Screen { page: 0, hover: None, help_row: None, nav_hover: None, font_hover: None, font_sel: 0 };
        let mut surf = Surface::new(W, H, [0, 0, 0, 255]);
        compose(&mut surf, &assets, &st, &scr);
        let sample = |x: i32, y: i32| {
            let i = ((y as u32 * W + x as u32) * 4) as usize;
            (surf.pixels[i], surf.pixels[i + 1], surf.pixels[i + 2])
        };
        // Centres of 「瞬间显示」 and the selected 「高速」.
        // Centres of the idle and the selected text-speed button. Page 1 sits
        // FONT_BLOCK_H lower now, and the artwork coordinates are page ones.
        let y = page_y(0, 72) + 22;
        let idle = sample(PAGE_X + 458 + 62, y);
        let picked = sample(PAGE_X + 586 + 62, y);
        assert!(
            picked.2 > idle.2 + 15 && picked.1 > picked.0,
            "selected body should be cyan, idle {:?} picked {:?}",
            idle,
            picked
        );
        let _ = std::fs::create_dir_all("/tmp/wa2btn");
        let _ = surf.save_tga("/tmp/wa2btn/menu_selected.tga");
    }

    #[test]
    /// Page 1 grew for the font block, so every widget the artwork placed on it
    /// has to move with it - and nothing on pages 2 and 3 may move.
    #[test]
    fn only_page_one_moves_for_the_font_block() {
        for page in 0..3 {
            let hot = page_hots(page)[0];
            assert_eq!(page_y(page, hot.y), PAGE_Y + hot.y + page_shift(page));
        }
        assert!(page_shift(0) >= 3 * FROW_PITCH, "the font block needs room for three rows");
        assert_eq!(page_shift(1), 0);
        assert_eq!(page_shift(2), 0);
        // The help line has to stay under the last row of its page, not end up
        // in the middle of them.
        let bottom = page_y(0, PAGE_Y as i32 - 4);
        assert!(help_y(0) > PAGE_Y + 400, "page 1 grew, so the help line has to with it");
        assert_eq!(help_y(1), HELP_Y);
    }

    /// The three font rows have to fit in the band the artwork gave up, and the
    /// dialogue row has to come first, because it is the one that carries the
    /// style entry.
    #[test]
    fn font_rows_fit_and_are_ordered() {
        for (i, role) in crate::render::text::FONT_ROLES.iter().enumerate() {
            let y = font_row_y(i);
            assert!(y >= 0 && y + FROW_H <= FONT_BLOCK_H, "row {} overflows the band", i);
        }
        assert_eq!(font_row_y(2) + FROW_H <= FONT_BLOCK_H, true);
        let hits = font_row_hits();
        assert!(hits.iter().any(|(_, _, _, _, w)| font_hit_kind(*w).map_or(false, |k| matches!(k, FontHit::Style))));
        // Every row offers both arrows.
        for i in 0..3 {
            let has = |dir: i32| hits.iter().any(|(_, _, _, _, w)| matches!(font_hit_kind(*w), Some(FontHit::Step(r, d)) if r == i && d == dir));
            assert!(has(-1) && has(1), "row {} is missing an arrow", i);
        }
    }

    /// Stepping a font moves only that role, and wraps around the catalogue.
    #[test]
    fn stepping_a_font_moves_only_that_role() {
        let mut st = Settings::default();
        st.font_game = 0;
        st.font_history = 0;
        st.font_home = 0;
        assert!(step_role_font(&mut st, 0, 1));
        assert_ne!(st.font_game, 0, "the game font should have moved");
        assert_eq!(st.font_history, 0);
        assert_eq!(st.font_home, 0, "the UI font must not follow the game one");
        assert!(step_role_font(&mut st, 0, -1));
        assert_eq!(st.font_game, 0, "stepping back has to return");
        let last = crate::render::text::font_profile_count() as i32 - 1;
        st.font_home = last;
        assert!(step_role_font(&mut st, 2, 1));
    }

    /// The dialogue style has to survive a round trip through the settings file
    /// and has to be clamped, because it is pixels and an index rather than a
    /// slider level.
    #[test]
    fn dialogue_style_round_trips_and_clamps() {
        let mut st = Settings::default();
        st.set("dlg_size", 30);
        st.set("dlg_edge", 2);
        st.set("dlg_color", 5);
        st.set("dlg_under", 1);
        assert_eq!((st.dlg_size, st.dlg_edge, st.dlg_color, st.dlg_under), (30, 2, 5, 1));
        let style = dialog_style(&st);
        assert_eq!(style, crate::render::text::DialogStyle { size: 30, edge: 2, color: 5, underline: true });
        // A hand-edited value is stored as it is and ranged where it is read,
        // the same way the slider keys clamp on the way out.
        st.set("dlg_size", 999);
        assert_eq!(st.dlg_size, 999);
        assert_eq!(dialog_style(&st).size, crate::render::text::DIALOG_SIZE_MAX, "must clamp on read");
        st.set("dlg_edge", 9);
        assert_eq!(dialog_style(&st).edge, crate::render::text::DIALOG_EDGE_MAX, "must clamp on read");
        for k in ["dlg_size", "dlg_edge", "dlg_color", "dlg_under"] {
            assert!(Settings::all_keys().iter().any(|s| s == k), "{} is not saved", k);
        }
    }

    /// The catalogue has to list the machine's fonts rather than a fixed
    /// handful, and every entry has to be able to draw the script.
    #[test]
    fn catalogue_lists_the_machines_fonts() {
        let cat = crate::render::text::font_catalog();
        assert_eq!(cat[0].id, 0, "the default entry must be id 0, so old configs work");
        assert!(cat.iter().all(|p| p.path.as_ref().map_or(false, |x| std::path::Path::new(x).is_file())));
        let mut checked = 0;
        for p in cat.iter() {
            let Some(path) = p.path.as_ref() else { continue };
            let mut ft = match crate::ft2::Ft::open(path, 24, p.face) {
                Some(f) => f,
                None => continue,
            };
            // A font the settings page offers but that cannot draw Chinese
            // would show tofu in the middle of every line.
            assert!(ft.glyph('汉').is_some(), "{} cannot draw Chinese", p.name);
            assert!(ft.glyph('あ').is_some(), "{} cannot draw kana", p.name);
            checked += 1;
        }
        assert!(checked > 0, "no font could be opened");
        // Ids are unique, or one config value would name two fonts.
        let mut ids: Vec<i32> = cat.iter().map(|p| p.id).collect();
        ids.sort();
        ids.dedup();
        assert_eq!(ids.len(), cat.len());
    }
}
