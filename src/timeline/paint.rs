//! Drawing the timeline chart: chapter bands, the fixed chapter header bar,
//! scenario edges (arrowheads, branch colour/dashes and labels), the node cards
//! and their markers, the preview bar and the minimap.
//!
//! Everything here is logical 1280x720 pixels. \`nav::place\` maps a node's world
//! cell onto the surface; headers, node tags and the preview text stay at fixed
//! pixel sizes so they do not shrink when the chart zooms out. The only text
//! drawn here is short (chapter names, edge labels, END/入口, debug strings);
//! it is rasterized once through preview::label and cached, so a frame loop
//! never re-rasterizes a line of text.

use super::*;
use crate::codec::tga::TgaImage;
use crate::render::Surface;
use std::collections::HashMap;
use std::sync::{Mutex, MutexGuard, OnceLock};

/// Everything paint needs for one frame. The run loop owns the state; paint
/// only reads it.
pub(super) struct PaintState<'a> {
    pub chart: &'a Chart,
    pub thumbs: &'a ThumbSet,
    pub banner: Option<&'a TgaImage>,
    pub pager: Option<&'a TgaImage>,
    pub sel: usize,
    pub hover: Option<usize>,
    pub view: &'a View,
    pub preview: &'a Preview,
    /// One header label per chart band, in chart.bands order.
    pub headers: &'a [Option<Label>],
}

// Layout owned by paint. The banner is 0..36, the chapter header bar is the
// fixed strip 42..66 (the chart starts at CHART_TOP=72), the preview bar is
// PREVIEW_TOP..H and the minimap sits inside its right end.
const HEADER_TOP: i32 = 42;
const HEADER_H: i32 = 24;
const HEADER_BOTTOM: i32 = HEADER_TOP + HEADER_H;
/// Preview text column: left edge and the right limit the lead fixed so the
/// quote can never run under the minimap.
const TEXT_X: i32 = 256;
const TEXT_RIGHT: i32 = 1030;
const MINI_X: i32 = 1040;
const MINI_Y: i32 = PREVIEW_TOP + 8;
const MINI_W: i32 = 224;
const MINI_H: i32 = 100;
/// Clear gap between an arrow tip and the node it points at.
const EDGE_GAP: i32 = 7;

const HEADER_BG: [u8; 4] = [2, 14, 36, 255];
const HEADER_LINE: [u8; 4] = [34, 62, 98, 255];
const PREVIEW_BG: [u8; 4] = [1, 20, 48, 255];
const MINI_BG: [u8; 4] = [2, 14, 34, 255];
const MINI_FRAME: [u8; 4] = [46, 80, 120, 255];
const LINE_DIM: [u8; 4] = [86, 120, 152, 255];
const TAG_BG: [u8; 4] = [0, 8, 20, 210];

pub(super) fn paint(s: &mut Surface, st: &PaintState) {
    s.fill_rect(0, 0, W as u32, H as u32, BG);
    draw_bands(s, st.chart, st.view);
    let tags = draw_edges(s, st.chart, st.view);
    draw_nodes(s, st);
    draw_edge_tags(s, &tags);
    draw_top(s, st);
    draw_preview_bar(s, st);
}

// ---------------------------------------------------------------- colours ---

/// Blend \`b\` into \`a\`; \`percent_b\` is 0..=100. Opaque output, so the fast
/// fill_rect path stays available for the large band rectangles.
fn mix(a: [u8; 4], b: [u8; 4], percent_b: u32) -> [u8; 4] {
    let t = percent_b.min(100);
    let f = |x: u8, y: u8| ((x as u32 * (100 - t) + y as u32 * t) / 100) as u8;
    [f(a[0], b[0]), f(a[1], b[1]), f(a[2], b[2]), 255]
}

fn rgb(c: [u8; 4]) -> [u8; 3] {
    [c[0], c[1], c[2]]
}

/// Low-saturation frame for a node of this chapter.
fn route_frame(chapter: &Chapter) -> [u8; 4] {
    mix([30, 40, 54, 255], chapter.color, 55)
}

/// Bright version for accents, arrows and tags.
fn route_bright(chapter: &Chapter) -> [u8; 4] {
    mix(chapter.color, [255, 255, 255, 255], 18)
}

fn route_ink(chapter: &Chapter) -> [u8; 3] {
    rgb(route_bright(chapter))
}

fn band_fill(chapter: &Chapter) -> [u8; 4] {
    mix(BG, chapter.color, 20)
}

fn band_edge(chapter: &Chapter) -> [u8; 4] {
    mix(BG, chapter.color, 58)
}

// ------------------------------------------------------------------- text ---

/// Rasterize short text once and reuse it. The cache is keyed by the font
/// generation, the pixel size and the text: the timeline has a fixed chapter
/// table, so it stays small for a session, and a font change made in the
/// option screen reaches the chart the next time it is opened instead of
/// after a restart.
fn with_label<R>(text: &str, size: u32, f: impl FnOnce(&Label) -> R) -> Option<R> {
    static CACHE: OnceLock<Mutex<HashMap<(u64, u32, String), Option<Label>>>> = OnceLock::new();
    let cache = CACHE.get_or_init(|| Mutex::new(HashMap::new()));
    let key = (crate::render::text::role_generation(), size, text.to_string());
    let mut guard = cache.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    let entry = guard.entry(key).or_insert_with(|| preview::label(text, size));
    entry.as_ref().map(f)
}

