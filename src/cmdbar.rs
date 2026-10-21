//! In-game function buttons, taken from the original assets.
//!
//! `WA2.exe` places nine icons from `sys_00010.tga` along the bottom of the
//! message window. The sheet is 9 columns by 3 rows of 40×40:
//! idle (cyan), hover (coloured), and active (white outline). Screen
//! positions are the nine `(x, y, w, h)` records at file offset `0x4be3c8`
//! — all at y=680, with a one-cell gap between quick-load and the rest.
//! Hover captions are the 16px rows of `sys_00012.tga` (the Chinese patch
//! ships its own copy, which `bg_dirs` prefers).

use crate::codec::tga::TgaImage;
use crate::render::Surface;
use std::path::Path;

const CELL: i32 = 40;
/// Top of every icon. From the executable's button table.
const ROW_Y: i32 = 680;
const LABEL_W: i32 = 208;
const LABEL_H: i32 = 16;

/// One function button.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cmd {
    QuickSave,
    QuickLoad,
    Config,
    Save,
    Load,
    Backlog,
    Auto,
    Skip,
    /// Fast rewind. Not an original button: it sits in the empty cell and
    /// uses the skip icon mirrored, so it reads as the opposite of 快进.
    Rewind,
    Hide,
    /// Open the story timeline, centred on the scene currently being played.
    Timeline,
}

impl Cmd {
    fn is_on(self, active: Active) -> bool {
        match self {
            Cmd::Auto => active.auto,
            Cmd::Skip => active.skip,
            Cmd::Rewind => active.rewind,
            Cmd::Hide => active.hide,
            _ => false,
        }
    }
}

struct Btn {
    cmd: Cmd,
    x: i32,
    /// Column in `sys_00010`. Rewind has none; it is the skip column, mirrored.
    sheet: Option<i32>,
    /// `sys_00012` row while the toggle is off. Non-toggles use one row.
    label_off: i32,
    /// Row while the toggle is on (`AUTO … ON` / `SKIP … ON`).
    label_on: i32,
}

/// Same order as the executable's position table. Rewind occupies the empty
/// cell the original left between quick-load and config.
const BTNS: [Btn; 11] = [
    // Timeline is deliberately the first visible command so it is easy to
    // discover during play.
    Btn { cmd: Cmd::Timeline, x: 768, sheet: None, label_off: 0, label_on: 0 },
    Btn { cmd: Cmd::QuickSave, x: 808, sheet: Some(0), label_off: 0, label_on: 0 },
    Btn { cmd: Cmd::QuickLoad, x: 848, sheet: Some(1), label_off: 1, label_on: 1 },
    Btn { cmd: Cmd::Rewind, x: 888, sheet: None, label_off: 0, label_on: 0 },
    Btn { cmd: Cmd::Config, x: 928, sheet: Some(2), label_off: 2, label_on: 2 },
    Btn { cmd: Cmd::Save, x: 968, sheet: Some(3), label_off: 3, label_on: 3 },
    Btn { cmd: Cmd::Load, x: 1008, sheet: Some(4), label_off: 4, label_on: 4 },
    Btn { cmd: Cmd::Backlog, x: 1048, sheet: Some(5), label_off: 5, label_on: 5 },
    Btn { cmd: Cmd::Auto, x: 1088, sheet: Some(6), label_off: 7, label_on: 6 },
    Btn { cmd: Cmd::Skip, x: 1128, sheet: Some(7), label_off: 9, label_on: 8 },
    Btn { cmd: Cmd::Hide, x: 1168, sheet: Some(8), label_off: 10, label_on: 10 },
];

pub struct CmdBar {
    icons: Option<TgaImage>,
    labels: Option<TgaImage>,
    /// Source cell sizes.  The command bar is a window-sized overlay, so its
    /// sheets stay at the highest available art scale instead of being
    /// reduced to the 40px/16px logical cache first.
    icon_cell_w: u32,
    icon_cell_h: u32,
    label_w: u32,
    label_h: u32,
    /// Skip column flipped left-to-right, three rows of 40×40.
    rewind: Option<TgaImage>,
    /// Hover caption. The label sheet has no 快退 row, so the vector font
    /// draws it. It is rasterized on the first hover and rebuilt when the font
    /// roles change: the bar lives for a whole game and the player can swap
    /// the UI font in the option screen, so a one-shot bitmap would keep the
    /// old face until a restart. The u64 is the role generation it came from.
    rewind_label: std::cell::RefCell<(u64, Option<(u32, u32, Vec<u8>)>)>,
    rewind_label_size: std::cell::Cell<u32>,
    timeline_label: std::cell::RefCell<(u64, Option<(u32, u32, Vec<u8>)>)>,
    timeline_label_size: std::cell::Cell<u32>,
}

