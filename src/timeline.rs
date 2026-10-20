//! Scene timeline: one thumbnail per script, lines following the script jumps,
//! so a scene can be opened from any chapter or branch.
//!
//! Sub-scripts (1008_020) stay inside their parent - including the jumps they
//! carry. The split at the end of the common route is the four-way jump in
//! 2019. A save's scene is marked so the chart opens on the line you were
//! reading.
//!
//! Module layout:
//!   chapters  static chapter table (id range -> title/lane/colour)
//!   graph     scenario graph: nodes, edges, branch/ending/root flags, layout
//!   shots     thumbnail choice and the data/cache/timeline pre-render
//!   preview   preview bar: Chinese text + vector typography
//!   paint     the chart itself: bands, headers, edges, nodes, preview bar
//!   nav       view transforms, dragging, zooming, keyboard movement

mod chapters;
mod graph;
mod nav;
mod paint;
mod preview;
mod shots;

use crate::codec::tga::TgaImage;
use crate::render::{self, Surface};
use crate::sdl2::{self, Sdl};
use chapters::Chapter;
use preview::Preview;
use std::collections::HashMap;
use std::path::{Path, PathBuf};

pub(crate) const W: i32 = 1280;
pub(crate) const H: i32 = 720;
/// Node thumbnail box, 16:9 (the old 104x58 compressed every picture slightly).
pub(crate) const NODE_W: i32 = 104;
pub(crate) const NODE_H: i32 = 59;
pub(crate) const PITCH_X: i32 = 136;
pub(crate) const PITCH_Y: i32 = 72;
pub(crate) const PREVIEW_H: i32 = 196;
pub(crate) const PREVIEW_TOP: i32 = H - PREVIEW_H;
/// Top of the chart, under the scene-replay banner and the chapter headers.
pub(crate) const CHART_TOP: i32 = 72;
pub(crate) const BG: [u8; 4] = [1, 18, 52, 255];
pub(crate) const LINE: [u8; 4] = [150, 196, 220, 230];
pub(crate) const GOLD: [u8; 4] = [232, 196, 92, 255];
pub(crate) const CYAN: [u8; 4] = [90, 210, 230, 255];
pub(crate) const PREVIEW_W: i32 = 208;
pub(crate) const PREVIEW_H_THUMB: i32 = 117;

/// One ACT146/147/148 picture named by a scenario, in script order.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Shot {
    pub cg: bool,
    pub id: i64,
    pub variant: i32,
    /// Index of the instruction in the parsed program, for debugging.
    pub op_index: usize,
    /// Script stem that named it: 1008 or the sub-script 1008_020.
    pub source: String,
}

/// How the picture shown on a node relates to the scene's entry.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ShotKind {
    /// The scene opens on its own picture.
    Entry,
    /// The entry repeated the previous scene, so the first change was taken.
    Changed,
    /// The scene never changes the picture; it inherits the previous one.
    Inherited,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum EdgeKind {
    /// The only way out of the script.
    Sequential,
    /// One of several exits (a choice / CATCH fan-out).
    Branch,
}

#[derive(Debug, Clone)]
pub struct Edge {
    pub to: usize,
    pub kind: EdgeKind,
    /// Empty for a plain continuation, otherwise the target's short name.
    pub label: String,
}

pub struct Node {
    pub id: String,
    pub chapter: &'static Chapter,
    pub col: i32,
    pub lane: i32,
    pub next: Vec<Edge>,
    /// No incoming edge: a legal entry point (IC start, extra novels).
    pub root: bool,
    /// No outgoing edge: an ending scene.
    pub ending: bool,
    /// Pictures in the order the script shows them.
    pub shot: Vec<Shot>,
}

/// Horizontal extent of one chapter on the chart.
pub struct Band {
    pub chapter: &'static Chapter,
    pub first_col: i32,
    pub last_col: i32,
    pub min_lane: i32,
    pub max_lane: i32,
}