fn label_dims(text: &str, size: u32) -> (i32, i32) {
    with_label(text, size, |label| (label.w as i32, label.h as i32)).unwrap_or((0, 0))
}

/// Chapter short names for the header bar when the zoomed-out band is too
/// narrow for the full title. Rasterized on first use, one per band, and
/// rebuilt when the font roles change. The guard is returned so the caller can
/// read the labels without cloning their buffers.
fn short_headers(chart: &Chart) -> MutexGuard<'static, (u64, Vec<Option<Label>>)> {
    static SHORT: Mutex<(u64, Vec<Option<Label>>)> = Mutex::new((0, Vec::new()));
    let generation = crate::render::text::role_generation();
    let mut guard = SHORT.lock().unwrap_or_else(|poisoned| poisoned.into_inner());
    if guard.0 != generation || guard.1.len() != chart.bands.len() {
        guard.1 = chart
            .bands
            .iter()
            .map(|band| preview::label(band.chapter.short, 15))
            .collect();
        guard.0 = generation;
    }
    guard
}

#[derive(Clone, Copy)]
enum Anchor {
    TopLeft,
    BottomLeft,
    BottomRight,
}

/// A small dark chip with tinted text inside a node rectangle. Used for the
/// END / 入口 / branch labels so the thumbnail behind them cannot wash them out.
fn draw_tag_in_rect(
    s: &mut Surface,
    text: &str,
    size: u32,
    rect: (i32, i32, i32, i32),
    anchor: Anchor,
    tint: [u8; 3],
) {
    with_label(text, size, |label| {
        let w = label.w as i32;
        let h = label.h as i32;
        let (rx, ry, rw, rh) = rect;
        if w == 0 || w + 6 > rw || h + 2 > rh {
            return;
        }
        let (x, y) = match anchor {
            Anchor::TopLeft => (rx + 3, ry + 2),
            Anchor::BottomLeft => (rx + 3, ry + rh - h - 2),
            Anchor::BottomRight => (rx + rw - w - 3, ry + rh - h - 2),
        };
        s.fill_rect(x - 2, y, (label.w + 4) as u32, label.h, TAG_BG);
        blit_tinted_clip(s, label, x, y, tint, W);
    });
}

/// Blit coverage-only label pixels with a flat tint and a right-hand clip.
fn blit_tinted_clip(s: &mut Surface, label: &Label, dx: i32, dy: i32, tint: [u8; 3], x_max: i32) {
    let x_max = x_max.min(s.width as i32);
    for row in 0..label.h as i32 {
        let y = dy + row;
        if y < 0 || y >= s.height as i32 {
            continue;
        }
        for col in 0..label.w as i32 {
            let x = dx + col;
            if x < 0 || x >= x_max {
                continue;
            }
            let i = ((row as u32 * label.w + col as u32) * 4) as usize;
            let a = label.rgba[i + 3];
            if a == 0 {
                continue;
            }
            s.blend_pixel(x, y, [tint[0], tint[1], tint[2], a]);
        }
    }
}

fn debug_timeline() -> bool {
    static FLAG: OnceLock<bool> = OnceLock::new();
    *FLAG.get_or_init(|| {
        std::env::var("WA2_DEBUG_TIMELINE")
            .map(|value| value.trim() != "0" && !value.trim().is_empty())
            .unwrap_or(false)
    })
}

// ---------------------------------------------------------------- drawing ---

/// Clip a fill to the chart viewport (between the header bar and the preview
/// bar) so edges can never bleed into the fixed UI strips.
fn chart_px(s: &mut Surface, x: i32, y: i32, w: i32, h: i32, color: [u8; 4]) {
    if w <= 0 || h <= 0 {
        return;
    }
    let x0 = x.max(0);
    let y0 = y.max(CHART_TOP);
    let x1 = (x + w).min(W);
    let y1 = (y + h).min(PREVIEW_TOP);
    if x1 <= x0 || y1 <= y0 {
        return;
    }
    s.fill_rect(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32, color);
}

fn hseg(s: &mut Surface, x0: i32, x1: i32, y: i32, thick: i32, color: [u8; 4]) {
    let (a, b) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    chart_px(s, a, y, b - a, thick, color);
}

fn vseg(s: &mut Surface, y0: i32, y1: i32, x: i32, thick: i32, color: [u8; 4]) {
    let (a, b) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
    chart_px(s, x, a, thick, b - a, color);
}

fn dashed_h(s: &mut Surface, x0: i32, x1: i32, y: i32, thick: i32, color: [u8; 4]) {
    let (mut a, b) = if x0 <= x1 { (x0, x1) } else { (x1, x0) };
    let mut on = true;
    while a < b {
        let step = if on { 6 } else { 4 };
        let next = (a + step).min(b);
        if on {
            hseg(s, a, next, y, thick, color);
        }
        a = next;
        on = !on;
    }
}