/// Load a command-bar sheet at the highest available source resolution.  The
/// normal UI loader intentionally returns a logical-size image for pixel
/// aligned menus; doing that here would make a 40px icon blurry when the
/// already available 4x/8x sheet is stretched over a fullscreen frame.
fn load_img(dirs: &[&Path], name: &str) -> Option<TgaImage> {
    let mut scaled: Vec<(&Path, u32)> = dirs
        .iter()
        .map(|dir| (*dir, crate::codec::image::dir_art_scale(dir)))
        .filter(|(_, scale)| *scale > 1)
        .collect();
    scaled.sort_by(|a, b| b.1.cmp(&a.1));
    for (dir, _) in scaled.into_iter().chain(dirs.iter().map(|dir| (*dir, 1))) {
        for cand in crate::codec::image::candidate_names(name) {
            let p = dir.join(cand);
            let decoded = if crate::codec::image::dir_art_scale(dir) > 1 {
                crate::codec::image::decode_file(&p)
            } else {
                crate::codec::image::decode_ui_file(&p)
            };
            if let Ok(img) = decoded {
                return Some(img);
            }
        }
    }
    None
}

impl CmdBar {
    pub fn load(dirs: &[&Path]) -> CmdBar {
        let icons = load_img(dirs, "sys_00010.tga");
        if icons.is_none() {
            eprintln!("cmdbar: sys_00010.tga not found");
        }
        let icon_cell_w = icons.as_ref().map(|img| (img.width / 9).max(1)).unwrap_or(CELL as u32);
        let icon_cell_h = icons.as_ref().map(|img| (img.height / 3).max(1)).unwrap_or(CELL as u32);
        let labels = load_img(dirs, "sys_00012.tga");
        let label_scale = labels
            .as_ref()
            .map(|img| (img.width / LABEL_W as u32).max(1))
            .unwrap_or(1);
        let label_w = LABEL_W as u32 * label_scale;
        let label_h = LABEL_H as u32 * label_scale;
        let rewind = icons.as_ref().map(|img| flip_column(img, 7, icon_cell_w, icon_cell_h));
        CmdBar {
            icons,
            labels,
            icon_cell_w,
            icon_cell_h,
            label_w,
            label_h,
            rewind,
            // Rasterized on demand: a game can run for hours without the
            // player ever hovering 快退.
            rewind_label: std::cell::RefCell::new((0, None)),
            rewind_label_size: std::cell::Cell::new(0),
            timeline_label: std::cell::RefCell::new((0, None)),
            timeline_label_size: std::cell::Cell::new(0),
        }
    }

    /// Which command a point falls on, in 720-layout coordinates. Callers
    /// convert pointer events with `Sdl::surface_pos` before asking, so the
    /// bar's rects are the same whether the frame was scaled up or not.
    pub fn hit(&self, x: i32, y: i32) -> Option<Cmd> {
        if y < ROW_Y || y >= ROW_Y + CELL {
            return None;
        }
        BTNS.iter().find(|b| x >= b.x && x < b.x + CELL).map(|b| b.cmd)
    }