pub struct Chart {
    pub nodes: Vec<Node>,
    /// Newest save, collapsed to its entry script.
    pub here: Option<usize>,
    pub bands: Vec<Band>,
}

pub struct View {
    pub zoom: f32,
    /// Where the wheel is heading. zoom eases toward this.
    pub target_zoom: f32,
    pub scroll_x: f32,
    pub scroll_y: f32,
}

/// Coverage-only rasterized text; tinted at blit time.
pub struct Label {
    pub rgba: Vec<u8>,
    pub w: u32,
    pub h: u32,
}

/// Where a node's display thumbnail came from (debug/tooling).
#[derive(Debug, Clone)]
pub struct ThumbInfo {
    pub kind: ShotKind,
    pub path: PathBuf,
}

/// Thumbnails keyed by scene id and "preview:{id}".
pub struct ThumbSet {
    pub imgs: HashMap<String, TgaImage>,
    pub info: HashMap<String, ThumbInfo>,
}

/// Open the chart. Some(id) starts that script; None returns to the title.
pub fn run(
    win: &mut Sdl,
    font: &render::Font,
    bg_dirs: &[&Path],
    script_dir: &Path,
    cn: bool,
    mixer: Option<&crate::audio::mixer::Mixer>,
    ui: Option<&crate::UiSound>,
) -> Option<String> {
    let _ = font;
    let mut cover = Surface::new(W as u32, H as u32, BG);
    if let Some(banner) = load_tga(bg_dirs, "sys_06000.tga") {
        cover.blit(&banner, 0, 0);
    }
    win.present(&cover);

    let chart = Chart::load(script_dir);
    if chart.nodes.is_empty() {
        eprintln!("timeline: no scenes under {}", script_dir.display());
        return None;
    }
    let cache_dir = script_dir
        .parent()
        .map(|root| root.join("cache").join("timeline"))
        .unwrap_or_else(|| PathBuf::from("cache/timeline"));
    let thumbs = shots::load_thumbs(bg_dirs, &chart, &cache_dir);
    let banner = load_tga(bg_dirs, "sys_06000.tga");
    let pager = load_tga(bg_dirs, "sys_06021.tga");
    // Chapter headers are fixed screen text: rasterize once, not per frame.
    let headers: Vec<Option<Label>> = chart
        .bands
        .iter()
        .map(|band| preview::label(band.chapter.title, 15))
        .collect();
    eprintln!("timeline: {} scenes, {} chapters", chart.nodes.len(), chart.bands.len());

    // Verification hooks (same spirit as WA2_TITLE_AUTO): open the chart on a
    // given scene / zoom so a headless screenshot can pin one visual state.
    let mut sel = chart.here.unwrap_or(0);
    if let Some(want) = std::env::var_os("WA2_TIMELINE_SEL") {
        let want = want.to_string_lossy().into_owned();
        if let Some(i) = chart.nodes.iter().position(|node| node.id == want) {
            sel = i;
        } else {
            eprintln!("timeline: WA2_TIMELINE_SEL={want} is not a chart node");
        }
    }
    let start_zoom = std::env::var("WA2_TIMELINE_ZOOM")
        .ok()
        .and_then(|v| v.trim().parse::<f32>().ok())
        .unwrap_or(1.0)
        .clamp(nav::ZOOM_MIN, nav::ZOOM_MAX);
    let mut view = View { zoom: start_zoom, target_zoom: start_zoom, scroll_x: 0.0, scroll_y: 0.0 };
    nav::focus_view(&chart, sel, &mut view);
    let mut filter: Option<&'static chapters::Chapter> = None;
    let mut hover: Option<usize> = None;
    let mut hold: Option<nav::Hold> = None;
    let mut cursor = (W / 2, (CHART_TOP + PREVIEW_TOP) / 2);
    let mut preview = preview::build(script_dir, &chart.nodes[sel], cn);
    let mut preview_for = sel;
    let mut frame = Surface::new(W as u32, H as u32, BG);
    let mut clock = std::time::Instant::now();

    loop {
        let dt = clock.elapsed().as_secs_f32().clamp(0.001, 0.05);
        clock = std::time::Instant::now();
        // The wheel sets a target; the picture eases toward it so a notch does
        // not pop the whole chart by 15% in one frame.
        nav::ease_zoom(&chart, &mut view, cursor.0, cursor.1, dt);
        if preview_for != sel {
            preview = preview::build(script_dir, &chart.nodes[sel], cn);
            preview_for = sel;
        }
        paint::paint(
            &mut frame,
            &paint::PaintState {
                chart: &chart,
                thumbs: &thumbs,
                banner: banner.as_ref(),
                pager: pager.as_ref(),
                sel,
                hover,
                view: &view,
                preview: &preview,
                headers: &headers,
                filter,
            },
        );
        win.present(&frame);

        match win.poll_input() {
            sdl2::Input::Quit | sdl2::Input::RightClick | sdl2::Input::Back => {
                crate::play_ui(mixer, ui, crate::UiSe::Cancel);
                return None;
            }
            sdl2::Input::Advance => {
                crate::play_ui(mixer, ui, crate::UiSe::Confirm);
                return Some(chart.nodes[sel].id.clone());
            }
            sdl2::Input::Left => {
                sel = nav::step(&chart, sel, -1);
                nav::focus_view(&chart, sel, &mut view);
                crate::play_ui(mixer, ui, crate::UiSe::Move);
            }
            sdl2::Input::Right => {
                sel = nav::step(&chart, sel, 1);
                nav::focus_view(&chart, sel, &mut view);
                crate::play_ui(mixer, ui, crate::UiSe::Move);
            }
            sdl2::Input::Up => {
                sel = nav::step_lane(&chart, sel, -1);
                nav::focus_view(&chart, sel, &mut view);
                crate::play_ui(mixer, ui, crate::UiSe::Move);
            }
            sdl2::Input::Down => {
                sel = nav::step_lane(&chart, sel, 1);
                nav::focus_view(&chart, sel, &mut view);
                crate::play_ui(mixer, ui, crate::UiSe::Move);
            }
            sdl2::Input::Nav(sdl2::NavKey::Home) => {
                sel = nav::home(&chart);
                nav::focus_view(&chart, sel, &mut view);
                crate::play_ui(mixer, ui, crate::UiSe::Move);
            }
            sdl2::Input::Nav(sdl2::NavKey::End) => {
                sel = nav::end(&chart, sel);
                nav::focus_view(&chart, sel, &mut view);
                crate::play_ui(mixer, ui, crate::UiSe::Move);
            }
            sdl2::Input::Nav(sdl2::NavKey::Tab) => {
                sel = nav::step_chapter(&chart, sel, 1);
                nav::focus_view(&chart, sel, &mut view);
                crate::play_ui(mixer, ui, crate::UiSe::Page);
            }
            sdl2::Input::Nav(sdl2::NavKey::Reset) => {
                nav::reset_view(&chart, &mut view);
                filter = None;
                crate::play_ui(mixer, ui, crate::UiSe::Page);
            }
            sdl2::Input::Nav(sdl2::NavKey::Filter) => {
                filter = cycle_filter(&chart, sel, filter);
                sel = ensure_filter_selection(&chart, sel, filter);
                nav::focus_view(&chart, sel, &mut view);
                crate::play_ui(mixer, ui, crate::UiSe::Page);
            }
            sdl2::Input::Nav(sdl2::NavKey::Focus) => {
                if let Some(here) = chart.here {
                    sel = here;
                    nav::focus_view(&chart, sel, &mut view);
                    crate::play_ui(mixer, ui, crate::UiSe::Move);
                }
            }
            sdl2::Input::Choice(n) => {
                if let Some(band) = chart.bands.get((n as usize).saturating_sub(1)) {
                    if let Some(i) = chart.nodes.iter().position(|node| std::ptr::eq(node.chapter, band.chapter)) {
                        sel = i;
                        filter = Some(band.chapter);
                        nav::focus_view(&chart, sel, &mut view);
                        crate::play_ui(mixer, ui, crate::UiSe::Page);
                    }
                }
            }
            sdl2::Input::Wheel { y } => {
                nav::wheel(&mut view, y);
                crate::play_ui(mixer, ui, crate::UiSe::Page);
            }
            sdl2::Input::Drag { x, y } => {
                let (sx, sy) = win.surface_pos(x, y, W as u32, H as u32);
                cursor = (sx, sy);
                // A press and its first moves can arrive as one drag, with no
                // separate down event. Grab here so the next sample still pans.
                if hold.is_none() {
                    hold = Some(nav::grab(sx, sy));
                }
                nav::apply_drag(&chart, &mut view, &mut hold, sx, sy);
            }
            sdl2::Input::Pointer { x, y, pressed } => {
                let (sx, sy) = win.surface_pos(x, y, W as u32, H as u32);
                cursor = (sx, sy);
                if pressed {
                    hold = Some(nav::grab(sx, sy));
                } else if hold.is_some() {
                    // Button-down motion sometimes arrives without the button
                    // bit set. A press is already held, so this is still a drag.
                    nav::apply_drag(&chart, &mut view, &mut hold, sx, sy);
                } else {
                    hover = nav::hit_node(&chart, &view, sx, sy);
                    continue;
                }
            }
            sdl2::Input::Release { x, y } => {
                let (sx, sy) = win.surface_pos(x, y, W as u32, H as u32);
                cursor = (sx, sy);
                let was_pan = hold.take().is_some_and(|h| h.panning);
                if was_pan {
                    continue;
                }
                if let Some(dir) = nav::hit_pager(pager.as_ref(), sx, sy) {
                    let page = (W - 80).max(200);
                    view.scroll_x += dir as f32 * page as f32;
                    nav::clamp_view(&chart, &mut view);
                    crate::play_ui(mixer, ui, crate::UiSe::Page);
                    continue;
                }
                if nav::hit_preview(sx, sy) {
                    crate::play_ui(mixer, ui, crate::UiSe::Confirm);
                    return Some(chart.nodes[sel].id.clone());
                }
                if let Some(i) = nav::hit_node(&chart, &view, sx, sy) {
                    if i == sel {
                        crate::play_ui(mixer, ui, crate::UiSe::Confirm);
                        return Some(chart.nodes[sel].id.clone());
                    }
                    sel = i;
                    hover = Some(i);
                    crate::play_ui(mixer, ui, crate::UiSe::Move);
                }
            }
            _ => win.delay(16),
        }
        if let Some(f) = filter {
            let visible = chart.nodes.get(sel).is_some_and(|node| std::ptr::eq(node.chapter, f));
            if !visible {
                sel = ensure_filter_selection(&chart, sel, filter);
                nav::focus_view(&chart, sel, &mut view);
            }
        }
    }
}

