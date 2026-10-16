//! Scene timeline: one thumbnail per script, lines following the script jumps,
//! so a scene can be opened from any chapter or branch.
//!
//! Sub-scripts (`1008_020`) stay inside their parent. The split at the end of
//! the common route is the four-way jump in `2019`. A save's scene is marked
//! so the chart opens on the line you were reading.

use crate::codec::sjis;
use crate::codec::tga::TgaImage;
use crate::render::{self, Surface};
use crate::sdl2::{self, Sdl};
use crate::vm::lscr::{Op, Program};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

const W: i32 = 1280;
const H: i32 = 720;
const NODE_W: i32 = 104;
const NODE_H: i32 = 58;
const PITCH_X: i32 = 136;
const PITCH_Y: i32 = 76;
const PREVIEW_H: i32 = 196;
const PREVIEW_TOP: i32 = H - PREVIEW_H;
const BG: [u8; 4] = [1, 18, 52, 255];
const LINE: [u8; 4] = [150, 196, 220, 230];
const GOLD: [u8; 4] = [232, 196, 92, 255];
const CYAN: [u8; 4] = [90, 210, 230, 255];

struct Node {
    id: String,
    chapter: &'static str,
    col: i32,
    lane: i32,
    next: Vec<usize>,
    /// Pictures in the order the script shows them: `(event_cg, id, variant)`.
    /// The first one that actually exists on disk is the thumbnail. Event CGs
    /// are `v{id*10+variant}`; backgrounds are `b{id:04}{variant:02}`.
    shot: Vec<(bool, i64, i32)>,
}

struct Chart {
    nodes: Vec<Node>,
    /// Newest save, collapsed to its entry script.
    here: Option<usize>,
}