fn dashed_v(s: &mut Surface, y0: i32, y1: i32, x: i32, thick: i32, color: [u8; 4]) {
    let (mut a, b) = if y0 <= y1 { (y0, y1) } else { (y1, y0) };
    let mut on = true;
    while a < b {
        let step = if on { 6 } else { 4 };
        let next = (a + step).min(b);
        if on {
            vseg(s, a, next, x, thick, color);
        }
        a = next;
        on = !on;
    }
}

/// Filled triangle pointing right; \`tip\` is clear of the target node by
/// EDGE_GAP so the head never touches the thumbnail.
fn arrow_right(s: &mut Surface, tip: i32, cy: i32, size: i32, thick: i32, color: [u8; 4]) {
    for i in 0..=size {
        let half = size - i;
        chart_px(s, tip - size + i, cy - half, thick, 2 * half + 1, color);
    }
}

/// A long / cross-chapter edge: a flat S-curve instead of an elbow, so it reads
/// as a jump rather than as neighbouring scenes.
fn curved_edge(s: &mut Surface, ax: i32, ay: i32, ex: i32, ey: i32, thick: i32, color: [u8; 4]) {
    let dx = (ex - ax) as f32;
    if dx.abs() < 4.0 {
        hseg(s, ax, ex, ay - thick / 2, thick, color);
        return;
    }
    let c1x = ax as f32 + dx * 0.35;
    let c2x = ex as f32 - dx * 0.35;
    let steps = ((dx.abs() * 1.5) as i32).clamp(16, 160);
    let mut last = (ax, ay);
    for i in 1..=steps {
        let t = i as f32 / steps as f32;
        let mt = 1.0 - t;
        let x = mt * mt * mt * ax as f32
            + 3.0 * mt * mt * t * c1x
            + 3.0 * mt * t * t * c2x
            + t * t * t * ex as f32;
        let y = mt * mt * mt * ay as f32
            + 3.0 * mt * mt * t * ay as f32
            + 3.0 * mt * t * t * ey as f32
            + t * t * t * ey as f32;
        let point = (x.round() as i32, y.round() as i32);
        if point != last {
            chart_px(s, point.0, point.1 - thick / 2, thick, thick, color);
            last = point;
        }
    }
    chart_px(s, ex, ey - thick / 2, thick, thick, color);
}

// ------------------------------------------------------------------ bands ---

/// One translucent-looking background strip per chapter: its column span times
/// its lane extent, with brighter boundary rules. The fill is pre-blended with
/// the chart background (opaque) so the whole chart can still be redrawn with
/// the fast rect-copy path each frame.
fn draw_bands(s: &mut Surface, chart: &Chart, view: &View) {
    let zoom = view.zoom;
    for band in &chart.bands {
        let x0 = (nav::world_x(band.first_col) * zoom - view.scroll_x).round() as i32 - 5;
        let x1 = (nav::world_x(band.last_col + 1) * zoom - view.scroll_x).round() as i32 + 5;
        if x1 <= 0 || x0 >= W {
            continue;
        }
        let y0 = (CHART_TOP as f32 + nav::world_y(band.min_lane) * zoom - view.scroll_y).round() as i32 - 5;
        let y1 =
            (CHART_TOP as f32 + nav::world_y(band.max_lane + 1) * zoom - view.scroll_y).round() as i32 + 5;
        let x0 = x0.max(0);
        let x1 = x1.min(W);
        let y0 = y0.max(HEADER_BOTTOM);
        let y1 = y1.min(PREVIEW_TOP);
        if x1 - x0 < 2 || y1 - y0 < 4 {
            continue;
        }
        s.fill_rect(x0, y0, (x1 - x0) as u32, (y1 - y0) as u32, band_fill(band.chapter));
        let edge = band_edge(band.chapter);
        s.fill_rect(x0, y0, (x1 - x0) as u32, 1, edge);
        s.fill_rect(x0, y1 - 1, (x1 - x0) as u32, 1, edge);
        s.fill_rect(x0, y0, 1, (y1 - y0) as u32, edge);
        s.fill_rect(x1 - 1, y0, 1, (y1 - y0) as u32, edge);
    }
}

// ------------------------------------------------------------------ edges ---

struct EdgeTag {
    text: String,
    /// Target node rectangle.
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    /// Ending nodes carry END at the bottom right, so their edge label moves up.
    ending: bool,
    tint: [u8; 3],
}