    /// Draw the icons, and the hovered button's caption just above it.
    ///
    /// `idle_alpha` is how faint the resting icons are (the original sits
    /// near 100). Hover and the toggled-on state stay at full strength.
    /// `ww`/`wh` is the size of `surf`; the bar's own geometry is 720-layout
    /// pixels and is scaled onto it.
    pub fn draw(
        &self,
        surf: &mut Surface,
        hover: Option<Cmd>,
        active: Active,
        idle_alpha: u8,
        ww: i32,
        wh: i32,
    ) {
        let cx = |v: i32| v * ww.max(1) / 1280;
        let cy = |v: i32| v * wh.max(1) / 720;
        let cw = |v: i32| (v * ww.max(1) / 1280).max(1);
        let ch = |v: i32| (v * wh.max(1) / 720).max(1);
        for b in &BTNS {
            let on = b.cmd.is_on(active);
            let row = if on {
                2
            } else if hover == Some(b.cmd) {
                1
            } else {
                0
            };
            // Idle icons sit faint on the window, the way the original draws
            // them. Hover and the toggled-on state come up to full strength.
            let alpha = if on || hover == Some(b.cmd) { 255 } else { idle_alpha };
            if b.cmd == Cmd::Rewind {
                if let Some(rew) = self.rewind.as_ref() {
                    surf.blit_region_scaled(
                        rew,
                        0,
                        row * self.icon_cell_h as i32,
                        self.icon_cell_w,
                        self.icon_cell_h,
                        cx(b.x),
                        cy(ROW_Y),
                        cw(CELL) as u32,
                        ch(CELL) as u32,
                        alpha,
                    );
                }
            } else if b.cmd == Cmd::Timeline {
                draw_timeline_icon(surf, cx(b.x), cy(ROW_Y), cw(CELL), ch(CELL), alpha, row);
            } else if let (Some(icons), Some(col)) = (self.icons.as_ref(), b.sheet) {
                surf.blit_region_scaled(
                    icons,
                    col * self.icon_cell_w as i32,
                    row * self.icon_cell_h as i32,
                    self.icon_cell_w,
                    self.icon_cell_h,
                    cx(b.x),
                    cy(ROW_Y),
                    cw(CELL) as u32,
                    ch(CELL) as u32,
                    alpha,
                );
            }
        }
        if let Some(cmd) = hover {
            if cmd == Cmd::Rewind {
                let mut cache = self.rewind_label.borrow_mut();
                let generation = crate::render::text::role_generation();
                let label_size = ((16 * wh.max(1) + 360) / 720).max(8) as u32;
                if cache.0 != generation || self.rewind_label_size.get() != label_size {
                    cache.1 = crate::render::text::with_ui_font(label_size, |f| f.line_rgba("快退", label_size)).flatten();
                    cache.0 = generation;
                    self.rewind_label_size.set(label_size);
                }
                if let Some((w, h, rgba)) = cache.1.as_ref() {
                    let b = BTNS.iter().find(|b| b.cmd == Cmd::Rewind).unwrap();
                    let x = cx(b.x + CELL) - *w as i32;
                    let y = cy(ROW_Y - 16 + 6) - *h as i32 / 2;
                    surf.blit_rgba_scaled(rgba, *w, *h, x, y, *w, *h);
                }
            } else if cmd == Cmd::Timeline {
                // Keep a separate cache for the timeline caption so a
                // font-role change cannot leave one command with stale
                // dimensions from the other.
                let mut cache = self.timeline_label.borrow_mut();
                let generation = crate::render::text::role_generation();
                let label_size = ((16 * wh.max(1) + 360) / 720).max(8) as u32;
                if cache.0 != generation || self.timeline_label_size.get() != label_size {
                    cache.1 = crate::render::text::with_ui_font(label_size, |f| f.line_rgba("时间线", label_size)).flatten();
                    cache.0 = generation;
                    self.timeline_label_size.set(label_size);
                }
                if let Some((w, h, rgba)) = cache.1.as_ref() {
                    let b = BTNS.iter().find(|b| b.cmd == cmd).unwrap();
                    let x = cx(b.x + CELL) - *w as i32;
                    let y = cy(ROW_Y - 16 + 6) - *h as i32 / 2;
                    surf.blit_rgba_scaled(rgba, *w, *h, x, y, *w, *h);
                }
            } else if let Some(labels) = self.labels.as_ref() {
                if let Some(b) = BTNS.iter().find(|b| b.cmd == cmd) {
                    let row = if b.cmd.is_on(active) { b.label_on } else { b.label_off };
                    // Captions are right-aligned in the 208px strip, so the text
                    // ends just above the icon and the empty left side stays clear.
                    let x = b.x + CELL - LABEL_W;
                    let y = ROW_Y - LABEL_H - 2;
                    surf.blit_region_scaled(
                        labels,
                        0,
                        row * self.label_h as i32,
                        self.label_w,
                        self.label_h,
                        cx(x),
                        cy(y),
                        cw(LABEL_W) as u32,
                        ch(LABEL_H) as u32,
                        255,
                    );
                }
            }
        }
    }
}