fn cycle_filter(
    chart: &Chart,
    sel: usize,
    current: Option<&'static chapters::Chapter>,
) -> Option<&'static chapters::Chapter> {
    let mut choices: Vec<&'static chapters::Chapter> = Vec::new();
    for band in &chart.bands {
        if !choices.iter().any(|chapter| std::ptr::eq(*chapter, band.chapter)) {
            choices.push(band.chapter);
        }
    }
    if choices.is_empty() { return None; }
    match current {
        None => chart.nodes.get(sel).map(|node| node.chapter),
        Some(chapter) => {
            let Some(i) = choices.iter().position(|candidate| std::ptr::eq(*candidate, chapter)) else {
                return Some(choices[0]);
            };
            choices.get(i + 1).copied()
        }
    }
}

fn ensure_filter_selection(
    chart: &Chart,
    sel: usize,
    filter: Option<&'static chapters::Chapter>,
) -> usize {
    let Some(chapter) = filter else { return sel };
    if chart.nodes.get(sel).is_some_and(|node| std::ptr::eq(node.chapter, chapter)) {
        return sel;
    }
    chart.nodes
        .iter()
        .position(|node| std::ptr::eq(node.chapter, chapter))
        .unwrap_or(sel)
}

/// Offline consistency report for the chart (wa2-native timeline-check).
pub fn check(script_dir: &Path) -> Vec<String> {
    graph::check(script_dir)
}