fn draw_edges(s: &mut Surface, chart: &Chart, view: &View) -> Vec<EdgeTag> {
    let zoom = view.zoom;
    let mut tags: Vec<EdgeTag> = Vec::new();
    let mut tagged = vec![false; chart.nodes.len()];
    for node in &chart.nodes {
        let (x, y, w, h) = nav::place(node, view);
        let ax = x + w;
        let ay = y + h / 2;
        for edge in &node.next {
            let target = &chart.nodes[edge.to];
            let (tx, ty, tw, th) = nav::place(target, view);
            let ex = tx - EDGE_GAP;
            let ey = ty + th / 2;
            let branch = edge.kind == EdgeKind::Branch;
            let cross = !std::ptr::eq(target.chapter, node.chapter);
            let long = cross || target.col - node.col > 1;
            // Cheap bounding-box cull before touching the surface.
            let minx = ax.min(ex);
            let maxx = ax.max(ex);
            let miny = ay.min(ey);
            let maxy = ay.max(ey);
            if maxx < 0 || minx > W || maxy < CHART_TOP || miny >= PREVIEW_TOP {
                continue;
            }
            let color = if branch {
                route_bright(target.chapter)
            } else if long {
                mix(LINE, target.chapter.color, 35)
            } else {
                LINE_DIM
            };
            let thick = if branch || long { 2 } else { 1 };
            if branch {
                // The four edges out of one branch point share a single stem;
                // draw it in the source chapter's colour so the last target's
                // colour does not repaint the whole trunk. The arriving arm
                // keeps the target's colour.
                let stem = route_bright(node.chapter);
                let mid = ax + ((ex - ax) / 2).max(14);
                dashed_h(s, ax, mid, ay - thick / 2, thick, stem);
                dashed_v(s, ay, ey, mid - thick / 2, thick, stem);
                dashed_h(s, mid, ex, ey - thick / 2, thick, color);
            } else if long {
                curved_edge(s, ax, ay, ex, ey, thick, color);
            } else if ay == ey {
                hseg(s, ax, ex, ay - thick / 2, thick, color);
            } else {
                let mid = ax + (ex - ax) / 2;
                hseg(s, ax, mid, ay - thick / 2, thick, color);
                vseg(s, ay, ey, mid - thick / 2, thick, color);
                hseg(s, mid, ex, ey - thick / 2, thick, color);
            }
            // The head scales with the chart so a 2x zoom does not leave a
            // crumb-sized arrow at the end of a long gap.
            let head = if branch || long {
                ((5.0 * zoom) as i32).clamp(4, 8)
            } else {
                ((4.0 * zoom) as i32).clamp(3, 7)
            };
            arrow_right(s, ex, ey, head, thick, color);

            // Branch and cross-chapter edges get a route label tag on the
            // target node. Below zoom 0.8 the nodes are too small to hold one.
            if zoom >= 0.8 && (branch || cross) && !tagged[edge.to] {
                let text = if edge.label.is_empty() { target.chapter.short } else { edge.label.as_str() };
                let (label_w, label_h) = label_dims(text, 11);
                if label_w > 0 && label_w + 8 <= tw && label_h + 2 <= th {
                    tagged[edge.to] = true;
                    let tint = if branch { route_ink(target.chapter) } else { [168, 206, 230] };
                    tags.push(EdgeTag {
                        text: text.to_string(),
                        x: tx,
                        y: ty,
                        w: tw,
                        h: th,
                        ending: target.ending,
                        tint,
                    });
                }
            }
        }
    }
    tags
}

fn draw_edge_tags(s: &mut Surface, tags: &[EdgeTag]) {
    for tag in tags {
        let anchor = if tag.ending { Anchor::TopLeft } else { Anchor::BottomLeft };
        draw_tag_in_rect(s, &tag.text, 11, (tag.x, tag.y, tag.w, tag.h), anchor, tag.tint);
    }
}

// ------------------------------------------------------------------ nodes ---

fn draw_nodes(s: &mut Surface, st: &PaintState) {
    let chart = st.chart;
    let view = st.view;
    let zoom = view.zoom;
    for (i, node) in chart.nodes.iter().enumerate() {
        let (x, y, w, h) = nav::place(node, view);
        if x + w < 0 || x > W || y + h < CHART_TOP || y > PREVIEW_TOP {
            continue;
        }
        if let Some(img) = st.thumbs.imgs.get(&node.id) {
            blit_scaled(s, img, x, y, w, h);
        } else {
            s.fill_rect(x, y, w.max(1) as u32, h.max(1) as u32, [10, 30, 60, 255]);
        }
        if i == st.sel {
            frame_rect(s, x, y, w, h, 3, GOLD);
        } else if st.hover == Some(i) {
            frame_rect(s, x, y, w, h, 2, CYAN);
        } else {
            frame_rect(s, x, y, w, h, 1, route_frame(node.chapter));
        }
        // Current save: a cyan corner wedge plus a small underline.
        if chart.here == Some(i) {
            let size = ((zoom * 7.0) as i32).clamp(5, 10);
            corner_triangle(s, x, y, size, CYAN);
            if y + h + 5 < PREVIEW_TOP {
                s.fill_rect(x + w / 2 - 5, y + h + 2, 10, 3, CYAN);
            }
        }
        // Branch point: a diamond in the top right corner of the card.
        if node.next.len() > 1 {
            let r = ((zoom * 4.0) as i32).clamp(3, 6);
            diamond(s, x + w - r - 3, y + r + 3, r, route_bright(node.chapter));
        }
        if node.ending {
            draw_tag_in_rect(s, "END", 11, (x, y, w, h), Anchor::BottomRight, rgb(GOLD));
        }
        if node.root {
            draw_tag_in_rect(s, "入口", 12, (x, y, w, h), Anchor::BottomLeft, [130, 226, 240]);
        }
    }
}

fn corner_triangle(s: &mut Surface, x: i32, y: i32, size: i32, color: [u8; 4]) {
    for row in 0..size {
        let w = size - row;
        chart_px(s, x, y + row, w, 1, color);
    }
}