/// Draw the timeline glyph in the one remaining 40px command-bar cell. The
/// original game has no corresponding atlas entry, so a compact three-node
/// graph makes the action legible at both 720p and fullscreen scales.
fn draw_timeline_icon(surf: &mut Surface, x: i32, y: i32, w: i32, h: i32, alpha: u8, row: i32) {
    let sx = w.max(1) as f32 / CELL as f32;
    let sy = h.max(1) as f32 / CELL as f32;
    let px = |v: i32| x + (v as f32 * sx).round() as i32;
    let py = |v: i32| y + (v as f32 * sy).round() as i32;
    let c = if row == 2 { [255, 255, 255, alpha] } else if row == 1 { [255, 214, 90, alpha] } else { [90, 220, 232, alpha] };
    let node = |surf: &mut Surface, nx: i32, ny: i32| {
        for yy in -2..=2 {
            for xx in -2..=2 {
                if xx * xx + yy * yy <= 6 {
                    surf.blend_pixel(px(nx + xx), py(ny + yy), c);
                }
            }
        }
    };
    for i in 0..16 {
        let t = i as f32 / 15.0;
        surf.blend_pixel(px(10 + (18.0 * t) as i32), py(20 - (8.0 * t) as i32), c);
        surf.blend_pixel(px(28 + (0.0 * t) as i32), py(12 + (8.0 * t) as i32), c);
    }
    node(surf, 10, 20);
    node(surf, 28, 12);
    node(surf, 28, 28);
}

/// Skip's column, mirrored, so the double arrow points backward.
fn flip_column(icons: &TgaImage, col: i32, cell_w: u32, cell_h: u32) -> TgaImage {
    let mut rgba = vec![0u8; (cell_w * cell_h * 3 * 4) as usize];
    for row in 0..3 {
        for y in 0..cell_h as i32 {
            for x in 0..cell_w as i32 {
                let sx = col * cell_w as i32 + x;
                let sy = row * cell_h as i32 + y;
                if sx < 0 || sy < 0 || sx >= icons.width as i32 || sy >= icons.height as i32 {
                    continue;
                }
                let si = ((sy as u32 * icons.width + sx as u32) * 4) as usize;
                let di = (((row * cell_h as i32 + y) * cell_w as i32 + (cell_w as i32 - 1 - x)) * 4) as usize;
                if si + 3 < icons.rgba.len() && di + 3 < rgba.len() {
                    rgba[di..di + 4].copy_from_slice(&icons.rgba[si..si + 4]);
                }
            }
        }
    }
    TgaImage { width: cell_w, height: cell_h * 3, rgba }
}

fn blit_gold(surf: &mut Surface, rgba: &[u8], w: u32, h: u32, x: i32, y: i32) {
    for row in 0..h as i32 {
        for col in 0..w as i32 {
            let i = ((row as u32 * w + col as u32) * 4) as usize;
            let a = rgba[i + 3];
            if a == 0 {
                continue;
            }
            surf.blend_pixel(x + col, y + row, [255, 214, 64, a]);
        }
    }
}

/// On/off state of the toggle buttons.
#[derive(Clone, Copy, Default)]
pub struct Active {
    pub auto: bool,
    pub skip: bool,
    pub rewind: bool,
    pub hide: bool,
}

#[cfg(test)]
mod integration {
    use super::*;
    use std::path::Path;

