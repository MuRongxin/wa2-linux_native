//! Save/load UI, using the shipped 01/02 family (not the 05 CG browser).
//!
//! The ten page tabs and 632x96 panels support a two-column, five-row layout.
//! Destination positions below are a sensible reconstruction, not recovered
//! executable coordinates. Slot identity remains save_00 .. save_99; the shared
//! legacy pagination helpers in save.rs are deliberately not used here.

use crate::codec::tga::TgaImage;
use crate::render::text::{Label, VectorFont};
use crate::render::Surface;
use crate::save::{self, SaveState, SLOT_COUNT};
use crate::sdl2::{self, Sdl};
use std::path::{Path, PathBuf};

const W: u32 = 1280;
const H: u32 = 720;
const PER_PAGE: usize = 10;
const CELL_W: i32 = 632;
const CELL_H: i32 = 96;
const THUMB_W: u32 = 128;
const THUMB_H: u32 = 72;
const TAB_X: i32 = 420;
const TAB_Y: i32 = 592;
const PREV: Rect = Rect(448, 652, 48, 64);
const NEXT: Rect = Rect(784, 652, 48, 64);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Mode { Save, Load }

impl Mode {
    fn index(self) -> usize { if self == Self::Save { 0 } else { 1 } }
    fn name(self) -> &'static str { if self == Self::Save { "save" } else { "load" } }
}

#[derive(Clone, Copy, Debug)]
struct Rect(i32, i32, i32, i32);
impl Rect {
    fn contains(self, x: i32, y: i32) -> bool {
        x >= self.0 && x < self.0 + self.2 && y >= self.1 && y < self.1 + self.3
    }
    fn pixels(self, w: u32, h: u32) -> Self {
        let x = |n: i32| (n as f64 * w as f64 / W as f64).round() as i32;
        let y = |n: i32| (n as f64 * h as f64 / H as f64).round() as i32;
        Self(x(self.0), y(self.1), (x(self.0 + self.2) - x(self.0)).max(1),
             (y(self.1 + self.3) - y(self.1)).max(1))
    }
}

fn page_count() -> usize { SLOT_COUNT.div_ceil(PER_PAGE) }
fn slot_index(page: usize, cell: usize) -> Option<usize> {
    if cell >= PER_PAGE { return None; }
    let idx = page.checked_mul(PER_PAGE)?.checked_add(cell)?;
    (idx < SLOT_COUNT).then_some(idx)
}
fn cells_on_page(page: usize) -> usize {
    SLOT_COUNT.saturating_sub(page.saturating_mul(PER_PAGE)).min(PER_PAGE)
}
fn cell_rect(i: usize) -> Rect {
    Rect(8 + (i % 2) as i32 * CELL_W, 52 + (i / 2) as i32 * 100, CELL_W, CELL_H)
}
fn tab_rect(page: usize) -> Rect { Rect(TAB_X + page as i32 * 64, TAB_Y, 64, 32) }

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Target { Cell(usize), Page(usize), Previous, Next }
fn hit(x: i32, y: i32, page: usize) -> Option<Target> {
    if PREV.contains(x, y) { return Some(Target::Previous); }
    if NEXT.contains(x, y) { return Some(Target::Next); }
    for p in 0..page_count() {
        if tab_rect(p).contains(x, y) { return Some(Target::Page(p)); }
    }
    (0..cells_on_page(page)).find(|&i| cell_rect(i).contains(x, y)).map(Target::Cell)
}

struct Screen { page: usize, cursor: usize, hover: Option<Target> }
impl Screen {
    fn change_page(&mut self, page: usize) {
        self.page = page % page_count();
        self.cursor = self.cursor.min(cells_on_page(self.page).saturating_sub(1));
    }
    fn previous(&mut self) { self.change_page((self.page + page_count() - 1) % page_count()); }
    fn next(&mut self) { self.change_page(self.page + 1); }
}

struct Asset { image: TgaImage, path: PathBuf, logical: (u32, u32) }