fn diamond(s: &mut Surface, cx: i32, cy: i32, r: i32, color: [u8; 4]) {
    for dy in (-r - 1)..=(r + 1) {
        let half = r + 1 - dy.abs();
        chart_px(s, cx - half, cy + dy, 2 * half + 1, 1, [0, 8, 20, 255]);
    }
    for dy in -r..=r {
        let half = r - dy.abs();
        chart_px(s, cx - half, cy + dy, 2 * half + 1, 1, color);
    }
}

// -------------------------------------------------------------- top chrome ---

fn draw_top(s: &mut Surface, st: &PaintState) {
    let view = st.view;
    let zoom = view.zoom;
    s.fill_rect(0, 0, W as u32, HEADER_TOP as u32, BG);
    if let Some(banner) = st.banner {
        s.blit(banner, 0, 0);
    }
    s.fill_rect(0, HEADER_TOP, W as u32, HEADER_H as u32, HEADER_BG);
    s.fill_rect(0, HEADER_BOTTOM - 1, W as u32, 1, HEADER_LINE);

    let shorts = short_headers(st.chart);
    let mut order: Vec<(i32, usize)> = Vec::new();
    for (idx, band) in st.chart.bands.iter().enumerate() {
        let x0 = (nav::world_x(band.first_col) * zoom - view.scroll_x).round() as i32;
        let x1 = (nav::world_x(band.last_col + 1) * zoom - view.scroll_x).round() as i32;
        if x1 > 0 && x0 < W {
            order.push((x0, idx));
        }
    }
    order.sort_unstable();

    let mut right = -10_000;
    for (band_x, idx) in order {
        let band = &st.chart.bands[idx];
        let Some(title) = st.headers.get(idx).and_then(|header| header.as_ref()) else { continue };
        let x1 = (nav::world_x(band.last_col + 1) * zoom - view.scroll_x).round() as i32;
        // Follow the band, but never overlap the previous chapter's chip.
        let mut x = (band_x + 6).max(2);
        if x < right + 8 {
            x = right + 8;
        }
        if x >= x1.min(W - 2) {
            continue;
        }
        let avail = x1.min(W - 2) - x - 4;
        let short = shorts.1.get(idx).and_then(|header| header.as_ref());
        let chosen: Option<&Label> = if avail >= title.w as i32 + 14 {
            Some(title)
        } else {
            match short {
                Some(short) if avail >= short.w as i32 + 14 => Some(short),
                _ => None,
            }
        };
        let color = route_bright(band.chapter);
        match chosen {
            Some(label) => {
                let chip_w = label.w as i32 + 12;
                if x + chip_w > W - 2 {
                    continue;
                }
                s.fill_rect(
                    x,
                    HEADER_TOP + 3,
                    chip_w as u32,
                    (HEADER_H - 6) as u32,
                    mix(HEADER_BG, band.chapter.color, 24),
                );
                s.fill_rect(x, HEADER_TOP + 3, 3, (HEADER_H - 6) as u32, color);
                blit_tinted_clip(s, label, x + 7, HEADER_TOP + 1, route_ink(band.chapter), x + chip_w - 2);
                right = x + chip_w;
            }
            None => {
                // Too narrow for any text: keep a colour tick at the boundary.
                s.fill_rect(x, HEADER_TOP + 3, 3, (HEADER_H - 6) as u32, color);
                right = x + 8;
            }
        }
    }
}

// ------------------------------------------------------------ preview bar ---

fn draw_preview_bar(s: &mut Surface, st: &PaintState) {
    let node = &st.chart.nodes[st.sel];
    s.fill_rect(0, PREVIEW_TOP, W as u32, PREVIEW_H as u32, PREVIEW_BG);
    s.fill_rect(0, PREVIEW_TOP, W as u32, 2, CYAN);

    // Main preview thumbnail, framed in gold.
    let (tx, ty) = (28, PREVIEW_TOP + 26);
    s.fill_rect(tx - 6, ty - 6, (PREVIEW_W + 12) as u32, (PREVIEW_H_THUMB + 12) as u32, [0, 8, 22, 255]);
    let preview_key = format!("preview:{}", node.id);
    if let Some(img) = st.thumbs.imgs.get(&preview_key) {
        s.blit(img, tx, ty);
    } else if let Some(img) = st.thumbs.imgs.get(&node.id) {
        blit_scaled(s, img, tx, ty, PREVIEW_W, PREVIEW_H_THUMB);
    } else {
        s.fill_rect(tx, ty, PREVIEW_W as u32, PREVIEW_H_THUMB as u32, [12, 34, 68, 255]);
    }
    frame_rect(s, tx - 2, ty - 2, PREVIEW_W + 4, PREVIEW_H_THUMB + 4, 2, GOLD);

    // Chapter badge, scene number, hint and the two-line quote.
    let ink = route_ink(node.chapter);
    let mut badge_right = TEXT_X;
    if let Some(badge) = &st.preview.badge {
        s.fill_rect(TEXT_X, PREVIEW_TOP + 14, 4, 20, route_bright(node.chapter));
        blit_tinted_clip(s, badge, TEXT_X + 10, PREVIEW_TOP + 12, ink, TEXT_RIGHT);
        badge_right = TEXT_X + 10 + badge.w as i32;
    }
    if let Some(scene) = &st.preview.scene {
        blit_tinted_clip(s, scene, badge_right + 18, PREVIEW_TOP + 12, [235, 240, 250], TEXT_RIGHT);
    }
    if let Some(hint) = &st.preview.hint {
        blit_tinted_clip(s, hint, TEXT_X, PREVIEW_TOP + 42, [148, 184, 214], TEXT_RIGHT);
    }
    let mut quote_y = PREVIEW_TOP + 70;
    for (i, line) in st.preview.quote.iter().enumerate() {
        if i >= 2 {
            break;
        }
        let tint = if i == 0 { [238, 244, 252] } else { [206, 222, 240] };
        blit_tinted_clip(s, line, TEXT_X, quote_y, tint, TEXT_RIGHT);
        quote_y += line.h as i32 + 4;
    }
    if debug_timeline() {
        draw_debug(s, st);
    }

    // Fixed minimap at the right end of the preview bar.
    s.fill_rect(MINI_X - 6, MINI_Y, 1, MINI_H as u32, [40, 70, 110, 255]);
    draw_minimap(s, st);

    // The pager is drawn last: nothing in the bar may cover it.
    if let Some(pager) = st.pager {
        s.blit(pager, W - pager.width as i32 - 16, H - pager.height as i32 - 16);
    }
}

