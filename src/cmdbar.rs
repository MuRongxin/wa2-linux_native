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
    Hide,
}

impl Cmd {
    fn is_on(self, active: Active) -> bool {
        match self {
            Cmd::Auto => active.auto,
            Cmd::Skip => active.skip,
            Cmd::Hide => active.hide,
            _ => false,
        }
    }
}

struct Btn {
    cmd: Cmd,
    x: i32,
    /// `sys_00012` row while the toggle is off. Non-toggles use one row.
    label_off: i32,
    /// Row while the toggle is on (`AUTO … ON` / `SKIP … ON`).
    label_on: i32,
}

/// Same order as the source cells and the executable's position table.
const BTNS: [Btn; 9] = [
    Btn { cmd: Cmd::QuickSave, x: 808, label_off: 0, label_on: 0 },
    Btn { cmd: Cmd::QuickLoad, x: 848, label_off: 1, label_on: 1 },
    Btn { cmd: Cmd::Config, x: 928, label_off: 2, label_on: 2 },
    Btn { cmd: Cmd::Save, x: 968, label_off: 3, label_on: 3 },
    Btn { cmd: Cmd::Load, x: 1008, label_off: 4, label_on: 4 },
    Btn { cmd: Cmd::Backlog, x: 1048, label_off: 5, label_on: 5 },
    Btn { cmd: Cmd::Auto, x: 1088, label_off: 7, label_on: 6 },
    Btn { cmd: Cmd::Skip, x: 1128, label_off: 9, label_on: 8 },
    Btn { cmd: Cmd::Hide, x: 1168, label_off: 10, label_on: 10 },
];

pub struct CmdBar {
    icons: Option<TgaImage>,
    labels: Option<TgaImage>,
}

fn load_img(dirs: &[&Path], name: &str) -> Option<TgaImage> {
    for dir in dirs {
        let p = dir.join(name);
        if let Ok(d) = std::fs::read(&p) {
            if let Ok(img) = crate::codec::tga::decode(&d) {
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
        CmdBar {
            icons,
            labels: load_img(dirs, "sys_00012.tga"),
        }
    }

    /// Which command a surface-space point falls on.
    pub fn hit(&self, sx: i32, sy: i32) -> Option<Cmd> {
        if sy < ROW_Y || sy >= ROW_Y + CELL {
            return None;
        }
        BTNS.iter().find(|b| sx >= b.x && sx < b.x + CELL).map(|b| b.cmd)
    }

    /// Draw the icons, and the hovered button's caption just above it.
    pub fn draw(&self, surf: &mut Surface, hover: Option<Cmd>, active: Active) {
        let Some(icons) = self.icons.as_ref() else {
            return;
        };
        for (i, b) in BTNS.iter().enumerate() {
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
            let alpha = if on || hover == Some(b.cmd) { 255 } else { 100 };
            surf.blit_region_alpha(
                icons,
                (i as i32) * CELL,
                row * CELL,
                CELL as u32,
                CELL as u32,
                b.x,
                ROW_Y,
                alpha,
            );
        }
        if let (Some(cmd), Some(labels)) = (hover, self.labels.as_ref()) {
            if let Some(b) = BTNS.iter().find(|b| b.cmd == cmd) {
                let row = if b.cmd.is_on(active) { b.label_on } else { b.label_off };
                // Captions are right-aligned in the 208px strip, so the text
                // ends just above the icon and the empty left side stays clear.
                let x = b.x + CELL - LABEL_W;
                let y = ROW_Y - LABEL_H - 2;
                surf.blit_region(labels, 0, row * LABEL_H, LABEL_W as u32, LABEL_H as u32, x, y);
            }
        }
    }
}

/// On/off state of the toggle buttons.
#[derive(Clone, Copy, Default)]
pub struct Active {
    pub auto: bool,
    pub skip: bool,
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
        // The executable leaves one empty cell between quick-load and config.
        assert_eq!(bar.hit(900, 700), None);
        assert_eq!(bar.hit(928, 680), Some(Cmd::Config));
        assert_eq!(bar.hit(1168, 700), Some(Cmd::Hide));
        assert_eq!(bar.hit(1208, 700), None);
        assert_eq!(bar.hit(1000, 679), None);

        let mut surf = Surface::new(1280, 720, [8, 12, 24, 255]);
        let active = Active { auto: true, skip: false, hide: false };
        bar.draw(&mut surf, Some(Cmd::Save), active);

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
        );
        font.draw_pitch(&mut surf, "春希", crate::render::NAME_X, crate::render::NAME_Y, crate::render::DIALOG_PITCH);
        let dirs = [
            Path::new("/home/lrxin/wa2-native/data/bg_cn"),
            Path::new("/home/lrxin/wa2-native/data/bg"),
        ];
        let bar = CmdBar::load(&dirs);
        bar.draw(&mut surf, Some(Cmd::Save), Active { auto: true, skip: false, hide: false });
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
}