// Do not globally sort CN and JP resolutions together: a Japanese 8x file
// must never shadow a valid translated 4x file. Only the supplied language
// trees are considered, so a Japanese session cannot accidentally discover CN.
fn ordered_dirs<'a>(dirs: &[&'a Path]) -> Vec<&'a Path> {
    let mut result = dirs.to_vec();
    result.sort_by_key(|dir| {
        let name = dir.file_name().and_then(|n| n.to_str()).unwrap_or("").to_ascii_lowercase();
        let cn = name == "bg_cn" || name.starts_with("bg_cn_");
        (!cn, std::cmp::Reverse(crate::codec::image::dir_art_scale(dir)))
    });
    result.dedup();
    result
}

fn valid_asset(img: &TgaImage, logical: (u32, u32)) -> bool {
    img.width >= logical.0 && img.height >= logical.1
        && img.width % logical.0 == 0 && img.height % logical.1 == 0
        && img.width / logical.0 == img.height / logical.1
        && img.rgba.len() == img.width as usize * img.height as usize * 4
        && img.rgba.chunks_exact(4).any(|p| p[3] != 0)
}
fn load(dirs: &[&Path], name: &str, logical: (u32, u32)) -> Option<Asset> {
    for dir in ordered_dirs(dirs) {
        for cand in crate::codec::image::candidate_names(name) {
            let path = dir.join(cand);
            if !path.is_file() { continue; }
            // Keep native pixels. decode_ui_file can silently reduce these to
            // 1x, losing the detail needed by a Retina drawable.
            if let Ok(image) = crate::codec::image::decode_file(&path) {
                if valid_asset(&image, logical) { return Some(Asset { image, path, logical }); }
                eprintln!("saveload: skip blank or wrong-sized required asset {}", path.display());
            }
        }
    }
    eprintln!("saveload: required asset {name} unavailable");
    None
}

struct Assets {
    headers: [Option<Asset>; 2],
    panel: Option<Asset>,
    highlights: [Option<Asset>; 2],
    pages: Option<Asset>,
    tabs: Option<Asset>,
    footer: Option<Asset>,
}
impl Assets {
    fn load(dirs: &[&Path]) -> Self {
        Self {
            headers: [load(dirs, "sys_01000.tga", (1280, 40)), load(dirs, "sys_02000.tga", (1280, 40))],
            panel: load(dirs, "sys_01010.tga", (1264, 192)),
            highlights: [load(dirs, "sys_01011.tga", (632, 96)), load(dirs, "sys_02011.tga", (632, 96))],
            pages: load(dirs, "sys_01020.tga", (840, 96)),
            tabs: load(dirs, "sys_01021.tga", (640, 96)),
            footer: load(dirs, "sys_01022.tga", (432, 128)),
        }
    }
    fn all(&self) -> [&Option<Asset>; 8] {
        [&self.headers[0], &self.headers[1], &self.panel, &self.highlights[0],
         &self.highlights[1], &self.pages, &self.tabs, &self.footer]
    }
}

fn draw_asset(s: &mut Surface, asset: &Option<Asset>, source: Rect, dest: Rect) {
    if let Some(a) = asset {
        let scale = (a.image.width / a.logical.0) as i32;
        let d = dest.pixels(s.width, s.height);
        s.blit_region_scaled(&a.image, source.0 * scale, source.1 * scale,
            (source.2 * scale) as u32, (source.3 * scale) as u32,
            d.0, d.1, d.2 as u32, d.3 as u32, 255);
    }
}

/// Scale the frozen caller frame to *current drawable* pixels; never let its
/// opening dimensions determine caption rasterization or later resize output.
fn background(frame: Option<&TgaImage>, w: u32, h: u32) -> Surface {
    let mut s = Surface::new(w, h, [77, 100, 112, 255]);
    if let Some(f) = frame {
        s.blit_region_scaled(f, 0, 0, f.width, f.height, 0, 0, w, h, 255);
        // The translucent artwork wants a visible backdrop, not black alpha190.
        s.fill_rect(0, 0, w, h, [24, 43, 54, 64]);
    }
    s
}

fn font_px(logical: u32, w: u32, h: u32) -> u32 {
    (logical as f64 * (w as f64 / W as f64).min(h as f64 / H as f64)).round().max(1.0) as u32
}
fn ellipsize(font: &VectorFont, text: &str, width: i32) -> String {
    let text = text.replace(['\n', '\r'], " ");
    if font.measure(&text) <= width { return text; }
    let mut out = String::new();
    for ch in text.chars() {
        let candidate = format!("{out}{ch}…");
        if font.measure(&candidate) > width { break; }
        out.push(ch);
    }
    if font.measure("…") <= width { out.push('…'); }
    out
}