/// Open the chart. `Some(id)` starts that script; `None` returns to the title.
pub fn run(
    win: &mut Sdl,
    font: &render::Font,
    bg_dirs: &[&Path],
    script_dir: &Path,
) -> Option<String> {
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
    let thumbs = load_thumbs(bg_dirs, &chart.nodes);
    let banner = load_tga(bg_dirs, "sys_06000.tga");
    let pager = load_tga(bg_dirs, "sys_06021.tga");
    eprintln!("timeline: {} scenes", chart.nodes.len());

    let mut sel = chart.here.unwrap_or(0);
    let mut view = View { zoom: 1.0, target_zoom: 1.0, scroll_x: 0.0, scroll_y: 0.0 };
    focus_view(&chart, sel, &mut view);
    let mut hover: Option<usize> = None;
    let mut hold: Option<Hold> = None;
    let mut cursor = (W / 2, (CHART_TOP + PREVIEW_TOP) / 2);
    let mut quote = String::new();
    let mut quote_for = String::new();
    let hint = text_label("滚轮缩放 · 拖动画面 · 回车进入", 18);
    let mut chapter_label: Option<Label> = None;
    let mut chapter_for = String::new();
    let mut frame = Surface::new(W as u32, H as u32, BG);
    let mut clock = std::time::Instant::now();

    loop {
        let dt = clock.elapsed().as_secs_f32().clamp(0.001, 0.05);
        clock = std::time::Instant::now();
        // The wheel sets a target; the picture eases toward it so a notch
        // does not pop the whole chart by 15% in one frame.
        ease_zoom(&chart, &mut view, cursor.0, cursor.1, dt);
        if quote_for != chart.nodes[sel].id {
            quote = preview_line(script_dir, &chart.nodes[sel].id);
            quote_for = chart.nodes[sel].id.clone();
        }
        let chapter = format!("{}    {}", chart.nodes[sel].chapter, chart.nodes[sel].id);
        if chapter_for != chapter {
            chapter_label = text_label(&chapter, 22);
            chapter_for = chapter;
        }
        paint(
            &mut frame,
            &chart,
            &thumbs,
            banner.as_ref(),
            pager.as_ref(),
            font,
            sel,
            hover,
            &view,
            &quote,
            chapter_label.as_ref(),
            hint.as_ref(),
        );
        win.present(&frame);

        match win.poll_input() {
            sdl2::Input::Quit | sdl2::Input::RightClick => return None,
            sdl2::Input::Advance => return Some(chart.nodes[sel].id.clone()),
            sdl2::Input::Back => {
                sel = step(&chart, sel, -1);
                focus_view(&chart, sel, &mut view);
            }
            sdl2::Input::Up => {
                sel = step_lane(&chart, sel, -1);
                focus_view(&chart, sel, &mut view);
            }
            sdl2::Input::Down => {
                sel = step_lane(&chart, sel, 1);
                focus_view(&chart, sel, &mut view);
            }
            sdl2::Input::Wheel { y } => {
                if y != 0 {
                    let factor = 1.12_f32.powi(y.clamp(-8, 8));
                    view.target_zoom = (view.target_zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
                }
            }
            sdl2::Input::Drag { x, y } => {
                let (sx, sy) = win.surface_pos(x, y, W as u32, H as u32);
                cursor = (sx, sy);
                // A press and its first moves can arrive as one drag, with no
                // separate down event. Grab here so the next sample still pans.
                if hold.is_none() {
                    hold = Some(grab(sx, sy));
                }
                apply_drag(&chart, &mut view, &mut hold, sx, sy);
            }
            sdl2::Input::Pointer { x, y, pressed } => {
                let (sx, sy) = win.surface_pos(x, y, W as u32, H as u32);
                cursor = (sx, sy);
                if pressed {
                    hold = Some(grab(sx, sy));
                } else if hold.is_some() {
                    // Button-down motion sometimes arrives without the button
                    // bit set. A press is already held, so this is still a drag.
                    apply_drag(&chart, &mut view, &mut hold, sx, sy);
                } else {
                    hover = hit_node(&chart, &view, sx, sy);
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
                if let Some(dir) = hit_pager(pager.as_ref(), sx, sy) {
                    let page = (W - 80).max(200);
                    view.scroll_x += dir as f32 * page as f32;
                    clamp_view(&chart, &mut view);
                    continue;
                }
                if hit_preview(sx, sy) {
                    return Some(chart.nodes[sel].id.clone());
                }
                if let Some(i) = hit_node(&chart, &view, sx, sy) {
                    if i == sel {
                        return Some(chart.nodes[sel].id.clone());
                    }
                    sel = i;
                    hover = Some(i);
                }
            }
            _ => win.delay(16),
        }
    }
}

/// Movement smaller than this stays a click. Past it, the chart follows the cursor.
const DRAG_SLOP: i32 = 5;
const ZOOM_MIN: f32 = 0.4;
const ZOOM_MAX: f32 = 2.4;

struct View {
    zoom: f32,
    /// Where the wheel is heading. `zoom` eases toward this.
    target_zoom: f32,
    scroll_x: f32,
    scroll_y: f32,
}

struct Label {
    rgba: Vec<u8>,
    w: u32,
    h: u32,
}

fn text_label(text: &str, size: u32) -> Option<Label> {
    let (w, h, rgba) = render::text::with_ui_font(size, |f| f.line_rgba(text, size)).flatten()?;
    Some(Label { rgba, w, h })
}

struct Hold {
    ax: i32,
    ay: i32,
    panning: bool,
    /// Press began on the chart, not the preview bar.
    chart: bool,
}

fn grab(sx: i32, sy: i32) -> Hold {
    Hold {
        ax: sx,
        ay: sy,
        panning: false,
        chart: sy >= 40 && sy < PREVIEW_TOP,
    }
}

/// Pan once the cursor leaves the click slop. The anchor stays on the grab
/// point, so the chart sticks to the cursor instead of ignoring the gesture.
fn begin_pan(h: &mut Hold, sx: i32, sy: i32) -> Option<(i32, i32)> {
    if !h.chart {
        return None;
    }
    if !h.panning {
        let dx = sx - h.ax;
        let dy = sy - h.ay;
        if dx * dx + dy * dy < DRAG_SLOP * DRAG_SLOP {
            return None;
        }
        h.panning = true;
    }
    let delta = (sx - h.ax, sy - h.ay);
    h.ax = sx;
    h.ay = sy;
    Some(delta)
}

fn apply_drag(chart: &Chart, view: &mut View, hold: &mut Option<Hold>, sx: i32, sy: i32) {
    let Some(h) = hold.as_mut() else { return };
    let Some((dx, dy)) = begin_pan(h, sx, sy) else { return };
    view.scroll_x -= dx as f32;
    view.scroll_y -= dy as f32;
    clamp_view(chart, view);
}

fn paint(
    mut s: &mut Surface,
    chart: &Chart,
    thumbs: &HashMap<String, TgaImage>,
    banner: Option<&TgaImage>,
    pager: Option<&TgaImage>,
    font: &render::Font,
    sel: usize,
    hover: Option<usize>,
    view: &View,
    quote: &str,
    chapter: Option<&Label>,
    hint: Option<&Label>,
) {
    s.fill_rect(0, 0, W as u32, H as u32, BG);
    // Lines first, so a thumbnail covers the joint.
    for node in &chart.nodes {
        let (x1, y1, w, h) = place(node, view);
        let ax = x1 + w;
        let ay = y1 + h / 2;
        for &j in &node.next {
            let (x2, y2, _, h2) = place(&chart.nodes[j], view);
            elbow(s, ax, ay, x2, y2 + h2 / 2);
        }
    }
    for (i, node) in chart.nodes.iter().enumerate() {
        let (x, y, w, h) = place(node, view);
        if x + w < 0 || x > W || y + h < 40 || y > PREVIEW_TOP {
            continue;
        }
        if let Some(img) = thumbs.get(&node.id) {
            blit_scaled(s, img, x, y, w, h);
        } else {
            s.fill_rect(x, y, w.max(1) as u32, h.max(1) as u32, [12, 36, 72, 255]);
        }
        let color = if i == sel {
            GOLD
        } else if hover == Some(i) {
            CYAN
        } else if chart.here == Some(i) {
            [120, 170, 210, 255]
        } else {
            [40, 70, 100, 255]
        };
        let thick = if i == sel { 3 } else { 1 };
        frame_rect(s, x, y, w, h, thick, color);
        if chart.here == Some(i) {
            s.fill_rect(x + w / 2 - 5, y + h + 4, 10, 6, CYAN);
        }
    }

    // Cover the chart where the header and the preview sit.
    s.fill_rect(0, 0, W as u32, 40, BG);
    if let Some(b) = banner {
        s.blit(b, 0, 0);
    }
    s.fill_rect(0, PREVIEW_TOP, W as u32, PREVIEW_H as u32, [1, 29, 89, 255]);
    s.fill_rect(0, PREVIEW_TOP, W as u32, 2, CYAN);

    let node = &chart.nodes[sel];
    let thumb_x = 28;
    let thumb_y = PREVIEW_TOP + 26;
    if let Some(big) = thumbs.get(&format!("preview:{}", node.id)) {
        s.blit(big, thumb_x, thumb_y);
    } else if let Some(img) = thumbs.get(&node.id) {
        s.blit(img, thumb_x, thumb_y);
    }
    frame_rect(s, thumb_x, thumb_y, PREVIEW_W, PREVIEW_H_THUMB, 2, GOLD);

    let text_x = thumb_x + PREVIEW_W + 20;
    let mut ty = PREVIEW_TOP + 16;
    for label in [chapter, hint].into_iter().flatten() {
        s.blit_rgba(&label.rgba, label.w, label.h, text_x, ty);
        ty += label.h as i32 + 2;
    }
    if !quote.is_empty() {
        font.draw_pitch(&mut s, quote, text_x, PREVIEW_TOP + 96, render::DIALOG_PITCH);
    }
    if let Some(p) = pager {
        s.blit(p, W - p.width as i32 - 16, H - p.height as i32 - 16);
    }
}

const PREVIEW_W: i32 = 208;
const PREVIEW_H_THUMB: i32 = 117;

/// Top of the chart, under the scene-replay banner.
const CHART_TOP: i32 = 48;

fn place(node: &Node, view: &View) -> (i32, i32, i32, i32) {
    let x = (world_x(node.col) * view.zoom - view.scroll_x).round() as i32;
    let y = (CHART_TOP as f32 + world_y(node.lane) * view.zoom - view.scroll_y).round() as i32;
    let w = (NODE_W as f32 * view.zoom).round().max(1.0) as i32;
    let h = (NODE_H as f32 * view.zoom).round().max(1.0) as i32;
    (x, y, w, h)
}

fn world_x(col: i32) -> f32 {
    28.0 + col as f32 * PITCH_X as f32
}

fn world_y(lane: i32) -> f32 {
    lane as f32 * PITCH_Y as f32
}

fn hit_node(chart: &Chart, view: &View, x: i32, y: i32) -> Option<usize> {
    if y >= PREVIEW_TOP || y < 40 {
        return None;
    }
    chart.nodes.iter().enumerate().rev().find_map(|(i, n)| {
        let (nx, ny, w, h) = place(n, view);
        if x >= nx && x < nx + w && y >= ny && y < ny + h {
            Some(i)
        } else {
            None
        }
    })
}

/// Zoom toward the cursor. The world point under `(mx, my)` stays there.
fn zoom_about(chart: &Chart, view: &mut View, mx: i32, my: i32, factor: f32) {
    let new = (view.zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
    if (new - view.zoom).abs() < 0.0001 {
        return;
    }
    let (sx, sy) = retarget(view.scroll_x, view.scroll_y, view.zoom, mx, my, new);
    view.zoom = new;
    view.scroll_x = sx;
    view.scroll_y = sy;
    clamp_view(chart, view);
}

/// Glide `zoom` toward `target_zoom`. `dt` is seconds since the previous frame,
/// so a slow frame takes a longer step instead of stuttering.
fn ease_zoom(chart: &Chart, view: &mut View, mx: i32, my: i32, dt: f32) {
    let goal = view.target_zoom.clamp(ZOOM_MIN, ZOOM_MAX);
    view.target_zoom = goal;
    let dz = goal - view.zoom;
    if dz.abs() < 0.0008 {
        if dz.abs() > 0.00001 {
            zoom_about(chart, view, mx, my, goal / view.zoom.max(0.0001));
        }
        return;
    }
    let k = 1.0 - (-dt / 0.06).exp();
    let next = view.zoom + dz * k;
    zoom_about(chart, view, mx, my, next / view.zoom.max(0.0001));
}

fn retarget(scroll_x: f32, scroll_y: f32, zoom: f32, mx: i32, my: i32, new_zoom: f32) -> (f32, f32) {
    let wx = (mx as f32 + scroll_x) / zoom;
    let wy = (my as f32 - CHART_TOP as f32 + scroll_y) / zoom;
    (
        wx * new_zoom - mx as f32,
        wy * new_zoom - (my as f32 - CHART_TOP as f32),
    )
}

fn hit_preview(x: i32, y: i32) -> bool {
    x >= 28 && x < 28 + PREVIEW_W && y >= PREVIEW_TOP + 26 && y < PREVIEW_TOP + 26 + PREVIEW_H_THUMB
}

/// Left third of the pager steps backward, right third forward.
fn hit_pager(pager: Option<&TgaImage>, x: i32, y: i32) -> Option<i32> {
    let p = pager?;
    let px = W - p.width as i32 - 16;
    let py = H - p.height as i32 - 16;
    if x < px || y < py || x >= px + p.width as i32 || y >= py + p.height as i32 {
        return None;
    }
    let rel = x - px;
    let third = p.width as i32 / 3;
    if rel < third {
        Some(-1)
    } else if rel >= third * 2 {
        Some(1)
    } else {
        None
    }
}

fn step(chart: &Chart, sel: usize, dir: i32) -> usize {
    let col = chart.nodes[sel].col;
    let lane = chart.nodes[sel].lane;
    let mut best: Option<(i32, usize)> = None;
    for (i, n) in chart.nodes.iter().enumerate() {
        if n.lane != lane {
            continue;
        }
        let d = n.col - col;
        if dir < 0 && d < 0 {
            let score = -d;
            if best.map(|(s, _)| score < s).unwrap_or(true) {
                best = Some((score, i));
            }
        } else if dir > 0 && d > 0 {
            if best.map(|(s, _)| d < s).unwrap_or(true) {
                best = Some((d, i));
            }
        }
    }
    best.map(|(_, i)| i).unwrap_or(sel)
}

fn step_lane(chart: &Chart, sel: usize, dir: i32) -> usize {
    let col = chart.nodes[sel].col;
    let lane = chart.nodes[sel].lane;
    let mut best: Option<(i32, i32, usize)> = None;
    for (i, n) in chart.nodes.iter().enumerate() {
        let dl = n.lane - lane;
        if dir < 0 && dl >= 0 || dir > 0 && dl <= 0 {
            continue;
        }
        let score = (dl.abs(), (n.col - col).abs());
        if best.map(|(a, b, _)| score < (a, b)).unwrap_or(true) {
            best = Some((score.0, score.1, i));
        }
    }
    best.map(|(_, _, i)| i).unwrap_or(sel)
}

fn focus_view(chart: &Chart, sel: usize, view: &mut View) {
    let (x, y, w, h) = place(&chart.nodes[sel], view);
    let margin = 24;
    if x < margin {
        view.scroll_x -= (margin - x) as f32;
    } else if x + w > W - margin {
        view.scroll_x += (x + w - (W - margin)) as f32;
    }
    let top = CHART_TOP + 8;
    let bot = PREVIEW_TOP - 8;
    if y < top {
        view.scroll_y -= (top - y) as f32;
    } else if y + h > bot {
        view.scroll_y += (y + h - bot) as f32;
    }
    clamp_view(chart, view);
}

fn clamp_view(chart: &Chart, view: &mut View) {
    let max_col = chart.nodes.iter().map(|n| n.col).max().unwrap_or(0);
    let max_lane = chart.nodes.iter().map(|n| n.lane).max().unwrap_or(0);
    let width = (world_x(max_col) + NODE_W as f32 + 40.0) * view.zoom;
    let height = (world_y(max_lane) + NODE_H as f32 + 16.0) * view.zoom;
    let max_x = (width - W as f32).max(0.0);
    let max_y = (height - (PREVIEW_TOP - CHART_TOP) as f32).max(0.0);
    view.scroll_x = view.scroll_x.clamp(0.0, max_x);
    view.scroll_y = view.scroll_y.clamp(0.0, max_y);
}

fn blit_scaled(dst: &mut Surface, img: &TgaImage, dx: i32, dy: i32, dw: i32, dh: i32) {
    if dw <= 0 || dh <= 0 || img.width == 0 || img.height == 0 {
        return;
    }
    // Chart band only. The header and the preview are painted on top, and a
    // straight copy here is what keeps a drag at frame rate.
    let x0 = dx.max(0);
    let y0 = dy.max(40);
    let x1 = (dx + dw).min(W);
    let y1 = (dy + dh).min(PREVIEW_TOP);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    if dw == img.width as i32 && dh == img.height as i32 && dx >= 0 && dy >= 40 {
        let stride = dst.width as usize * 4;
        let row_bytes = (x1 - x0) as usize * 4;
        let src_x = (x0 - dx) as usize;
        for y in y0..y1 {
            let si = ((y - dy) as usize * img.width as usize + src_x) * 4;
            let di = y as usize * stride + x0 as usize * 4;
            dst.pixels[di..di + row_bytes].copy_from_slice(&img.rgba[si..si + row_bytes]);
        }
        return;
    }
    let sw = img.width as i64;
    let sh = img.height as i64;
    let stride = dst.width as usize * 4;
    // Downscale by averaging the texels a pixel covers. Upscale blends the
    // four neighbours so a zoom does not turn the picture into stairs.
    let shrink = sw * 4 > dw as i64 * 5 || sh * 4 > dh as i64 * 5;
    for y in y0..y1 {
        let dst_row = y as usize * stride;
        for x in x0..x1 {
            let di = dst_row + x as usize * 4;
            let px = if shrink {
                let sx0 = ((x - dx) as i64 * sw / dw as i64).clamp(0, sw - 1);
                let sx1 = (((x + 1 - dx) as i64 * sw / dw as i64).clamp(sx0 + 1, sw)) as usize;
                let sy0 = ((y - dy) as i64 * sh / dh as i64).clamp(0, sh - 1);
                let sy1 = (((y + 1 - dy) as i64 * sh / dh as i64).clamp(sy0 + 1, sh)) as usize;
                let sx0 = sx0 as usize;
                let sy0 = sy0 as usize;
                let mut acc = [0u32; 4];
                let mut n = 0u32;
                for sy in sy0..sy1 {
                    let row = sy * img.width as usize * 4;
                    for sx in sx0..sx1 {
                        let si = row + sx * 4;
                        acc[0] += img.rgba[si] as u32;
                        acc[1] += img.rgba[si + 1] as u32;
                        acc[2] += img.rgba[si + 2] as u32;
                        acc[3] += img.rgba[si + 3] as u32;
                        n += 1;
                    }
                }
                let n = n.max(1);
                [
                    (acc[0] / n) as u8,
                    (acc[1] / n) as u8,
                    (acc[2] / n) as u8,
                    (acc[3] / n) as u8,
                ]
            } else {
                let fx = (x - dx) as f32 * sw as f32 / dw as f32;
                let fy = (y - dy) as f32 * sh as f32 / dh as f32;
                let x_i = (fx.floor() as i64).clamp(0, sw - 1) as usize;
                let y_i = (fy.floor() as i64).clamp(0, sh - 1) as usize;
                let x_n = (x_i as i64 + 1).min(sw - 1) as usize;
                let y_n = (y_i as i64 + 1).min(sh - 1) as usize;
                let tx = fx - x_i as f32;
                let ty = fy - y_i as f32;
                let mut out = [0u8; 4];
                for c in 0..4 {
                    let p00 = img.rgba[(y_i * img.width as usize + x_i) * 4 + c] as f32;
                    let p10 = img.rgba[(y_i * img.width as usize + x_n) * 4 + c] as f32;
                    let p01 = img.rgba[(y_n * img.width as usize + x_i) * 4 + c] as f32;
                    let p11 = img.rgba[(y_n * img.width as usize + x_n) * 4 + c] as f32;
                    let top = p00 + (p10 - p00) * tx;
                    let bot = p01 + (p11 - p01) * tx;
                    out[c] = (top + (bot - top) * ty).round().clamp(0.0, 255.0) as u8;
                }
                out
            };
            if px[3] == 255 {
                dst.pixels[di..di + 4].copy_from_slice(&px);
            } else if px[3] != 0 {
                let aa = px[3] as u32;
                let ia = 255 - aa;
                for c in 0..3 {
                    dst.pixels[di + c] = ((px[c] as u32 * aa + dst.pixels[di + c] as u32 * ia) / 255) as u8;
                }
                dst.pixels[di + 3] = 255;
            }
        }
    }
}

fn elbow(s: &mut Surface, x1: i32, y1: i32, x2: i32, y2: i32) {
    let minx = x1.min(x2);
    let maxx = x1.max(x2);
    let miny = y1.min(y2);
    let maxy = y1.max(y2);
    if maxx < 0 || minx > W || maxy < 40 || miny >= PREVIEW_TOP {
        return;
    }
    let mid = x1 + (x2 - x1) / 2;
    hline(s, x1, y1, mid - x1);
    vline(s, mid, y1.min(y2), (y1 - y2).abs());
    hline(s, mid, y2, x2 - mid);
}

fn hline(s: &mut Surface, x: i32, y: i32, w: i32) {
    if w < 0 {
        hline(s, x + w, y, -w);
        return;
    }
    let x0 = x.max(0);
    let x1 = (x + w).min(W);
    if y < 0 || y >= PREVIEW_TOP || x1 <= x0 {
        return;
    }
    s.fill_rect(x0, y, (x1 - x0) as u32, 2, LINE);
}

fn vline(s: &mut Surface, x: i32, y: i32, h: i32) {
    if h < 0 {
        vline(s, x, y + h, -h);
        return;
    }
    let y0 = y.max(0);
    let y1 = (y + h).min(PREVIEW_TOP);
    if x < 0 || x >= W || y1 <= y0 {
        return;
    }
    s.fill_rect(x, y0, 2, (y1 - y0) as u32, LINE);
}

fn frame_rect(s: &mut Surface, x: i32, y: i32, w: i32, h: i32, t: i32, c: [u8; 4]) {
    s.fill_rect(x, y, w as u32, t as u32, c);
    s.fill_rect(x, y + h - t, w as u32, t as u32, c);
    s.fill_rect(x, y, t as u32, h as u32, c);
    s.fill_rect(x + w - t, y, t as u32, h as u32, c);
}

fn load_thumbs(dirs: &[&Path], nodes: &[Node]) -> HashMap<String, TgaImage> {
    let mut out = HashMap::new();
    for (id, path) in scene_files(dirs, nodes) {
        let Ok(img) = crate::codec::image::decode_file(&path) else { continue };
        out.insert(format!("preview:{id}"), fit(&img, PREVIEW_W as u32, PREVIEW_H_THUMB as u32));
        // Three times the on-screen node, so a zoom still has pixels to sample
        // instead of stretching a 104×58 nearest-neighbor crumb.
        out.insert(id, fit(&img, (NODE_W * 3) as u32, (NODE_H * 3) as u32));
    }
    out
}

struct Pic {
    variant: i32,
    /// A 256×144 `tv*` stand-in of this same picture.
    thumb: bool,
    /// A `b*` background. A `v*`/`tv*` file fills a background id only when no
    /// `b*` was shipped for that number — otherwise it is an event CG that
    /// happens to share the six digits (`b101700` is a room, `tv101700` is not).
    background: bool,
    path: PathBuf,
}

struct PictureIndex {
    /// Background id → files named `b/tv` + `{id:04}{variant:02}`.
    bg: HashMap<i64, Vec<Pic>>,
    /// Event-CG id → files named `v/tv` + `{id * 10 + variant:06}`.
    cg: HashMap<i64, Vec<Pic>>,
}

/// One picture per scene. The file shared by the fewest scenes wins, so a
/// script that begins by repeating the previous location does not stamp that
/// location onto every following node. An event CG breaks a tie.
fn scene_files(dirs: &[&Path], nodes: &[Node]) -> HashMap<String, PathBuf> {
    let index = picture_index(dirs);
    let resolved: Vec<Vec<Option<PathBuf>>> = nodes
        .iter()
        .map(|node| node.shot.iter().map(|shot| resolve(&index, *shot)).collect())
        .collect();
    let mut owners: HashMap<PathBuf, usize> = HashMap::new();
    for list in &resolved {
        let mut seen = HashSet::new();
        for path in list.iter().flatten() {
            if seen.insert(path.clone()) {
                *owners.entry(path.clone()).or_default() += 1;
            }
        }
    }
    let mut out = HashMap::new();
    for (node, list) in nodes.iter().zip(resolved) {
        if let Some(path) = pick_picture(&node.shot, &list, &owners) {
            out.insert(node.id.clone(), path);
        }
    }
    out
}

fn picture_index(dirs: &[&Path]) -> PictureIndex {
    // Earlier directories win, so the Chinese patch overrides a same-named file.
    let mut files: HashMap<String, PathBuf> = HashMap::new();
    for dir in dirs.iter().rev() {
        let rd = match std::fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().to_ascii_lowercase();
            files.insert(name, ent.path());
        }
    }
    let mut index = PictureIndex { bg: HashMap::new(), cg: HashMap::new() };
    for (name, path) in &files {
        let stem = if let Some(stem) = name.strip_suffix(".tga") {
            stem
        } else if let Some(stem) = name.strip_suffix(".webp") {
            stem
        } else {
            continue;
        };
        let (thumb, rest) = if let Some(rest) = stem.strip_prefix("tv") {
            (true, rest)
        } else if let Some(rest) = stem.strip_prefix("b") {
            (false, rest)
        } else if let Some(rest) = stem.strip_prefix("v") {
            (false, rest)
        } else {
            continue;
        };
        if rest.len() != 6 || !rest.chars().all(|c| c.is_ascii_digit()) {
            continue;
        }
        let Ok(number) = rest.parse::<i64>() else { continue };
        let has_background = files.contains_key(&format!("b{rest}.tga"))
            || files.contains_key(&format!("b{rest}.webp"));
        if stem.starts_with("b") {
            index.bg.entry(number / 100).or_default().push(Pic {
                variant: (number % 100) as i32,
                thumb: false,
                background: true,
                path: path.clone(),
            });
            continue;
        }
        // `v` and `tv` are the event CG (full frame and its small copy).
        index.cg.entry(number / 10).or_default().push(Pic {
            variant: (number % 10) as i32,
            thumb,
            background: false,
            path: path.clone(),
        });
        if !has_background {
            index.bg.entry(number / 100).or_default().push(Pic {
                variant: (number % 100) as i32,
                thumb,
                background: false,
                path: path.clone(),
            });
        }
    }
    index
}

fn resolve(index: &PictureIndex, shot: (bool, i64, i32)) -> Option<PathBuf> {
    let (cg, id, variant) = shot;
    let pics = if cg { index.cg.get(&id)? } else { index.bg.get(&id)? };
    pics.iter()
        .min_by_key(|pic| {
            (
                u8::from(pic.variant != variant),
                // The shipped background wins over a CG that reuses the number.
                u8::from(!cg && !pic.background),
                u8::from(!pic.thumb),
                u8::from(pic.variant != 0),
                pic.variant,
            )
        })
        .map(|pic| pic.path.clone())
}

fn pick_picture(
    shots: &[(bool, i64, i32)],
    resolved: &[Option<PathBuf>],
    owners: &HashMap<PathBuf, usize>,
) -> Option<PathBuf> {
    let mut best: Option<(usize, u8, usize, PathBuf)> = None;
    let mut seen = HashSet::new();
    for (i, path) in resolved.iter().enumerate() {
        let Some(path) = path else { continue };
        if !seen.insert(path.clone()) {
            continue;
        }
        let cg_rank = u8::from(!shots[i].0);
        let rank = (*owners.get(path).unwrap_or(&1), cg_rank, i);
        if best.as_ref().map(|b| rank < (b.0, b.1, b.2)).unwrap_or(true) {
            best = Some((rank.0, rank.1, rank.2, path.clone()));
        }
    }
    best.map(|(_, _, _, path)| path)
}

/// `tv100100.tga` is script 1001, variant 00.
fn thumb_script_id(filename: &str) -> Option<String> {
    let stem = filename
        .strip_suffix(".tga")
        .or_else(|| filename.strip_suffix(".webp"))?
        .strip_prefix("tv")?;
    if stem.len() < 3 || !stem.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let id = &stem[..stem.len() - 2];
    if id.is_empty() { None } else { Some(id.to_string()) }
}

/// Area-average scale. Each output pixel is the mean of the source texels it
/// covers, which keeps edges from turning into stairs the way a single sample does.
fn fit(img: &TgaImage, dw: u32, dh: u32) -> TgaImage {
    let mut rgba = vec![0u8; (dw * dh * 4) as usize];
    if img.width == 0 || img.height == 0 || dw == 0 || dh == 0 {
        return TgaImage { width: dw, height: dh, rgba };
    }
    if img.width == dw && img.height == dh {
        let n = rgba.len().min(img.rgba.len());
        rgba[..n].copy_from_slice(&img.rgba[..n]);
        return TgaImage { width: dw, height: dh, rgba };
    }
    let sw = img.width as usize;
    let sh = img.height as usize;
    for y in 0..dh as usize {
        let y0 = y * sh / dh as usize;
        let y1 = ((y + 1) * sh / dh as usize).max(y0 + 1).min(sh);
        for x in 0..dw as usize {
            let x0 = x * sw / dw as usize;
            let x1 = ((x + 1) * sw / dw as usize).max(x0 + 1).min(sw);
            let mut acc = [0u32; 4];
            let mut n = 0u32;
            for sy in y0..y1 {
                let row = sy * sw * 4;
                for sx in x0..x1 {
                    let si = row + sx * 4;
                    acc[0] += img.rgba[si] as u32;
                    acc[1] += img.rgba[si + 1] as u32;
                    acc[2] += img.rgba[si + 2] as u32;
                    acc[3] += img.rgba[si + 3] as u32;
                    n += 1;
                }
            }
            let di = (y * dw as usize + x) * 4;
            let n = n.max(1);
            rgba[di] = (acc[0] / n) as u8;
            rgba[di + 1] = (acc[1] / n) as u8;
            rgba[di + 2] = (acc[2] / n) as u8;
            rgba[di + 3] = (acc[3] / n) as u8;
        }
    }
    TgaImage { width: dw, height: dh, rgba }
}

fn preview_line(script_dir: &Path, id: &str) -> String {
    let Ok(bytes) = std::fs::read(script_dir.join(format!("{id}.txt"))) else {
        return String::new();
    };
    let text = sjis::decode(&bytes);
    for raw in text.split(',') {
        let t = raw.trim().trim_matches(['\r', '\n', ' ']);
        if t.chars().count() < 4 || t.chars().all(|c| c.is_ascii()) {
            continue;
        }
        if t.starts_with("CATCH") {
            continue;
        }
        let flat = t.replace("\\n", " ").replace(['\n', '\r'], " ");
        let flat = strip_tags(&flat);
        let flat = flat.split_whitespace().collect::<Vec<_>>().join(" ");
        if flat.chars().count() < 4 {
            continue;
        }
        return flat.chars().take(18).collect();
    }
    String::new()
}

fn strip_tags(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '<' {
            for d in chars.by_ref() {
                if d == '>' {
                    break;
                }
            }
        } else {
            out.push(c);
        }
    }
    out
}

fn load_tga(dirs: &[&Path], name: &str) -> Option<TgaImage> {
    for dir in dirs {
        for cand in crate::codec::image::candidate_names(name) {
            if let Ok(img) = crate::codec::image::decode_file(&dir.join(cand)) {
                return Some(img);
            }
        }
    }
    None
}

impl Chart {
    fn load(script_dir: &Path) -> Chart {
        let nodes = build_nodes(script_dir);
        let here = newest_entry().and_then(|id| nodes.iter().position(|n| n.id == id));
        Chart { nodes, here }
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

fn build_nodes(script_dir: &Path) -> Vec<Node> {
    let rd = match std::fs::read_dir(script_dir) {
        Ok(r) => r,
        Err(_) => return Vec::new(),
    };
    let mut edges: HashMap<String, Vec<String>> = HashMap::new();
    // stem → pictures, so a subscript's CG is not thrown away when the entry
    // file opens by restating the previous scene's background.
    let mut shots: HashMap<String, Vec<(String, Vec<(bool, i64, i32)>)>> = HashMap::new();
    let mut ids: Vec<String> = Vec::new();
    for ent in rd.flatten() {
        let name = ent.file_name().to_string_lossy().to_string();
        let Some(stem) = name.strip_suffix(".bnr") else { continue };
        let entry = entry_of(stem);
        if !wanted(entry) {
            continue;
        }
        if !ids.iter().any(|id| id == entry) {
            ids.push(entry.to_string());
        }
        let txt_path = ent.path().with_extension("txt");
        let Ok(bnr) = std::fs::read(ent.path()) else { continue };
        let Ok(txt) = std::fs::read(&txt_path) else { continue };
        // Shot names and jump targets are ASCII resource tokens, so the
        // Chinese-patch glyph reading is not needed here.
        let Ok(prog) = Program::load(&bnr, &txt, false) else { continue };
        let found = scene_shots(&prog);
        if !found.is_empty() {
            shots.entry(entry.to_string()).or_default().push((stem.to_string(), found));
        }
        let list = edges.entry(entry.to_string()).or_default();
        for target in script_jumps(&prog) {
            let to = entry_of(&target);
            if to != entry && wanted(to) && !list.iter().any(|t| t == to) {
                list.push(to.to_string());
            }
        }
    }
    ids.sort();
    // A jump target that has no file of its own still needs a node.
    for list in edges.values() {
        for to in list {
            if !ids.iter().any(|id| id == to) {
                ids.push(to.clone());
            }
        }
    }
    ids.sort();
    ids.dedup();

    let index: HashMap<&str, usize> = ids.iter().enumerate().map(|(i, id)| (id.as_str(), i)).collect();
    let mut nodes: Vec<Node> = ids
        .iter()
        .map(|id| {
            let (lane, chapter) = lane_chapter(id);
            let next = edges
                .get(id)
                .map(|list| {
                    list.iter().filter_map(|to| index.get(to.as_str()).copied()).collect()
                })
                .unwrap_or_default();
            Node {
                id: id.clone(),
                chapter,
                col: 0,
                lane,
                next,
                shot: merge_shots(id, shots.get(id)),
            }
        })
        .collect();

    // Longest path from the roots. Extras (5xxx) are placed after the story
    // so a disconnected chain does not sit on top of 序章.
    let n = nodes.len();
    for _ in 0..n {
        let mut moved = false;
        for i in 0..n {
            let col = nodes[i].col;
            let next = nodes[i].next.clone();
            for j in next {
                if nodes[j].col < col + 1 {
                    nodes[j].col = col + 1;
                    moved = true;
                }
            }
        }
        if !moved {
            break;
        }
    }
    let max_story = nodes
        .iter()
        .filter(|node| !node.id.starts_with('5'))
        .map(|node| node.col)
        .max()
        .unwrap_or(0);
    for node in &mut nodes {
        if node.id.starts_with('5') {
            node.col += max_story + 2;
        }
    }
    uncollide(&mut nodes);
    nodes
}

fn uncollide(nodes: &mut [Node]) {
    for _ in 0..nodes.len() {
        let mut clash = None;
        for i in 0..nodes.len() {
            for j in (i + 1)..nodes.len() {
                if nodes[i].col == nodes[j].col && nodes[i].lane == nodes[j].lane {
                    clash = Some(if nodes[i].id > nodes[j].id { i } else { j });
                    break;
                }
            }
            if clash.is_some() {
                break;
            }
        }
        let Some(j) = clash else { return };
        nodes[j].col += 1;
    }
}

/// Backgrounds and event CGs with a real id, in script order. id 0 and id -2
/// are the fade-out and the black plate, not a picture of the scene.
fn scene_shots(prog: &Program) -> Vec<(bool, i64, i32)> {
    let mut ints = Vec::new();
    let mut out = Vec::new();
    for op in &prog.ops {
        match op {
            Op::Rec { .. } => ints.clear(),
            Op::Int { v, .. } => ints.push(*v),
            Op::Act { id: act @ (146 | 147 | 148), .. } => {
                if ints.len() >= 4 && ints[1] > 0 {
                    let shot = (*act == 148, ints[1] as i64, ints[2].max(0));
                    if !out.contains(&shot) {
                        out.push(shot);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Entry file first, then subscripts by name. Later copies of a picture drop.
fn merge_shots(
    entry: &str,
    parts: Option<&Vec<(String, Vec<(bool, i64, i32)>)>>,
) -> Vec<(bool, i64, i32)> {
    let mut parts = parts.cloned().unwrap_or_default();
    parts.sort_by(|a, b| (a.0 != entry).cmp(&(b.0 != entry)).then(a.0.cmp(&b.0)));
    let mut out = Vec::new();
    for (_, found) in parts {
        for shot in found {
            if !out.contains(&shot) {
                out.push(shot);
            }
        }
    }
    out
}

fn script_jumps(prog: &Program) -> Vec<String> {
    let mut out = Vec::new();
    let mut tok: Option<u32> = None;
    for op in &prog.ops {
        match op {
            Op::Rec { .. } => tok = None,
            Op::Tok { n, .. } => tok = Some(*n),
            Op::Act { id: 0, .. } => {
                if let Some(n) = tok {
                    if let Some(t) = prog.tokens.get(n as usize) {
                        let t = t.trim();
                        if is_script_id(t) {
                            out.push(t.to_string());
                        }
                    }
                }
            }
            _ => {}
        }
    }
    out
}

fn entry_of(id: &str) -> &str {
    id.split('_').next().unwrap_or(id)
}

fn is_script_id(s: &str) -> bool {
    !s.is_empty()
        && !s.starts_with('_')
        && s.chars().any(|c| c.is_ascii_digit())
        && s.chars().all(|c| c.is_ascii_digit() || c == '_')
}

fn wanted(id: &str) -> bool {
    let Ok(n) = entry_of(id).parse::<i32>() else { return false };
    matches!(
        n,
        1000..=1013
            | 2000..=2033
            | 2300..=2322
            | 2400..=2413
            | 2500..=2517
            | 3000..=3024
            | 3100..=3111
            | 3200..=3211
            | 3900..=3909
            | 5000..=5104
    )
}

/// Lane 0 is the top row. Side routes share a row with a coda ending only
/// where their columns do not meet.
fn lane_chapter(id: &str) -> (i32, &'static str) {
    let n = entry_of(id).parse::<i32>().unwrap_or(0);
    // 2025 is inside the 雪菜 numbers but it is a side ending, not the spine.
    if n == 2025 {
        return (5, "终章 · 雪菜");
    }
    match n {
        2400..=2413 => (0, "终章 · 千晶"),
        2300..=2322 => (1, "终章 · 小春"),
        3200..=3211 => (1, "Coda · 雪菜"),
        1000..=1013 => (2, "序章"),
        2001..=2019 => (2, "终章 · 共通"),
        2020..=2033 => (2, "终章 · 雪菜"),
        3000..=3024 => (2, "Coda"),
        2500..=2517 => (3, "终章 · 麻理"),
        3100..=3111 => (4, "Coda · 和纱"),
        3900..=3909 => (5, "Coda"),
        5000..=5004 => (0, "序章特典"),
        5100..=5104 => (3, "终章特典"),
        _ => (5, "其他"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn common_route_reaches_four_branches_without_subscripts() {
        let nodes = build_nodes(Path::new("data/script"));
        assert!(nodes.len() > 40, "expected the chapter scripts, got {}", nodes.len());
        assert!(nodes.iter().all(|n| !n.id.contains('_')));
        let at = |id: &str| nodes.iter().position(|n| n.id == id).unwrap();
        assert!(nodes[at("1001")].col < nodes[at("1013")].col);
        assert!(nodes[at("1013")].col < nodes[at("2001")].col);
        assert!(nodes[at("2001")].col < nodes[at("2019")].col);
        let next: Vec<&str> = nodes[at("2019")].next.iter().map(|&i| nodes[i].id.as_str()).collect();
        for id in ["2020", "2301", "2401", "2501"] {
            assert!(next.contains(&id), "2019 should reach {id}, got {next:?}");
            assert!(nodes[at("2019")].col < nodes[at(id)].col);
        }
        // No two scenes occupy the same cell.
        let mut seen = std::collections::HashSet::new();
        for n in &nodes {
            assert!(seen.insert((n.col, n.lane)), "overlap {} at {},{}", n.id, n.col, n.lane);
        }
        assert_eq!(thumb_script_id("tv100100.tga").as_deref(), Some("1001"));
        let pictured = nodes.iter().filter(|n| !n.shot.is_empty()).count();
        assert!(pictured * 10 > nodes.len() * 8, "most scenes should name a picture, {pictured}/{}", nodes.len());
        assert!(!nodes[at("2301")].shot.is_empty());
        assert!(!nodes[at("3001")].shot.is_empty());
    }

    #[test]
    fn neighbouring_scenes_do_not_share_a_thumbnail() {
        let bg = Path::new("data/bg");
        if !bg.is_dir() {
            return;
        }
        let nodes = build_nodes(Path::new("data/script"));
        let chosen = scene_files(&[bg], &nodes);
        let name = |id: &str| {
            chosen.get(id).and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned())
        };
        assert_ne!(name("1002"), name("1003"), "opening street reused");
        assert_ne!(name("1012"), name("1013"), "bedroom reused");
        assert_ne!(name("1003").as_deref(), Some("tv100100.tga"));
        let distinct = chosen.values().collect::<std::collections::HashSet<_>>().len();
        assert!(
            distinct > 100,
            "thumbnails collapsed onto {distinct} files for {} scenes",
            chosen.len()
        );
        let index = picture_index(&[bg]);
        let file = |shot| {
            resolve(&index, shot)
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_default()
        };
        assert_eq!(file((false, 1001, 0)), "tv100100.tga");
        assert_eq!(file((false, 1055, 0)), "b105503.tga");
        assert_eq!(file((true, 10030, 0)), "tv100300.tga");
        // b101700 is the room. tv101700 is the event CG that shares the number.
        assert_eq!(file((false, 1017, 0)), "b101700.tga");
        assert_eq!(file((true, 10170, 0)), "tv101700.tga");
        assert_ne!(name("3211").as_deref(), Some("tv101700.tga"));
    }

    #[test]
    fn thumbnail_downscale_averages_neighbouring_pixels() {
        let img = TgaImage {
            width: 2,
            height: 2,
            rgba: vec![
                0, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255,
            ],
        };
        let out = fit(&img, 1, 1);
        assert_eq!(&out.rgba[..4], &[63, 63, 63, 255]);
    }

    #[test]
    fn a_zoomed_thumbnail_blends_neighbouring_pixels() {
        let img = TgaImage {
            width: 2,
            height: 1,
            rgba: vec![0, 0, 0, 255, 255, 255, 255, 255],
        };
        let mut surf = Surface::new(1280, 720, [0, 0, 0, 255]);
        blit_scaled(&mut surf, &img, 0, 40, 4, 1);
        let px = |x: i32| surf.pixels[((40 * 1280 + x) * 4) as usize];
        assert_eq!(px(0), 0);
        assert!(px(1) > 40 && px(1) < 220, "zoom should blend, got {}", px(1));
        assert_eq!(px(2), 255);
    }

    #[test]
    fn a_drag_follows_the_cursor_and_a_click_does_not() {
        let mut held = Hold { ax: 100, ay: 80, panning: false, chart: true };
        assert!(begin_pan(&mut held, 103, 82).is_none(), "a small move is still a click");
        assert!(!held.panning);
        // The whole gesture counts, including the part before the slop.
        assert_eq!(begin_pan(&mut held, 160, 80), Some((60, 0)));
        assert_eq!(begin_pan(&mut held, 140, 90), Some((-20, 10)));
        let mut off = Hold { ax: 10, ay: 600, panning: false, chart: false };
        assert!(begin_pan(&mut off, 80, 600).is_none(), "the preview bar does not pan");
    }

    #[test]
    fn zoom_keeps_the_point_under_the_cursor() {
        let (sx, sy) = retarget(400.0, 30.0, 1.0, 500, 200, 2.0);
        let wx = (500.0 + sx) / 2.0;
        let wy = (200.0 - CHART_TOP as f32 + sy) / 2.0;
        assert!((wx - (500.0 + 400.0)).abs() < 0.01, "world x moved to {wx}");
        assert!((wy - (200.0 - CHART_TOP as f32 + 30.0)).abs() < 0.01, "world y moved to {wy}");
    }

    #[test]
    fn zoom_glides_toward_the_wheel_and_stays_under_the_cursor() {
        let chart = Chart {
            nodes: vec![Node {
                id: "1".into(),
                chapter: "序章",
                col: 20,
                lane: 5,
                next: vec![],
                shot: vec![],
            }],
            here: None,
        };
        let mut view = View {
            zoom: 1.0,
            target_zoom: 1.5,
            scroll_x: 200.0,
            scroll_y: 0.0,
        };
        let (mx, my) = (400, 180);
        let wx = (mx as f32 + view.scroll_x) / view.zoom;
        ease_zoom(&chart, &mut view, mx, my, 1.0 / 60.0);
        assert!(view.zoom > 1.02 && view.zoom < 1.25, "first step jumped to {}", view.zoom);
        let wx2 = (mx as f32 + view.scroll_x) / view.zoom;
        assert!((wx - wx2).abs() < 1.5, "world x {wx} -> {wx2}");
        for _ in 0..48 {
            ease_zoom(&chart, &mut view, mx, my, 1.0 / 60.0);
        }
        assert!((view.zoom - 1.5).abs() < 0.01, "settled at {}", view.zoom);
    }

    #[test]
    fn chart_thumbnail_is_copied_onto_the_node() {
        let mut nodes = Vec::new();
        for i in 0..160 {
            nodes.push(Node {
                id: format!("{i}"),
                chapter: "序章",
                col: i as i32 % 40,
                lane: (i % 6) as i32,
                next: if i + 1 < 160 { vec![i + 1] } else { vec![] },
                shot: vec![],
            });
        }
        let chart = Chart { nodes, here: Some(0) };
        let tile = TgaImage {
            width: NODE_W as u32,
            height: NODE_H as u32,
            rgba: vec![40, 80, 120, 255].repeat((NODE_W * NODE_H) as usize),
        };
        let mut thumbs = HashMap::new();
        for n in &chart.nodes {
            thumbs.insert(n.id.clone(), tile.clone());
        }
        let font = render::Font::new(
            TgaImage { width: 16, height: 16, rgba: vec![0; 16 * 16 * 4] },
            16,
            HashMap::new(),
        );
        let view = View { zoom: 1.0, target_zoom: 1.0, scroll_x: 0.0, scroll_y: 0.0 };
        let mut frame = Surface::new(W as u32, H as u32, BG);
        paint(
            &mut frame,
            &chart,
            &thumbs,
            None,
            None,
            &font,
            0,
            None,
            &view,
            "",
            None,
            None,
        );
        let i = ((CHART_TOP as usize + 12) * W as usize + 40) * 4;
        assert_eq!(&frame.pixels[i..i + 4], &[40, 80, 120, 255], "thumbnail did not land on the chart");
    }
}