/// WA2_DEBUG_TIMELINE=1: show where the selected node's thumbnail came from.
fn draw_debug(s: &mut Surface, st: &PaintState) {
    let node = &st.chart.nodes[st.sel];
    let text = match st.thumbs.info.get(&node.id) {
        Some(info) => {
            let file = info
                .path
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
                .unwrap_or_default();
            format!("thumb {:?} · {file} · node {}", info.kind, node.id)
        }
        None => format!("thumb - · node {}", node.id),
    };
    // Stop before the pager (starts at x = W-288-16 = 976).
    with_label(&text, 12, |label| {
        blit_tinted_clip(s, label, TEXT_X, PREVIEW_TOP + 170, [130, 170, 200], 968);
    });
}

// ---------------------------------------------------------------- minimap ---

/// World extents of the whole chart plus the fixed-pixel panel they map into.
struct MiniMap {
    x0: i32,
    y0: i32,
    x1: i32,
    y1: i32,
    wx0: f32,
    wy0: f32,
    wx1: f32,
    wy1: f32,
}

impl MiniMap {
    fn for_chart(chart: &Chart) -> MiniMap {
        let max_col = chart.nodes.iter().map(|node| node.col).max().unwrap_or(0);
        let max_lane = chart.nodes.iter().map(|node| node.lane).max().unwrap_or(0);
        MiniMap {
            x0: MINI_X + 5,
            y0: MINI_Y + 5,
            x1: MINI_X + MINI_W - 5,
            y1: MINI_Y + MINI_H - 5,
            wx0: nav::world_x(0),
            wy0: nav::world_y(0),
            wx1: nav::world_x(max_col + 1),
            wy1: nav::world_y(max_lane + 1),
        }
    }

    fn map_x(&self, wx: f32) -> i32 {
        let span = (self.wx1 - self.wx0).max(1.0);
        let t = ((wx - self.wx0) / span).clamp(0.0, 1.0);
        self.x0 + (t * (self.x1 - self.x0) as f32).round() as i32
    }

    fn map_y(&self, wy: f32) -> i32 {
        let span = (self.wy1 - self.wy0).max(1.0);
        let t = ((wy - self.wy0) / span).clamp(0.0, 1.0);
        self.y0 + (t * (self.y1 - self.y0) as f32).round() as i32
    }

    fn map(&self, wx: f32, wy: f32) -> (i32, i32) {
        (self.map_x(wx), self.map_y(wy))
    }
}

/// Fixed-size overview: the whole chart's chapter strips, the current viewport
/// rectangle, the selected node and the current save node. It does not zoom.
fn draw_minimap(s: &mut Surface, st: &PaintState) {
    let chart = st.chart;
    let view = st.view;
    let mini = MiniMap::for_chart(chart);
    s.fill_rect(MINI_X, MINI_Y, MINI_W as u32, MINI_H as u32, MINI_BG);
    frame_rect(s, MINI_X, MINI_Y, MINI_W, MINI_H, 1, MINI_FRAME);

    for band in &chart.bands {
        let ax = mini.map_x(nav::world_x(band.first_col));
        let bx = mini.map_x(nav::world_x(band.last_col + 1));
        let ay = mini.map_y(nav::world_y(band.min_lane));
        let by = mini.map_y(nav::world_y(band.max_lane + 1));
        let w = (bx - ax + 1).max(1);
        let h = (by - ay + 1).max(1);
        s.fill_rect(ax, ay, w as u32, h as u32, mix(MINI_BG, band.chapter.color, 52));
    }

    if view.zoom > 0.0 {
        let vx0 = view.scroll_x / view.zoom;
        let vx1 = (view.scroll_x + W as f32) / view.zoom;
        let vy0 = view.scroll_y / view.zoom;
        let vy1 = (view.scroll_y + (PREVIEW_TOP - CHART_TOP) as f32) / view.zoom;
        let ax = mini.map_x(vx0);
        let bx = mini.map_x(vx1);
        let ay = mini.map_y(vy0);
        let by = mini.map_y(vy1);
        if bx - ax > 1 && by - ay > 1 {
            frame_rect(s, ax, ay, bx - ax, by - ay, 1, CYAN);
        }
    }

    let dot = |s: &mut Surface, node: &Node, color: [u8; 4]| {
        let (cx, cy) = mini.map(
            nav::world_x(node.col) + NODE_W as f32 / 2.0,
            nav::world_y(node.lane) + NODE_H as f32 / 2.0,
        );
        s.fill_rect(cx - 2, cy - 2, 5, 5, [0, 10, 24, 255]);
        s.fill_rect(cx - 1, cy - 1, 3, 3, color);
    };
    if let Some(here) = chart.here {
        dot(s, &chart.nodes[here], CYAN);
    }
    if let Some(node) = chart.nodes.get(st.sel) {
        dot(s, node, GOLD);
    }
}