/// Independent native-pixel label boxes, not block_rgba's fixed (size+8)
/// stacking split by a different caption pitch. Measure before drawing so a
/// long date, category or CJK sentence cannot run into its neighbour.
fn captions(scr: &Screen, slots: &[Option<SaveState>], mode: Mode, can_save: bool,
            w: u32, h: u32) -> Option<Vec<u8>> {
    crate::render::text::with_ui_font(font_px(16, w, h), |font| {
        let mut labels = Vec::new();
        let mut add = |text: String, rect: Rect, size: u32| {
            let r = rect.pixels(w, h);
            let size = font_px(size, w, h);
            font.set_size(size);
            let text = ellipsize(font, &text, (r.2 - 4).max(1));
            labels.push(Label::plain(text, r.0 + 2, r.1, (r.2 - 4).max(1), r.3,
                                     size, [239, 250, 255]));
        };
        for i in 0..cells_on_page(scr.page) {
            let idx = slot_index(scr.page, i).unwrap();
            let Rect(x, y, _, _) = cell_rect(i);
            add(format!("{idx:02}"), Rect(x + 20, y + 20, 34, 26), 16);
            if let Some(st) = slots.get(idx).and_then(Option::as_ref) {
                add(st.time.clone(), Rect(x + 200, y + 16, 222, 26), 15);
                add(format!("场景 {}", st.scene), Rect(x + 432, y + 16, 178, 26), 15);
                add(st.caption(), Rect(x + 208, y + 44, 400, 30), 17);
            }
        }
        let hint = if can_save { "[1] 保存  [2] 读取  ·  " } else { "" };
        add(format!("{hint}[Enter] 确定  ·  [←/→] 翻页  ·  [Esc / 右键] 返回     {}  {}/{}  ·  {:02}–{:02}",
            if mode == Mode::Save { "保存" } else { "读取" }, scr.page + 1, page_count(),
            scr.page * PER_PAGE, scr.page * PER_PAGE + cells_on_page(scr.page) - 1),
            Rect(120, 626, 1040, 26), 15);
        font.labels_rgba(w, h, &labels)
    }).flatten()
}

fn compose_base(a: &Assets, scr: &Screen, slots: &[Option<SaveState>], thumbs: &[Option<TgaImage>],
                mode: Mode, can_save: bool, frame: Option<&TgaImage>, w: u32, h: u32) -> (Surface, bool) {
    let mut s = background(frame, w, h);
    draw_asset(&mut s, &a.headers[mode.index()], Rect(0, 0, 1280, 40), Rect(0, 0, 1280, 40));
    for i in 0..cells_on_page(scr.page) {
        let idx = slot_index(scr.page, i).unwrap();
        let r = cell_rect(i);
        let occupied = slots.get(idx).and_then(Option::as_ref).is_some();
        // Empty cells retain the authored No Data and template lettering.
        // Occupied cells use the clean variant, avoiding baked Date/First
        // Sentence beneath real values. Alternate the authored teal/blue rows.
        let row = if (i / 2) % 2 == 0 { 0 } else { 96 };
        draw_asset(&mut s, &a.panel, Rect(if occupied { 632 } else { 0 }, row, 632, 96), r);
        if occupied {
            let rect = Rect(r.0 + 56, r.1 + 8, THUMB_W as i32, THUMB_H as i32).pixels(w, h);
            s.fill_rect(rect.0, rect.1, rect.2 as u32, rect.3 as u32, [28, 57, 69, 160]);
            if let Some(t) = thumbs.get(idx).and_then(Option::as_ref).filter(|t| t.width > 0 && t.height > 0) {
                let scale = (rect.2 as f64 / t.width as f64).min(rect.3 as f64 / t.height as f64);
                let tw = (t.width as f64 * scale).round().max(1.0) as u32;
                let th = (t.height as f64 * scale).round().max(1.0) as u32;
                s.blit_region_scaled(t, 0, 0, t.width, t.height,
                    rect.0 + (rect.2 - tw as i32) / 2, rect.1 + (rect.3 - th as i32) / 2, tw, th, 255);
            }
        }
    }
    draw_asset(&mut s, &a.pages, Rect(0, 0, 840, 96), Rect(220, 544, 840, 96));
    for page in 0..page_count() {
        draw_asset(&mut s, &a.tabs, Rect(page as i32 * 64, if page == scr.page { 32 } else { 0 }, 64, 32), tab_rect(page));
    }
    draw_asset(&mut s, &a.footer, Rect(144, mode.index() as i32 * 40, 288, 40), Rect(496, 664, 288, 40));
    let text = captions(scr, slots, mode, can_save, w, h);
    if let Some(rgba) = text.as_ref() { s.blit_rgba(rgba, w, h, 0, 0); }
    (s, text.is_some())
}
fn compose_active(s: &mut Surface, a: &Assets, scr: &Screen, mode: Mode) {
    draw_asset(s, &a.highlights[mode.index()], Rect(0, 0, 632, 96), cell_rect(scr.cursor));
    for (target, row, dest) in [(Target::Previous, 0, PREV), (Target::Next, 64, NEXT)] {
        let state = if scr.hover == Some(target) { 48 } else { 0 };
        draw_asset(s, &a.footer, Rect(state, row, 48, 64), dest);
    }
    if let Some(Target::Page(p)) = scr.hover {
        if p != scr.page { draw_asset(s, &a.tabs, Rect(p as i32 * 64, 32, 64, 32), tab_rect(p)); }
    }
}