pub fn entry_of(id: &str) -> &str {
    id.split('_').next().unwrap_or(id)
}

pub fn is_script_id(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('_')
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_digit() || c == '_')
}

fn load_tga(dirs: &[&Path], name: &str) -> Option<TgaImage> {
    for dir in dirs {
        for cand in crate::codec::image::candidate_names(name) {
            // The chart's banner/pager are UI sheets authored in logical
            // coordinates, so they must be normalized just like the settings
            // screens rather than decoded at the window's art scale.
            if let Ok(img) = crate::codec::image::decode_ui_file(&dir.join(cand)) {
                return Some(img);
            }
        }
    }
    None
}

impl Chart {
    fn load(script_dir: &Path) -> Chart {
        let nodes = graph::build_nodes(script_dir);
        let here = newest_entry().and_then(|id| nodes.iter().position(|n| n.id == id));
        let bands = chapters::bands(&nodes);
        Chart { nodes, here, bands }
    }
}

fn newest_entry() -> Option<String> {
    let dir = crate::save::dir();
    let rd = std::fs::read_dir(&dir).ok()?;
    let mut best: Option<(std::time::SystemTime, std::path::PathBuf)> = None;
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().to_string();
        if !name.ends_with(".sav") {
            continue;
        }
        let Ok(meta) = ent.metadata() else { continue };
        let Ok(modified) = meta.modified() else { continue };
        if best.as_ref().map(|(t, _)| modified > *t).unwrap_or(true) {
            best = Some((modified, ent.path()));
        }
    }
    let path = best?.1;
    let scene = crate::save::SaveState::load_from(&path)?.scene;
    let entry = entry_of(&scene);
    if is_script_id(entry) { Some(entry.to_string()) } else { None }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn script_ids_are_parsed_conservatively() {
        assert!(is_script_id("1001"));
        assert!(is_script_id("1008_020"));
        assert!(!is_script_id("CATCH"));
        assert!(!is_script_id("_020"));
        assert_eq!(entry_of("1008_020"), "1008");
        assert_eq!(entry_of("1008"), "1008");
    }

    #[test]
    fn the_chart_is_laid_out_in_bands() {
        let dir = Path::new("data/script");
        if !dir.is_dir() {
            return;
        }
        let nodes = graph::build_nodes(dir);
        let bands = chapters::bands(&nodes);
        let chart = Chart { nodes, here: None, bands };
        assert!(chart.bands.len() >= 8, "expected the chapter bands, got {}", chart.bands.len());
        for band in &chart.bands {
            assert!(band.first_col <= band.last_col);
            assert!(band.min_lane <= band.max_lane);
        }
        // Every node belongs to exactly one band.
        for node in &chart.nodes {
            let count = chart.bands.iter().filter(|b| std::ptr::eq(b.chapter, node.chapter)).count();
            assert_eq!(count, 1, "{} belongs to {} bands", node.id, count);
        }
    }

    #[test]
    fn route_filter_cycles_and_keeps_selection_inside_the_chapter() {
        let dir = Path::new("data/script");
        if !dir.is_dir() {
            return;
        }
        let nodes = graph::build_nodes(dir);
        let bands = chapters::bands(&nodes);
        let chart = Chart { nodes, here: None, bands };
        if chart.nodes.is_empty() {
            return;
        }
        let first = cycle_filter(&chart, 0, None).expect("first chapter filter");
        let selected = ensure_filter_selection(&chart, 0, Some(first));
        assert!(std::ptr::eq(chart.nodes[selected].chapter, first));
        let next = cycle_filter(&chart, selected, Some(first));
        assert!(next.is_some_and(|chapter| !std::ptr::eq(chapter, first)));
    }
}
