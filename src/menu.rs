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
//!   sys_03020               bottom navigation bar, 2 states of 432x64
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

const NAV_W: i32 = 432;
const NAV_H: i32 = 64;
const NAV_X: i32 = 128;
const NAV_Y: i32 = 592;

const DEF_W: i32 = 208;
const DEF_H: i32 = 24;
const DEF_X: i32 = NAV_X + NAV_W + 24;
const DEF_Y: i32 = NAV_Y + 20;

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
            alpha_back: LEVELS,
            alpha_command: LEVELS,
            yes_no: 1,
            r_click: 1,
            vol: [LEVELS, LEVELS, LEVELS, LEVELS],
            page_voice: 1,
            ero_voice: 2,
            char_voice: [1; 10],
        }
    }
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
            _ => return None,
        })
    }

    fn set(&mut self, key: &str, v: i32) {
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
        std::fs::write(&path, out)
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
    gold: [Option<TgaImage>; 3],
    cyan: [Option<TgaImage>; 3],
    header: Option<TgaImage>,
    tabs: Option<TgaImage>,
    nav: Option<TgaImage>,
    defbtn: Option<TgaImage>,
    help: [Option<TgaImage>; 3],
}

fn load(dirs: &[&Path], name: &str) -> Option<TgaImage> {
    for dir in dirs {
        for cand in [name.to_string(), name.to_ascii_lowercase(), name.to_ascii_uppercase()] {
            let p = dir.join(&cand);
            if let Ok(d) = std::fs::read(&p) {
                if let Ok(img) = crate::codec::tga::decode(&d) {
                    return Some(img);
                }
            }
        }
        // case-insensitive scan as a last resort
        if let Ok(rd) = std::fs::read_dir(dir) {
            let want = name.to_ascii_lowercase();
            for e in rd.flatten() {
                if let Ok(n) = e.file_name().into_string() {
                    if n.to_ascii_lowercase() == want {
                        if let Ok(d) = std::fs::read(e.path()) {
                            if let Ok(img) = crate::codec::tga::decode(&d) {
                                return Some(img);
                            }
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
        Assets {
            base: [
                load(dirs, "sys_03100.tga"),
                load(dirs, "sys_03101.tga"),
                load(dirs, "sys_03102.tga"),
            ],
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
}

fn draw_page(
    s: &mut Surface,
    a: &Assets,
    page: usize,
    hots: &[Hot],
    st: &Settings,
    styles: &[Style],
) {
    if let Some(b) = a.base[page].as_ref() {
        s.blit(b, PAGE_X, PAGE_Y);
    }
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
        if let (Some(src), Some(w)) = (a.gold[page].as_ref(), fill_width(st, sl)) {
            s.blit_region(src, sl.x0, sl.y, w as u32, BAR_H as u32, PAGE_X + sl.x0, PAGE_Y + sl.y);
        }
    }
    for (i, h) in hots.iter().enumerate() {
        let style = styles[i];
        if style == Style::Base {
            continue;
        }
        let src = match style {
            Style::Gold => a.gold[page].as_ref(),
            Style::Cyan => a.cyan[page].as_ref(),
            Style::Base => None,
        };
        if let Some(src) = src {
            // A hovered bar is drawn from the bright layer instead of the gold
            // one, but still only as far as the value goes, so dragging it keeps
            // showing the level instead of lighting up the whole bar.
            if style == Style::Cyan && matches!(h.act, Act::Track) {
                if let Some(sl) = h.slider {
                    if let Some(w) = fill_width(st, sl) {
                        s.blit_region(src, sl.x0, sl.y, w as u32, BAR_H as u32, PAGE_X + sl.x0, PAGE_Y + sl.y);
                    }
                    continue;
                }
            }
            s.blit_region(src, h.x.max(0), h.y.max(0), h.w as u32, h.h as u32, PAGE_X + h.x, PAGE_Y + h.y);
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

    // Help line for the row under the cursor.
    let hr = scr.help_row.or_else(|| hover.map(|i| hots[i].row));
    if let (Some(img), Some(row)) = (a.help[page].as_ref(), hr) {
        let y = (row as i32 * HELP_H).min(img.height as i32 - HELP_H).max(0);
        s.fill_rect(GUIDE_X, GUIDE_Y, GUIDE_W as u32, GUIDE_H as u32, [0, 0, 0, 255]);
        s.blit_region(img, 0, y, HELP_W as u32, HELP_H as u32, HELP_X, HELP_Y);
    }

    // Navigation bar and the "restore defaults" button.
    if let Some(n) = a.nav.as_ref() {
        // Row 0 carries the labels (「上一页 / OPTION / 下一页」); row 1 is the same
        // bar with the arrow cluster flipped, i.e. an animation frame rather
        // than a hover state. Always draw row 0, and only swap the arrows.
        s.blit_region(n, 0, 0, NAV_W as u32, NAV_H as u32, NAV_X, NAV_Y);
        if scr.nav_hover.is_some() {
            s.blit_region(n, 0, NAV_H, 150, NAV_H as u32, NAV_X, NAV_Y);
        }
    }
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
        let (x, y) = (PAGE_X + h.x, PAGE_Y + h.y);
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
    // navigation bar
    if sy >= NAV_Y && sy < NAV_Y + NAV_H && sx >= NAV_X && sx < NAV_X + NAV_W {
        let rel = sx - NAV_X;
        return Some(if rel < 145 {
            NAV_PREV
        } else if rel < 290 {
            NAV_MID
        } else {
            NAV_NEXT
        });
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
        NAV_PREV => Some(Act::Page(-1)),
        NAV_MID => Some(Act::Close),
        NAV_NEXT => Some(Act::Page(1)),
        NAV_DEF => Some(Act::Reset),
        _ => None,
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
    let mut scr = Screen { page: 0, hover: None, help_row: None, nav_hover: None };
    let mut dirty = false;

    loop {
        let mut activate: Option<Act> = None;
        // Where the click happened, for the slider tracks.
        let mut click_at: (i32, i32) = (0, 0);
        match win.poll_input() {
            sdl2::Input::Quit => break,
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
            sdl2::Input::Back => activate = Some(Act::Close),
            sdl2::Input::Advance => activate = Some(Act::Close),
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
                // Clicking empty space does nothing: leaving the screen is the
                // job of the OPTION button in the navigation bar, the left
                // arrow key or Escape.
            }
            _ => {}
        }

        if let Some(a) = activate {
            if a == Act::Close {
                break;
            }
            match a {
                Act::Opt(k, v) => {
                    st.set(k, v);
                    dirty = true;
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
                        let (x, y) = (PAGE_X + h.x, PAGE_Y + h.y);
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
                Act::Reset => {
                    *st = Settings::default();
                    dirty = true;
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

        let mut s = Surface::new(W, H, [0, 0, 0, 255]);
        compose(&mut s, &assets, st, &scr);
        // `WA2_MENU_DUMP` writes the frame whenever the screen state changes,
        // which makes the option screen verifiable without a working X11 screen
        // capture. The write goes to a temporary file and is renamed, so a
        // reader never sees a half-written image.
        if let Ok(path) = std::env::var("WA2_MENU_DUMP") {
            let sig = format!("{:?}|{:?}|{:?}|{:?}", scr.page, scr.hover, scr.help_row, st);
            let changed = DUMP_SIG.lock().map(|g| *g != sig).unwrap_or(true);
            if changed {
                let tmp = format!("{}.tmp", path);
                if s.save_tga(&tmp).is_ok() {
                    let _ = std::fs::rename(&tmp, &path);
                }
                if let Ok(mut g) = DUMP_SIG.lock() {
                    *g = sig;
                }
            }
        }
        win.present(&s);
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

    #[test]
    fn auto_hold_follows_the_slider_and_the_line() {
        let fast = auto_hold_ms(0, 20);
        let slow = auto_hold_ms(LEVELS, 20);
        assert!(slow > fast * 2, "slow {slow} should be well above fast {fast}");
        assert!(auto_hold_ms(5, 40) > auto_hold_ms(5, 8));
        assert!(fast >= 200);
    }
}