/// Only this interactive activation path writes saves. Capture never enters it.
fn write_slot(i: usize, st: &SaveState, frame: &Surface) -> std::io::Result<()> {
    st.save_to(&save::slot_path(i))?;
    let t = frame.thumbnail(THUMB_W, THUMB_H);
    let path = save::thumb_path(i);
    if let Some(p) = path.parent() { std::fs::create_dir_all(p)?; }
    let mut out = Surface::new(t.width, t.height, [0, 0, 0, 255]);
    out.blit(&t, 0, 0);
    out.save_tga(&path.to_string_lossy())
}
fn load_thumb(i: usize) -> Option<TgaImage> {
    std::fs::read(save::thumb_path(i)).ok().and_then(|d| crate::codec::tga::decode(&d).ok())
}

/// Explicit opt-in evidence from the real loaded slots. No synthetic SaveState,
/// no save/INI write and no input processing, even when displaying Save mode.
fn capture(dir: &Path, win: &mut Sdl, a: &Assets, slots: &[Option<SaveState>],
           thumbs: &[Option<TgaImage>], frame: Option<&TgaImage>, preview: Option<SaveState>) -> Result<(), String> {
    if a.all().iter().any(|a| a.is_none()) { return Err("missing required UI assets".into()); }
    std::fs::create_dir(dir).map_err(|e| format!("{} (capture directory must be new): {e}", dir.display()))?;
    let mut report = format!("read_only=true\nsource_slots={}\nbackground={}\nlayout=2 columns x 5 rows; reconstructed destination coordinates\n",
        save::dir().display(), if frame.is_some() { "frozen caller frame" } else { "neutral fallback" });
    for asset in a.all().into_iter().flatten() {
        report.push_str(&format!("asset={} native={}x{} logical={}x{}\n", asset.path.display(),
            asset.image.width, asset.image.height, asset.logical.0, asset.logical.1));
    }
    for mode in [Mode::Save, Mode::Load] {
        for page in [0, page_count() - 1] {
            let (w, h) = win.drawable_size();
            if w <= 0 || h <= 0 { return Err("empty drawable".into()); }
            let scr = Screen { page, cursor: if page == 0 { 0 } else { PER_PAGE - 1 }, hover: None };
            let (mut s, text_ok) = compose_base(a, &scr, slots, thumbs, mode, false, frame, w as u32, h as u32);
            if !text_ok { return Err("native vector UI font unavailable".into()); }
            compose_active(&mut s, a, &scr, mode);
            let rendered = win.present_checked(&s)?;
            let name = format!("{}-page{:02}.tga", mode.name(), page + 1);
            s.save_tga(&dir.join(&name).to_string_lossy()).map_err(|e| e.to_string())?;
            rendered.save_tga(&dir.join(format!("renderer-{name}")).to_string_lossy()).map_err(|e| e.to_string())?;
            report.push_str(&format!("renderer=renderer-{name} output={}x{} includes letterbox; SDL readback before Present\n", rendered.width, rendered.height));
            report.push_str(&format!("capture={name} mode={} drawable={w}x{h} page={} slot_ids={:02}..{:02} font_px={}\n",
                mode.name(), page + 1, page * PER_PAGE, page * PER_PAGE + cells_on_page(page) - 1, font_px(17, w as u32, h as u32)));
            for cell in 0..cells_on_page(page) {
                let idx = slot_index(page, cell).unwrap();
                report.push_str(&format!("slot={idx:02} loaded={} thumbnail={}\n", slots[idx].is_some(), thumbs[idx].is_some()));
            }
        }
    }
    // A scene acceptance caller may supply its isolated roundtrip snapshot.
    // Keep this explicitly separate from the four authoritative real-slot
    // images above; neither the vector nor the preview is persisted as a save.
    if let Some(preview) = preview {
        let mut preview_slots: Vec<Option<SaveState>> = (0..SLOT_COUNT).map(|_| None).collect();
        preview_slots[0] = Some(preview);
        let mut preview_thumbs: Vec<Option<TgaImage>> = (0..SLOT_COUNT).map(|_| None).collect();
        if let Some(f) = frame {
            let s = Surface { width: f.width, height: f.height, pixels: f.rgba.clone() };
            preview_thumbs[0] = Some(s.thumbnail(THUMB_W, THUMB_H));
        }
        let (w, h) = win.drawable_size();
        if w <= 0 || h <= 0 { return Err("empty preview drawable".into()); }
        let scr = Screen { page: 0, cursor: 0, hover: None };
        let (mut s, text_ok) = compose_base(a, &scr, &preview_slots, &preview_thumbs,
            Mode::Save, false, frame, w as u32, h as u32);
        if !text_ok { return Err("native vector UI font unavailable for preview".into()); }
        compose_active(&mut s, a, &scr, Mode::Save);
        let rendered = win.present_checked(&s)?;
        rendered.save_tga(&dir.join("renderer-save-supplied-snapshot-preview.tga").to_string_lossy()).map_err(|e| e.to_string())?;
        report.push_str(&format!("renderer=renderer-save-supplied-snapshot-preview.tga output={}x{} includes letterbox; SDL readback before Present\n", rendered.width, rendered.height));
        s.save_tga(&dir.join("save-supplied-snapshot-preview.tga").to_string_lossy()).map_err(|e| e.to_string())?;
        report.push_str(&format!("capture=save-supplied-snapshot-preview.tga mode=save drawable={w}x{h} fixture=supplied in-memory snapshot; NOT a user slot; no save writes\n"));
    }
    std::fs::write(dir.join("metadata.txt"), report).map_err(|e| e.to_string())?;
    eprintln!("saveload: read-only capture complete: {}", dir.display());
    Ok(())
}