// -------------------------------------------------------------- thumbnails ---

fn blit_scaled(dst: &mut Surface, img: &TgaImage, dx: i32, dy: i32, dw: i32, dh: i32) {
    if dw <= 0 || dh <= 0 || img.width == 0 || img.height == 0 {
        return;
    }
    // Chart band only. The header and the preview are painted on top, and a
    // straight copy here is what keeps a drag at frame rate.
    let x0 = dx.max(0);
    let y0 = dy.max(CHART_TOP);
    let x1 = (dx + dw).min(W);
    let y1 = (dy + dh).min(PREVIEW_TOP);
    if x0 >= x1 || y0 >= y1 {
        return;
    }
    if dw == img.width as i32 && dh == img.height as i32 && dx >= 0 && dy >= CHART_TOP {
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

fn frame_rect(s: &mut Surface, x: i32, y: i32, w: i32, h: i32, t: i32, c: [u8; 4]) {
    s.fill_rect(x, y, w as u32, t as u32, c);
    s.fill_rect(x, y + h - t, w as u32, t as u32, c);
    s.fill_rect(x, y, t as u32, h as u32, c);
    s.fill_rect(x + w - t, y, t as u32, h as u32, c);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::tga::TgaImage;
    use std::collections::HashMap;

    fn node(id: &str, col: i32, lane: i32, next: Vec<Edge>) -> Node {
        Node {
            id: id.into(),
            chapter: chapters::other(),
            col,
            lane,
            next,
            root: false,
            ending: false,
            shot: Vec::new(),
        }
    }

    fn blank_preview() -> Preview {
        Preview { badge: None, scene: None, quote: Vec::new(), hint: None, line: String::new() }
    }

    fn empty_thumbs() -> ThumbSet {
        ThumbSet { imgs: HashMap::new(), info: HashMap::new() }
    }

    fn view() -> View {
        View { zoom: 1.0, target_zoom: 1.0, scroll_x: 0.0, scroll_y: 0.0 }
    }

    fn make_state<'a>(
        chart: &'a Chart,
        thumbs: &'a ThumbSet,
        view: &'a View,
        preview: &'a Preview,
        sel: usize,
    ) -> PaintState<'a> {
        PaintState {
            chart,
            thumbs,
            banner: None,
            pager: None,
            sel,
            hover: None,
            view,
            preview,
            headers: &[],
        }
    }

    #[test]
    fn a_zoomed_thumbnail_blends_neighbouring_pixels() {
        let img = TgaImage { width: 2, height: 1, rgba: vec![0, 0, 0, 255, 255, 255, 255, 255] };
        let mut surf = Surface::new(1280, 720, [0, 0, 0, 255]);
        blit_scaled(&mut surf, &img, 0, CHART_TOP, 4, 1);
        let px = |x: i32| surf.pixels[((CHART_TOP * 1280 + x) * 4) as usize];
        assert_eq!(px(0), 0);
        assert!(px(1) > 40 && px(1) < 220, "zoom should blend, got {}", px(1));
        assert_eq!(px(2), 255);
    }

    #[test]
    fn chart_thumbnail_is_copied_onto_the_node() {
        let mut nodes = Vec::new();
        for i in 0..160 {
            nodes.push(node(&format!("{i}"), i as i32 % 40, (i % 6) as i32, Vec::new()));
        }
        let chart = Chart { nodes, here: Some(0), bands: Vec::new() };
        let tile = TgaImage {
            width: NODE_W as u32,
            height: NODE_H as u32,
            rgba: vec![40, 80, 120, 255].repeat((NODE_W * NODE_H) as usize),
        };
        let mut imgs = HashMap::new();
        for n in &chart.nodes {
            imgs.insert(n.id.clone(), tile.clone());
        }
        let thumbs = ThumbSet { imgs, info: HashMap::new() };
        let view = view();
        let preview = blank_preview();
        let mut frame = Surface::new(W as u32, H as u32, BG);
        paint(&mut frame, &make_state(&chart, &thumbs, &view, &preview, 0));
        let i = ((CHART_TOP as usize + 12) * W as usize + 40) * 4;
        assert_eq!(&frame.pixels[i..i + 4], &[40, 80, 120, 255], "thumbnail did not land on the chart");
    }

    #[test]
    fn the_selected_node_is_framed_in_gold() {
        let mut nodes = vec![node("1001", 0, 0, Vec::new())];
        nodes[0].chapter = &chapters::CHAPTERS[0];
        let chart = Chart { nodes, here: None, bands: Vec::new() };
        let tile = TgaImage {
            width: NODE_W as u32,
            height: NODE_H as u32,
            rgba: vec![40, 80, 120, 255].repeat((NODE_W * NODE_H) as usize),
        };
        let mut imgs = HashMap::new();
        imgs.insert("1001".to_string(), tile);
        let thumbs = ThumbSet { imgs, info: HashMap::new() };
        let view = view();
        let preview = blank_preview();
        let mut frame = Surface::new(W as u32, H as u32, BG);
        paint(&mut frame, &make_state(&chart, &thumbs, &view, &preview, 0));
        let (x, y, _, _) = nav::place(&chart.nodes[0], &view);
        let at = |x: i32, y: i32| {
            let i = ((y * W + x) * 4) as usize;
            [frame.pixels[i], frame.pixels[i + 1], frame.pixels[i + 2], frame.pixels[i + 3]]
        };
        assert_eq!(at(x + 50, y + 1), GOLD, "top edge should be the 3px gold frame");
        assert_eq!(at(x + 50, y + 30), [40, 80, 120, 255], "the frame must not eat the middle of the card");
    }

    #[test]
    fn chapter_bands_stay_below_the_fixed_header_bar() {
        let mut nodes = vec![node("1001", 0, 0, Vec::new())];
        nodes[0].chapter = &chapters::CHAPTERS[0];
        let bands = chapters::bands(&nodes);
        let chart = Chart { nodes, here: None, bands };
        let thumbs = empty_thumbs();
        let view = view();
        let preview = blank_preview();
        let mut frame = Surface::new(W as u32, H as u32, BG);
        paint(&mut frame, &make_state(&chart, &thumbs, &view, &preview, 0));
        let at = |x: i32, y: i32| {
            let i = ((y * W + x) * 4) as usize;
            [frame.pixels[i], frame.pixels[i + 1], frame.pixels[i + 2], frame.pixels[i + 3]]
        };
        // Nothing but the fixed header strip may be drawn across the bar.
        assert_eq!(at(700, 50), HEADER_BG, "the header bar must stay a solid strip");
        assert_eq!(at(700, 40), BG, "the gap under the banner stays background");
        // Just under the bar the chapter band is already painted. The band
        // spans exactly the chapter's columns, so sample inside node column 0.
        assert_eq!(at(60, 67), band_edge(&chapters::CHAPTERS[0]), "band should open with its boundary rule");
        assert_eq!(at(60, 70), band_fill(&chapters::CHAPTERS[0]));
    }

    #[test]
    fn branch_edges_stop_short_of_the_target_node() {
        let mut nodes = vec![
            node("2000", 0, 2, vec![Edge { to: 1, kind: EdgeKind::Branch, label: "小春".into() }]),
            node("2301", 1, 1, Vec::new()),
        ];
        nodes[0].chapter = &chapters::CHAPTERS[1];
        nodes[1].chapter = &chapters::CHAPTERS[4];
        let bands = chapters::bands(&nodes);
        let chart = Chart { nodes, here: None, bands };
        let thumbs = empty_thumbs();
        let view = view();
        let preview = blank_preview();
        let mut frame = Surface::new(W as u32, H as u32, BG);
        paint(&mut frame, &make_state(&chart, &thumbs, &view, &preview, 0));
        let (tx, ty, _, th) = nav::place(&chart.nodes[1], &view);
        let ey = ty + th / 2;
        // The arrowhead itself.
        let ax = (tx - EDGE_GAP) as usize;
        let arrow = frame.pixels[(ey as usize * W as usize + ax) * 4..][..4].to_vec();
        assert_eq!(arrow, route_bright(&chapters::CHAPTERS[4]), "no arrow at the edge tip");
        // ... and the 6px between the tip and the node are clear.
        let gap = frame.pixels[((ey as usize) * W as usize + (tx - 2) as usize) * 4..][..4].to_vec();
        assert_ne!(gap, arrow, "the edge must not be painted into the target node");
    }

    #[test]
    fn minimap_maps_the_chart_into_its_panel() {
        let mut nodes = vec![node("a", 0, 0, Vec::new()), node("b", 10, 2, Vec::new())];
        nodes[1].chapter = &chapters::CHAPTERS[0];
        let chart = Chart { nodes, here: None, bands: Vec::new() };
        let mini = MiniMap::for_chart(&chart);
        let (ax, ay) = mini.map(nav::world_x(0), nav::world_y(0));
        let (bx, by) = mini.map(nav::world_x(11), nav::world_y(3));
        assert_eq!((ax, ay), (MINI_X + 5, MINI_Y + 5), "world origin should map to the panel corner");
        assert_eq!(
            (bx, by),
            (MINI_X + MINI_W - 5, MINI_Y + MINI_H - 5),
            "the world end should map to the opposite corner"
        );
        // Out-of-world points clamp to the panel instead of leaking out of it.
        assert_eq!(mini.map(nav::world_x(-10), nav::world_y(-4)), (MINI_X + 5, MINI_Y + 5));
    }
}
