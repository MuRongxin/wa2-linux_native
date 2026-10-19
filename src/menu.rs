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
const ARROW_W: i32 = 48;
const NAV_H: i32 = 64;
const BAR_SRC_X: i32 = 144;
const BAR_W: i32 = 432 - BAR_SRC_X;
const NAV_PAD: i32 = 40;
const DEF_GAP: i32 = 56;
const DEF_W: i32 = 208;
const DEF_H: i32 = 24;
const NAV_CLUSTER: i32 = ARROW_W + NAV_PAD + BAR_W + NAV_PAD + ARROW_W + DEF_GAP + DEF_W;
const NAV_X: i32 = (W as i32 - NAV_CLUSTER) / 2;
const NAV_Y: i32 = 608;
const NAV_BAR_X: i32 = NAV_X + ARROW_W + NAV_PAD;
const NAV_RIGHT_X: i32 = NAV_BAR_X + BAR_W + NAV_PAD;
const DEF_X: i32 = NAV_RIGHT_X + ARROW_W + DEF_GAP;
const DEF_Y: i32 = NAV_Y + (NAV_H - DEF_H) / 2;

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
    if let Some(b) = a.clean[page].as_ref() {
        s.blit(b, PAGE_X, PAGE_Y);
    } else if let Some(b) = a.base[page].as_ref() {
        s.blit(b, PAGE_X, PAGE_Y);
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
            s.blit_region(src, sl.x0, sl.y, w as u32, BAR_H as u32, PAGE_X + sl.x0, PAGE_Y + sl.y);
        }
    }
    for (i, h) in hots.iter().enumerate() {
        let style = styles[i];
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
                        s.blit_region(src, sl.x0, sl.y, w as u32, BAR_H as u32, PAGE_X + sl.x0, PAGE_Y + sl.y);
                    }
                    continue;
                }
            }
            s.blit_region(src, h.x.max(0), h.y.max(0), h.w as u32, h.h as u32, PAGE_X + h.x, PAGE_Y + h.y);
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
                fill_option_body(s, PAGE_X + h.x, PAGE_Y + h.y, h.w, h.h, tint, alpha);
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

    // Help line for the row under the cursor.
    let hr = scr.help_row.or_else(|| hover.map(|i| hots[i].row));
    if let (Some(img), Some(row)) = (a.help[page].as_ref(), hr) {
        let y = (row as i32 * HELP_H).min(img.height as i32 - HELP_H).max(0);
        s.fill_rect(GUIDE_X, GUIDE_Y, GUIDE_W as u32, GUIDE_H as u32, [0, 0, 0, 255]);
        s.blit_region(img, 0, y, HELP_W as u32, HELP_H as u32, HELP_X, HELP_Y);
    }

    // One left arrow, the label strip, one right arrow. Column 0 is idle,
    // column 1 is the arrow under the cursor. The third column is the pressed
    // frame and stays in the sheet.
    if let Some(n) = a.nav.as_ref() {
        let left = if scr.nav_hover == Some(NAV_PREV) { 1 } else { 0 };
        let right = if scr.nav_hover == Some(NAV_NEXT) { 1 } else { 0 };
        // The sheet's top half points left and the bottom half points right.
        s.blit_region(n, left * ARROW_W, 0, ARROW_W as u32, NAV_H as u32, NAV_X, NAV_Y);
        s.blit_region(n, BAR_SRC_X, 0, BAR_W as u32, NAV_H as u32, NAV_BAR_X, NAV_Y);
        s.blit_region(n, right * ARROW_W, NAV_H, ARROW_W as u32, NAV_H as u32, NAV_RIGHT_X, NAV_Y);
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
    // Left arrow and 「上一页」, the OPTION label, 「下一页」 and the right arrow.
    if sy >= NAV_Y && sy < NAV_Y + NAV_H {
        if sx >= NAV_X && sx < NAV_X + ARROW_W {
            return Some(NAV_PREV);
        }
        if sx >= NAV_RIGHT_X && sx < NAV_RIGHT_X + ARROW_W {
            return Some(NAV_NEXT);
        }
        if sx >= NAV_BAR_X && sx < NAV_BAR_X + BAR_W {
            let rel = sx - NAV_BAR_X;
            return Some(if rel < 100 {
                NAV_PREV
            } else if rel < 210 {
                NAV_MID
            } else {
                NAV_NEXT
            });
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
        NAV_PREV => Some(Act::Page(-1)),
        NAV_MID => Some(Act::Close),
        NAV_NEXT => Some(Act::Page(1)),
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

/// Group heading: 18px, a touch of tracking, the pale teal the section rules
/// are drawn in.
fn head(x: i32, y: i32, w: i32, text: &'static str) -> Lab {
    let mut l = lab(PAGE_X + x, PAGE_Y + y, w, 20, text, "", 18, false, 0, false);
    l.color = ink::HEAD;
    l.tracking = 1;
    l
}

/// Left-hand label of a row. 20px, no tracking: at 22 with a space between
/// every glyph it read looser than the button text it labels.
fn row(x: i32, y: i32, w: i32, h: i32, text: &'static str, sub: &'static str) -> Lab {
    let mut l = lab(PAGE_X + x, PAGE_Y + y, w, h, text, sub, 20, false, 0, false);
    l.color = ink::LABEL;
    l
}

/// Parenthetical note under a label.
fn note(x: i32, y: i32, w: i32, text: &'static str) -> Lab {
    let mut l = lab(PAGE_X + x, PAGE_Y + y, w, 16, text, "", 13, false, 0, false);
    l.color = ink::SUB;
    l
}

fn btn(x: i32, y: i32, w: i32, h: i32, text: &'static str, sub: &'static str) -> Lab {
    let mut l = lab(PAGE_X + x, PAGE_Y + y, w, h, text, sub, 17, true, 7, false);
    l.color = ink::BUTTON;
    l.sub_size = 11;
    l
}

/// Slider scale.
fn tiny(x: i32, y: i32, w: i32, h: i32, text: &'static str) -> Lab {
    let mut l = lab(PAGE_X + x, PAGE_Y + y, w, h, text, "", 10, true, 3, false);
    l.color = ink::SCALE;
    l
}

fn menu_px(layout_px: u32, window_h: i32) -> u32 {
    ((layout_px as i32 * window_h.max(1)) / 720).clamp(8, 64) as u32
}

fn page_labs(page: usize) -> Vec<Lab> {
    let mut v = Vec::new();
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
            v.push(row(40, 254, 220, 22, "窗口透明度", ""));
            v.push(note(48, 278, 180, "（普通场景）"));
            v.push(row(40, 296, 220, 22, "窗口透明度", ""));
            v.push(note(56, 320, 160, "（CG场景）"));
            v.push(row(40, 336, 220, 22, "背景透明度", ""));
            v.push(note(48, 360, 180, "（小说场景）"));
            v.push(row(40, 386, 260, 26, "指令图标透明度", ""));
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
    let mut v = Vec::new();
    v.push(lab(NAV_BAR_X, NAV_Y + 14, 100, 36, "上一页", "", 16, true, 0, false));
    // The gap between 上一页 and 下一页 is BAR_W - 200 = 88px. The old caption
    // needed 135 and simply overlapped both arrows; four characters fit.
    v.push(lab(NAV_BAR_X + 100, NAV_Y + 4, BAR_W - 200, 26, "环境设置", "", 15, true, 0, false));
    v.push(lab(NAV_BAR_X + 100, NAV_Y + 36, BAR_W - 200, 22, "OPTION", "", 13, true, 0, false));
    v.push(lab(NAV_BAR_X + BAR_W - 100, NAV_Y + 14, 100, 36, "下一页", "", 16, true, 0, false));
    v
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

fn wipe_baked_captions(s: &mut Surface, base: &TgaImage, page: usize) {
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
            let sy = PAGE_Y + y;
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
                let sy = PAGE_Y + ny;
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
            let sy = PAGE_Y + y;
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

/// Tabs, the bottom bar and the default button are separate sheets. Repaint
/// each caption area from a clean pixel so the baked letters are gone and the
/// face stays a smooth gradient. A per-pixel brightness test leaves ridges.
fn wipe_chrome_captions(s: &mut Surface) {
    // The tab strip already ships its English "OPTION 1/2/3" captions in all
    // three states and they match the labels the engine wants, so it is
    // blitted untouched (repainting it from a column left a flat grey band).
    s.fill_rect(NAV_BAR_X + 2, NAV_Y + 2, (BAR_W - 4) as u32, (NAV_H - 4) as u32, [0, 0, 0, 255]);
    // The default button sheet already ships Chinese (恢复默认设置) with its
    // gear and pill in all three states, so it is blitted as-is; flattening it
    // here used to leave a pale band under the vector caption.
}

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
    let cn = using_cn_art(dirs);
    let mut menu_text: Option<MenuText> = None;
    let mut shown: Option<(String, Surface)> = None;

    loop {
        let mut activate: Option<Act> = None;
        // Where the click happened, for the slider tracks.
        let mut click_at: (i32, i32) = (0, 0);
        match win.poll_input() {
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
            sdl2::Input::Back => activate = Some(Act::Close),
            sdl2::Input::Advance | sdl2::Input::Right => activate = Some(Act::Close),
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
        let sig = format!("{:?}|{:?}|{:?}|{:?}|{}|{}", scr.page, scr.hover, scr.help_row, st, ww, wh);
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
                s.fill_rect(GUIDE_X, GUIDE_Y, GUIDE_W as u32, GUIDE_H as u32, [0, 0, 0, 255]);
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

    /// Every caption has to stay inside the box the page artwork gave it: a
    /// baseline that drifts down puts a label on the rule under it, and one
    /// that drifts up clips it against the row above. This is the check the
    /// layout change has to keep passing, on all three pages.
    /// With a baked clean page present, `compose` must blit it and leave the
    /// page free of original captions before the text layer is stamped. That
    /// is the whole point of the cache: no per-frame pixel classification.
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
        let scr = Screen { page: 0, hover: None, help_row: None, nav_hover: None };
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
        let scr = Screen { page: 0, hover: None, help_row: None, nav_hover: None };
        let mut s = Surface::new(W, H, [0, 0, 0, 255]);
        compose(&mut s, &assets, &st, &scr);
        // A stroke of 「画面模式」, page (140, 32).
        let at = |s: &Surface, x: u32, y: u32| {
            let i = ((y * s.width + x) * 4) as usize;
            s.pixels[i] as u32 + s.pixels[i + 1] as u32 + s.pixels[i + 2] as u32
        };
        let mut found = None;
        for y in (PAGE_Y as u32)..(PAGE_Y as u32 + 50) {
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
        let frame = at(&s, (PAGE_X as u32) + 465, (PAGE_Y as u32) + 16);
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
        let scr = Screen { page: 0, hover: None, help_row: None, nav_hover: None };
        let mut surf = Surface::new(W, H, [0, 0, 0, 255]);
        compose(&mut surf, &assets, &st, &scr);
        let sample = |x: i32, y: i32| {
            let i = ((y as u32 * W + x as u32) * 4) as usize;
            (surf.pixels[i], surf.pixels[i + 1], surf.pixels[i + 2])
        };
        // Centres of 「瞬间显示」 and the selected 「高速」.
        let idle = sample(PAGE_X + 458 + 62, PAGE_Y + 72 + 22);
        let picked = sample(PAGE_X + 586 + 62, PAGE_Y + 72 + 22);
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
    fn nav_arrows_are_one_on_each_side() {
        let dirs = [
            Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let assets = Assets::load(&dirs);
        let st = Settings::default();
        let scr = Screen { page: 0, hover: None, help_row: None, nav_hover: None };
        let mut surf = Surface::new(W, H, [0, 0, 0, 255]);
        compose(&mut surf, &assets, &st, &scr);
        let lit = |x: i32, y: i32| {
            let i = ((y as u32 * W + x as u32) * 4) as usize;
            let s = surf.pixels[i] as u16 + surf.pixels[i + 1] as u16 + surf.pixels[i + 2] as u16;
            s > 80
        };
        let mut arrow = 0;
        let mut gap = 0;
        for y in NAV_Y..NAV_Y + NAV_H {
            for x in NAV_X..NAV_X + ARROW_W {
                if lit(x, y) {
                    arrow += 1;
                }
            }
            for x in NAV_X + ARROW_W..NAV_BAR_X {
                if lit(x, y) {
                    gap += 1;
                }
            }
        }
        assert!(arrow > 200, "left arrow should be drawn, lit pixels {arrow}");
        assert!(gap < 40, "the other arrow states must not sit in the gap, lit {gap}");
        // A left-pointing triangle has its base on the right.
        let mut base = 0;
        let mut tip = 0;
        let mid = NAV_X + ARROW_W / 2;
        for y in NAV_Y..NAV_Y + NAV_H {
            for x in NAV_X..mid {
                if lit(x, y) {
                    tip += 1;
                }
            }
            for x in mid..NAV_X + ARROW_W {
                if lit(x, y) {
                    base += 1;
                }
            }
        }
        assert!(base > tip, "left arrow should point left, base {base} tip {tip}");
        let _ = std::fs::create_dir_all("/tmp/wa2btn");
        let _ = surf.save_tga("/tmp/wa2btn/menu_nav.tga");
    }
}