/// Returns a state only on Load activation. Esc/right-click/Quit cancel without
/// saving; title callers may supply a backdrop without a writable snapshot.
pub fn run(win: &mut Sdl, dirs: &[&Path], mode: Mode, snap: Option<SaveState>,
           frame: Option<&Surface>, mixer: Option<&crate::audio::mixer::Mixer>,
           ui: Option<&crate::UiSound>) -> Option<SaveState> {
    let assets = Assets::load(dirs);
    let frozen = frame.map(|f| TgaImage { width: f.width, height: f.height, rgba: f.pixels.clone() });
    let mut slots = save::list();
    let mut thumbs: Vec<_> = (0..SLOT_COUNT).map(load_thumb).collect();
    if let Some(dir) = std::env::var_os("WA2_SAVELOAD_CAPTURE") {
        if let Err(error) = capture(Path::new(&dir), win, &assets, &slots, &thumbs, frozen.as_ref(), snap) {
            eprintln!("saveload capture: {error}");
            std::process::exit(1);
        }
        return None;
    }
    let can_save = snap.is_some() && frame.is_some();
    let mut mode = if mode == Mode::Save && !can_save { Mode::Load } else { mode };
    let mut scr = Screen { page: 0, cursor: 0, hover: None };
    let mut cache: Option<((usize, Mode, u32, u32, u64), Surface)> = None;
    loop {
        let (w, h) = win.drawable_size();
        let (w, h) = (w.max(1) as u32, h.max(1) as u32);
        let key = (scr.page, mode, w, h, crate::render::text::role_generation());
        if cache.as_ref().map(|c| c.0) != Some(key) {
            let (base, _) = compose_base(&assets, &scr, &slots, &thumbs, mode, can_save, frozen.as_ref(), w, h);
            cache = Some((key, base));
        }
        let base = &cache.as_ref().unwrap().1;
        let mut s = Surface { width: w, height: h, pixels: base.pixels.clone() };
        compose_active(&mut s, &assets, &scr, mode);
        win.present(&s);
        let mut activate = None;
        match win.poll_input() {
            sdl2::Input::Quit | sdl2::Input::Back | sdl2::Input::RightClick => {
                crate::play_ui(mixer, ui, crate::UiSe::Cancel);
                return None;
            }
            sdl2::Input::Choice(1) if can_save => {
                mode = Mode::Save;
                crate::play_ui(mixer, ui, crate::UiSe::Page);
            }
            sdl2::Input::Choice(2) => {
                mode = Mode::Load;
                crate::play_ui(mixer, ui, crate::UiSe::Page);
            }
            sdl2::Input::Left | sdl2::Input::Rewind => {
                scr.previous();
                crate::play_ui(mixer, ui, crate::UiSe::Page);
            }
            sdl2::Input::Right => {
                scr.next();
                crate::play_ui(mixer, ui, crate::UiSe::Page);
            }
            sdl2::Input::Up => {
                if scr.cursor == 0 { scr.previous(); scr.cursor = cells_on_page(scr.page) - 1; }
                else { scr.cursor -= 1; }
                crate::play_ui(mixer, ui, crate::UiSe::Move);
            }
            sdl2::Input::Down => {
                if scr.cursor + 1 >= cells_on_page(scr.page) { scr.next(); scr.cursor = 0; }
                else { scr.cursor += 1; }
                crate::play_ui(mixer, ui, crate::UiSe::Move);
            }
            sdl2::Input::Advance => { activate = Some(scr.cursor); }
            sdl2::Input::Pointer { x, y, pressed } => {
                let (x, y) = win.surface_pos(x, y, W, H);
                let target = hit(x, y, scr.page);
                if !pressed && target != scr.hover && target.is_some() { crate::play_ui(mixer, ui, crate::UiSe::Move); }
                scr.hover = target;
                if let Some(Target::Cell(i)) = target { scr.cursor = i; }
                if pressed {
                    match target {
                        Some(Target::Cell(i)) => activate = Some(i),
                        Some(Target::Page(page)) => scr.change_page(page),
                        Some(Target::Previous) => scr.previous(),
                        Some(Target::Next) => scr.next(),
                        None => {}
                    }
                    if matches!(target, Some(Target::Page(_) | Target::Previous | Target::Next)) {
                        crate::play_ui(mixer, ui, crate::UiSe::Page);
                    }
                }
            }
            _ => {}
        }
        if let Some(idx) = activate.and_then(|i| slot_index(scr.page, i)) {
            crate::play_ui(mixer, ui, crate::UiSe::Confirm);
            match mode {
                Mode::Save => if let (Some(st), Some(f)) = (snap.as_ref(), frame) {
                    match write_slot(idx, st, f) {
                        Ok(()) => {
                            slots[idx] = SaveState::load_from(&save::slot_path(idx));
                            thumbs[idx] = load_thumb(idx);
                            cache = None;
                        }
                        Err(e) => eprintln!("save: slot {idx}: {e}"),
                    }
                },
                Mode::Load => if let Some(st) = SaveState::load_from(&save::slot_path(idx)) { return Some(st); },
            }
        }
        win.delay(16);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ten_pages_cover_exactly_the_existing_hundred_slot_ids() {
        assert_eq!(page_count(), 10);
        let ids: Vec<_> = (0..page_count()).flat_map(|p| (0..cells_on_page(p)).map(move |c| slot_index(p, c).unwrap())).collect();
        assert_eq!(ids, (0..100).collect::<Vec<_>>());
        assert_eq!(slot_index(9, 9), Some(99));
        assert_eq!(slot_index(0, 10), None);
        assert_eq!(slot_index(10, 0), None);
        assert_eq!(slot_index(usize::MAX, 0), None);
        assert_eq!(cells_on_page(10), 0);
    }

    #[test]
    fn mouse_controls_match_drawn_rectangles_and_wrap_pages() {
        assert_eq!(hit(448, 652, 0), Some(Target::Previous));
        assert_eq!(hit(495, 715, 0), Some(Target::Previous));
        assert_eq!(hit(496, 715, 0), None);
        assert_eq!(hit(784, 652, 0), Some(Target::Next));
        assert_eq!(hit(831, 716, 0), None);
        assert_eq!(hit(-1, -1, 0), None);
        for page in 0..10 {
            let r = tab_rect(page);
            assert_eq!(hit(r.0 + 10, r.1 + 10, 0), Some(Target::Page(page)));
        }
        let mut scr = Screen { page: 0, cursor: 9, hover: None };
        scr.previous();
        assert_eq!(slot_index(scr.page, scr.cursor), Some(99));
        scr.next();
        assert_eq!(scr.page, 0);
    }

    #[test]
    fn chinese_priority_precedes_resolution_and_blank_assets_are_rejected() {
        let dirs = [Path::new("bg_8x"), Path::new("bg_cn_2x"), Path::new("bg_cn_4x"), Path::new("bg")];
        assert_eq!(ordered_dirs(&dirs), vec![dirs[2], dirs[1], dirs[0], dirs[3]]);
        let mut image = TgaImage { width: 864, height: 256, rgba: vec![0; 864 * 256 * 4] };
        assert!(!valid_asset(&image, (432, 128)), "CN2x blank footer must not end fallback");
        image.rgba[3] = 255;
        assert!(valid_asset(&image, (432, 128)));
        assert!(!valid_asset(&image, (288, 40)), "never squeeze footer sheet into a text strip");
        assert!(!valid_asset(&image, (1280, 40)));
    }

    #[test]
    fn required_asset_fallback_continues_in_same_language() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let root = std::env::temp_dir().join(format!("wa2-saveload-{}-{stamp}", std::process::id()));
        let paths: Vec<_> = ["bg_cn_8x", "bg_cn_4x", "bg_8x"].iter().map(|n| root.join(n)).collect();
        for (i, path) in paths.iter().enumerate() {
            std::fs::create_dir_all(path).unwrap();
            // Small native-size fixture also checks that actual image dimensions,
            // not the directory suffix, define source crop scale.
            Surface::new(432, 128, if i == 0 { [0, 0, 0, 0] } else { [30, 60, 90, 255] })
                .save_tga(&path.join("sys_01022.tga").to_string_lossy()).unwrap();
        }
        let dirs: Vec<_> = paths.iter().map(PathBuf::as_path).collect();
        let asset = load(&dirs, "sys_01022.tga", (432, 128)).unwrap();
        assert_eq!(asset.path.parent(), Some(paths[1].as_path()));
        assert_eq!(asset.image.width, 432);
        assert!(root.starts_with(std::env::temp_dir()));
        std::fs::remove_dir_all(&root).unwrap();
    }

    #[test]
    fn background_preserves_alpha_scene_and_follows_resize_not_snapshot() {
        let f = TgaImage { width: 2, height: 1, rgba: vec![200, 160, 120, 255, 100, 120, 160, 255] };
        for (w, h) in [(1280, 720), (2560, 1440), (960, 540)] {
            let s = background(Some(&f), w, h);
            assert_eq!((s.width, s.height), (w, h));
            assert!(s.pixels[0] > 140, "backdrop must not become near-black");
            assert_eq!(s.pixels[3], 255);
            let before = s.pixels[0];
            let mut s = s;
            s.fill_rect(0, 0, 1, 1, [0, 120, 140, 128]);
            assert!(s.pixels[0] > 0 && s.pixels[0] < before, "panel alpha must blend");
        }
        let s = background(None, 32, 18);
        assert_eq!(&s.pixels[..4], &[77, 100, 112, 255]);
    }

    #[test]
    fn native_text_and_cell_bounds_scale_without_block_caption_clipping() {
        assert_eq!(font_px(17, 1280, 720), 17);
        assert_eq!(font_px(17, 2560, 1440), 34);
        assert_eq!(font_px(17, 3840, 2160), 51);
        let slots: Vec<Option<SaveState>> = (0..100).map(|_| None).collect();
        let scr = Screen { page: 9, cursor: 9, hover: None };
        for (w, h) in [(1280, 720), (2560, 1440)] {
            let rgba = captions(&scr, &slots, Mode::Load, false, w, h).expect("vector UI font");
            assert_eq!(rgba.len(), w as usize * h as usize * 4);
            for i in 0..10 {
                let Rect(x, y, _, _) = cell_rect(i);
                let r = Rect(x + 20, y + 20, 34, 26).pixels(w, h);
                assert!((r.1..r.1 + r.3).any(|y| (r.0..r.0 + r.2).any(|x| rgba[((y as u32 * w + x as u32) * 4 + 3) as usize] > 0)), "missing slot {i} at {w}x{h}");
            }
        }
        crate::render::text::with_ui_font(34, |font| {
            let short = ellipsize(font, "很长的存档对白不能覆盖相邻的字段或面板边框", 180);
            assert!(short.ends_with('…'));
            assert!(font.measure(&short) <= 180);
        }).expect("vector UI font");
    }

    #[test]
    fn populated_chinese_captions_fit_their_native_pixel_fields() {
        let stamp = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap().as_nanos();
        let path = std::env::temp_dir().join(format!("wa2-saveload-caption-{}-{stamp}.sav", std::process::id()));
        std::fs::write(&path, "version=2\nscene=长场景名称必须保持在类别区域内\ntime=2026-10-31 23:59 很长的时间字段\ntext=很长的中文存档对白必须完整测量后省略而不能覆盖其他字段或面板边框\n").unwrap();
        let mut slots: Vec<Option<SaveState>> = (0..100).map(|_| None).collect();
        slots[0] = Some(SaveState::load_from(&path).unwrap());
        assert!(path.starts_with(std::env::temp_dir()));
        std::fs::remove_file(&path).unwrap();
        let scr = Screen { page: 0, cursor: 0, hover: None };
        for (w, h) in [(1280, 720), (2560, 1440)] {
            let rgba = captions(&scr, &slots, Mode::Save, true, w, h).expect("native vector captions");
            let mut boxes: Vec<_> = (0..10).map(|i| {
                let r = cell_rect(i);
                Rect(r.0 + 20, r.1 + 20, 34, 26).pixels(w, h)
            }).collect();
            let r = cell_rect(0);
            let fields = [Rect(r.0 + 200, r.1 + 16, 222, 26),
                          Rect(r.0 + 432, r.1 + 16, 178, 26),
                          Rect(r.0 + 208, r.1 + 44, 400, 30)];
            for field in fields {
                let b = field.pixels(w, h);
                assert!((b.1..b.1 + b.3).any(|y| (b.0..b.0 + b.2).any(|x|
                    rgba[((y as u32 * w + x as u32) * 4 + 3) as usize] != 0)), "occupied field has no text");
                boxes.push(b);
            }
            boxes.push(Rect(120, 626, 1040, 26).pixels(w, h));
            for (i, pixel) in rgba.chunks_exact(4).enumerate().filter(|(_, p)| p[3] != 0) {
                let (x, y) = ((i % w as usize) as i32, (i / w as usize) as i32);
                assert!(boxes.iter().any(|b| b.contains(x, y)), "text overflow at {x},{y} for {w}x{h}: {pixel:?}");
            }
        }
    }

    /// Explicit resource acceptance; no live game process or persistent writes.
    #[test]
    fn installed_assets_have_save_load_dimensions_and_visible_translated_footer() {
        let paths = crate::test_data::bg_dirs();
        let dirs: Vec<_> = paths.iter().map(PathBuf::as_path).collect();
        let a = Assets::load(&dirs);
        assert!(a.all().iter().all(|a| a.is_some()));
        assert!(a.headers[0].as_ref().unwrap().path.file_stem().unwrap().to_string_lossy().eq_ignore_ascii_case("sys_01000"));
        assert!(a.headers[1].as_ref().unwrap().path.file_stem().unwrap().to_string_lossy().eq_ignore_ascii_case("sys_02000"));
        let f = a.footer.as_ref().unwrap();
        assert_eq!(f.logical, (432, 128));
        assert!(f.path.parent().unwrap().file_name().unwrap().to_string_lossy().starts_with("bg_cn"));
        assert!(valid_asset(&f.image, f.logical));
    }
}