    #[test]
    fn icons_use_the_original_rects() {
        let dirs = [
            Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let bar = CmdBar::load(&dirs);
        assert!(bar.icons.is_some(), "sys_00010.tga did not load");
        assert!(bar.labels.is_some(), "sys_00012.tga did not load");

        assert_eq!(bar.hit(808, 690), Some(Cmd::QuickSave));
        assert_eq!(bar.hit(847, 719), Some(Cmd::QuickSave));
        // The empty cell between quick-load and config is the rewind button.
        assert_eq!(bar.hit(888, 700), Some(Cmd::Rewind));
        assert_eq!(bar.hit(900, 700), Some(Cmd::Rewind));
        assert_eq!(bar.hit(927, 700), Some(Cmd::Rewind));
        assert_eq!(bar.hit(928, 680), Some(Cmd::Config));
        assert_eq!(bar.hit(1168, 700), Some(Cmd::Hide));
        assert_eq!(bar.hit(768, 700), Some(Cmd::Timeline));
        assert_eq!(bar.hit(1208, 700), None);
        assert_eq!(bar.hit(1000, 679), None);

        let mut surf = Surface::new(1280, 720, [8, 12, 24, 255]);
        let active = Active { auto: true, skip: false, rewind: false, hide: false };
        bar.draw(&mut surf, Some(Cmd::Save), active, 100, 1280, 720);

        // Idle quick-save icon is the cyan row.
        let mut cyan = 0u32;
        for y in 684..716 {
            for x in 812..844 {
                let i = ((y * 1280 + x) * 4) as usize;
                if surf.pixels[i + 2] > 60 && surf.pixels[i + 1] > 50 && surf.pixels[i + 3] > 200 {
                    cyan += 1;
                }
            }
        }
        assert!(cyan > 40, "expected the idle icon, cyan pixels {cyan}");

        // Auto is on, so its cell is the white-outline row, not the cyan one.
        let mut white = 0u32;
        for y in 680..700 {
            for x in 1088..1128 {
                let i = ((y * 1280 + x) * 4) as usize;
                if surf.pixels[i] > 230 && surf.pixels[i + 1] > 230 && surf.pixels[i + 2] > 210 && surf.pixels[i + 3] > 200
                {
                    white += 1;
                }
            }
        }
        assert!(white > 10, "expected the active outline, white pixels {white}");

        // SAVE caption sits just above that icon.
        let mut cap = 0u32;
        for y in 650..680 {
            for x in 800..1008 {
                let i = ((y * 1280 + x) * 4) as usize;
                if surf.pixels[i] > 180 && surf.pixels[i + 1] > 200 && surf.pixels[i + 3] > 200 {
                    cap += 1;
                }
            }
        }
        assert!(cap > 20, "expected the hover caption, pixels {cap}");

        let mut rew = 0u32;
        for y in 684..716 {
            for x in 892..924 {
                let i = ((y * 1280 + x) * 4) as usize;
                if surf.pixels[i] != 8 || surf.pixels[i + 1] != 12 || surf.pixels[i + 2] != 24 {
                    rew += 1;
                }
            }
        }
        assert!(rew > 20, "expected the rewind icon, pixels {rew}");

        let mut hover_rew = Surface::new(1280, 720, [8, 12, 24, 255]);
        bar.draw(
            &mut hover_rew,
            Some(Cmd::Rewind),
            Active { auto: false, skip: false, rewind: true, hide: false },
            100,
            1280,
            720,
        );
        let mut cap_rew = 0u32;
        for y in 640..678 {
            for x in 820..928 {
                let i = ((y * 1280 + x) * 4) as usize;
                if hover_rew.pixels[i] > 180 && hover_rew.pixels[i + 1] > 140 && hover_rew.pixels[i + 3] > 180 {
                    cap_rew += 1;
                }
            }
        }
        assert!(cap_rew > 10, "expected the 快退 caption, pixels {cap_rew}");
        let _ = hover_rew.save_tga("/tmp/wa2btn/rewind_hover.tga");

        let _ = std::fs::create_dir_all("/tmp/wa2btn");
        let _ = surf.save_tga("/tmp/wa2btn/cmdbar_preview.tga");
    }

    #[test]
    fn window_text_sits_inside_the_oval() {
        let atlas = crate::codec::tga::decode_font(
            &std::fs::read("/home/lrxin/wa2-native/data/font/cn_body80.tga").unwrap(),
        )
        .unwrap();
        let font = crate::render::Font::from_cell_map(
            atlas,
            40,
            std::path::Path::new("/home/lrxin/wa2-native/data/font/wa2_cell_map_cn.tsv"),
        )
        .unwrap();
        let plate = crate::codec::tga::decode(
            &std::fs::read("/home/lrxin/wa2-native/data/bg/sys_00001.tga").unwrap(),
        )
        .unwrap();
        let frame = crate::codec::tga::decode(
            &std::fs::read("/home/lrxin/wa2-native/data/bg/sys_00000.tga").unwrap(),
        )
        .unwrap();
        let bg = crate::codec::tga::TgaImage {
            width: 1280,
            height: 720,
            rgba: vec![70u8, 110, 150, 255].repeat(1280 * 720),
        };
        let mut surf = crate::render::render_scene_ex(
            &bg,
            "「…这个确实让我不得不同意啊」",
            &font,
            1280,
            720,
            &[],
            None,
            true,
            Some(&plate),
            Some(&frame),
            255,
            0,
            &[],
        );
        font.draw_pitch(&mut surf, "春希", crate::render::NAME_X, crate::render::NAME_Y, crate::render::DIALOG_PITCH);
        let dirs = [
            Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let bar = CmdBar::load(&dirs);
        bar.draw(&mut surf, Some(Cmd::Save), Active { auto: true, skip: false, rewind: false, hide: false }, 100, 1280, 720);
        // A glyph of the line must land inside the oval, not on the bare background.
        let i = ((600 * 1280 + 700) * 4) as usize;
        assert!(
            surf.pixels[i] < 90,
            "text origin should be on the dark plate, rgb {} {} {}",
            surf.pixels[i],
            surf.pixels[i + 1],
            surf.pixels[i + 2]
        );
        let _ = std::fs::create_dir_all("/tmp/wa2btn");
        let _ = surf.save_tga("/tmp/wa2btn/window_preview.tga");
    }

    #[test]
    fn typewriter_and_window_alpha_change_the_frame() {
        let atlas = crate::codec::tga::decode_font(
            &std::fs::read("/home/lrxin/wa2-native/data/font/cn_body80.tga").unwrap(),
        )
        .unwrap();
        let font = crate::render::Font::from_cell_map(
            atlas,
            40,
            std::path::Path::new("/home/lrxin/wa2-native/data/font/wa2_cell_map_cn.tsv"),
        )
        .unwrap();
        let plate = crate::codec::tga::decode(
            &std::fs::read("/home/lrxin/wa2-native/data/bg/sys_00001.tga").unwrap(),
        )
        .unwrap();
        let frame = crate::codec::tga::decode(
            &std::fs::read("/home/lrxin/wa2-native/data/bg/sys_00000.tga").unwrap(),
        )
        .unwrap();
        let bg = crate::codec::tga::TgaImage {
            width: 1280,
            height: 720,
            rgba: vec![70u8, 110, 150, 255].repeat(1280 * 720),
        };
        let line = "春希不我";
        let mut partial = crate::render::render_scene_ex(
            &bg, "", &font, 1280, 720, &[], None, true, Some(&plate), Some(&frame), 255, 0, &[],
        );
        crate::render::draw_dialog_text(&mut partial, &font, line, true, 1280, 720, 1);
        let full = crate::render::render_scene_ex(
            &bg, line, &font, 1280, 720, &[], None, true, Some(&plate), Some(&frame), 255, 0, &[],
        );
        let bare = crate::render::render_scene_ex(
            &bg, line, &font, 1280, 720, &[], None, true, Some(&plate), Some(&frame), 0, 0, &[],
        );
        let ink = |img: &crate::render::Surface| {
            let mut n = 0u32;
            for y in 546..580 {
                for x in 264..1080 {
                    let i = ((y * 1280 + x) * 4) as usize;
                    if img.pixels[i] > 180 && img.pixels[i + 1] > 180 && img.pixels[i + 2] > 180 {
                        n += 1;
                    }
                }
            }
            n
        };
        let one = ink(&partial);
        let all = ink(&full);
        assert!(one > 0, "the first character should leave ink");
        assert!(all > one, "the rest of the line should add ink, {} vs {}", all, one);
        // Alpha 0 leaves the flat backdrop; alpha 255 composites the dark plate.
        let i = ((600 * 1280 + 700) * 4) as usize;
        assert!(full.pixels[i] < bare.pixels[i], "the plate should darken that pixel");
    }

    /// The 快退 caption is rasterized lazily and stamped with the font
    /// generation. The bar outlives the option screen, so without the stamp a
    /// UI-font change would keep the old face over the button for the rest of
    /// the game.
    #[test]
    fn rewind_caption_is_cached_per_font_generation() {
        let dirs = [
            Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let bar = CmdBar::load(&dirs);
        if bar.icons.is_none() {
            return;
        }
        assert!(bar.rewind_label.borrow().1.is_none(), "nothing rasterized before the first hover");
        let mut surf = Surface::new(1280, 720, [8, 12, 24, 255]);
        let active = Active { auto: false, skip: false, rewind: false, hide: false };
        bar.draw(&mut surf, Some(Cmd::Rewind), active, 100, 1280, 720);
        let cached = bar.rewind_label.borrow();
        let Some((w, h, rgba)) = cached.1.as_ref() else {
            // No vector font on this machine: there is nothing to cache.
            return;
        };
        // A parallel test may change the roles between the draw and this
        // line, so only the fact that a generation was stamped is asserted.
        assert!(
            cached.0 > 0 && cached.0 <= crate::render::text::role_generation(),
            "the cache has to carry the font generation it was built from"
        );
        assert!(*w > 0 && *h > 0 && rgba.len() == (*w * *h * 4) as usize, "caption buffer {}x{}", w, h);
    }
}
