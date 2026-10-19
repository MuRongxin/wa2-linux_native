//! View transforms, dragging, zooming and keyboard movement over the chart.
//!
//! Movement is graph-first: left/right follow the scenario edges, so a branch
//! line moves to its actual successor rather than to whatever thumbnail
//! happens to sit in the next column.

use super::*;

/// Movement smaller than this stays a click. Past it, the chart follows the cursor.
pub(super) const DRAG_SLOP: i32 = 5;
pub(crate) const ZOOM_MIN: f32 = 0.4;
pub(crate) const ZOOM_MAX: f32 = 2.4;

pub(super) struct Hold {
    pub(super) ax: i32,
    pub(super) ay: i32,
    pub(super) panning: bool,
    /// Press began on the chart, not the preview bar.
    pub(super) chart: bool,
}

pub(super) fn grab(sx: i32, sy: i32) -> Hold {
    Hold { ax: sx, ay: sy, panning: false, chart: sy >= 40 && sy < PREVIEW_TOP }
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

pub(super) fn apply_drag(chart: &Chart, view: &mut View, hold: &mut Option<Hold>, sx: i32, sy: i32) {
    let Some(h) = hold.as_mut() else { return };
    let Some((dx, dy)) = begin_pan(h, sx, sy) else { return };
    view.scroll_x -= dx as f32;
    view.scroll_y -= dy as f32;
    clamp_view(chart, view);
}

pub(super) fn place(node: &Node, view: &View) -> (i32, i32, i32, i32) {
    let x = (world_x(node.col) * view.zoom - view.scroll_x).round() as i32;
    let y = (CHART_TOP as f32 + world_y(node.lane) * view.zoom - view.scroll_y).round() as i32;
    let w = (NODE_W as f32 * view.zoom).round().max(1.0) as i32;
    let h = (NODE_H as f32 * view.zoom).round().max(1.0) as i32;
    (x, y, w, h)
}

pub(super) fn world_x(col: i32) -> f32 {
    28.0 + col as f32 * PITCH_X as f32
}

pub(super) fn world_y(lane: i32) -> f32 {
    lane as f32 * PITCH_Y as f32
}

pub(super) fn hit_node(chart: &Chart, view: &View, x: i32, y: i32) -> Option<usize> {
    if y >= PREVIEW_TOP || y < CHART_TOP {
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

/// Zoom toward the cursor. The world point under (mx, my) stays there.
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

/// Glide zoom toward target_zoom. dt is seconds since the previous frame, so a
/// slow frame takes a longer step instead of stuttering.
pub(super) fn ease_zoom(chart: &Chart, view: &mut View, mx: i32, my: i32, dt: f32) {
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

/// Apply a wheel notch to the zoom target.
pub(super) fn wheel(view: &mut View, y: i32) {
    if y != 0 {
        let factor = 1.12_f32.powi(y.clamp(-8, 8));
        view.target_zoom = (view.target_zoom * factor).clamp(ZOOM_MIN, ZOOM_MAX);
    }
}

fn retarget(scroll_x: f32, scroll_y: f32, zoom: f32, mx: i32, my: i32, new_zoom: f32) -> (f32, f32) {
    let wx = (mx as f32 + scroll_x) / zoom;
    let wy = (my as f32 - CHART_TOP as f32 + scroll_y) / zoom;
    (
        wx * new_zoom - mx as f32,
        wy * new_zoom - (my as f32 - CHART_TOP as f32),
    )
}

pub(super) fn hit_preview(x: i32, y: i32) -> bool {
    x >= 28 && x < 28 + PREVIEW_W && y >= PREVIEW_TOP + 26 && y < PREVIEW_TOP + 26 + PREVIEW_H_THUMB
}

/// Left third of the pager steps backward, right third forward.
pub(super) fn hit_pager(pager: Option<&crate::codec::tga::TgaImage>, x: i32, y: i32) -> Option<i32> {
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

/// Left/right: the graph edge first, then the nearest scene in the same lane.
pub(super) fn step(chart: &Chart, sel: usize, dir: i32) -> usize {
    let node = &chart.nodes[sel];
    let mut best_target: Option<usize> = None;
    for edge in &node.next {
        let target = &chart.nodes[edge.to];
        if (target.col > node.col && dir > 0) || (target.col < node.col && dir < 0) {
            let score = (target.col - node.col).abs();
            let better = match best_target {
                None => true,
                Some(best) => {
                    let best_score = (chart.nodes[best].col - node.col).abs();
                    // Branch fan-outs put their targets in equal columns. Ties
                    // go to the lower scenario id, so Right from 2019 walks to
                    // 2020 (the main continuation) instead of a random sibling.
                    score < best_score
                        || (score == best_score && target.id.as_str() < chart.nodes[best].id.as_str())
                }
            };
            if better {
                best_target = Some(edge.to);
            }
        }
    }
    if let Some(to) = best_target {
        return to;
    }
    let col = node.col;
    let lane = node.lane;
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

/// Up/down: nearest node in another lane *in the same x region*.
///
/// Candidates are ranked by (distance from the current column to the
/// candidate chapter's band, |lane delta|, |column delta|). The band term is
/// what keeps a vertical step inside the chapter that spans the current
/// column: ranking by (|lane|, |column|) alone could jump from a CODA scene
/// to an early route thumbnail merely because it happened to sit in the next
/// lane and nearby.
pub(super) fn step_lane(chart: &Chart, sel: usize, dir: i32) -> usize {
    let col = chart.nodes[sel].col;
    let lane = chart.nodes[sel].lane;
    let mut best: Option<((i32, i32, i32), usize)> = None;
    for (i, n) in chart.nodes.iter().enumerate() {
        let dl = n.lane - lane;
        if dir < 0 && dl >= 0 || dir > 0 && dl <= 0 {
            continue;
        }
        let band = band_gap(chart, n.chapter, col).unwrap_or((n.col - col).abs());
        let score = (band, dl.abs(), (n.col - col).abs());
        if best.map(|(current, _)| score < current).unwrap_or(true) {
            best = Some((score, i));
        }
    }
    best.map(|(_, i)| i).unwrap_or(sel)
}

/// How far the column is outside the chapter's band; 0 when the band covers
/// it. None when the chapter draws no band (a fallback node).
fn band_gap(chart: &Chart, chapter: &Chapter, col: i32) -> Option<i32> {
    let band = chart.bands.iter().find(|b| std::ptr::eq(b.chapter, chapter))?;
    Some(if col < band.first_col {
        band.first_col - col
    } else if col > band.last_col {
        col - band.last_col
    } else {
        0
    })
}

/// Tab: jump to the next chapter band (wrapping).
pub(super) fn step_chapter(chart: &Chart, sel: usize, dir: i32) -> usize {
    if chart.bands.is_empty() {
        return sel;
    }
    let current = chart.nodes[sel].chapter;
    let n = chart.bands.len() as i32;
    let next = match chart.bands.iter().position(|b| std::ptr::eq(b.chapter, current)) {
        Some(index) => ((index as i32 + dir) % n + n) % n,
        // Outside the chapter table (a fallback node): step into the nearest end.
        None if dir > 0 => 0,
        None => n - 1,
    };
    let band = &chart.bands[next as usize];
    chart
        .nodes
        .iter()
        .position(|node| std::ptr::eq(node.chapter, band.chapter))
        .unwrap_or(sel)
}

/// Home: the earliest root (the beginning of the story).
pub(super) fn home(chart: &Chart) -> usize {
    chart
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.root)
        .min_by_key(|(_, n)| n.col)
        .map(|(i, _)| i)
        .unwrap_or(0)
}

/// End: the furthest scene in the selected node's lane.
pub(super) fn end(chart: &Chart, sel: usize) -> usize {
    let lane = chart.nodes[sel].lane;
    chart
        .nodes
        .iter()
        .enumerate()
        .filter(|(_, n)| n.lane == lane)
        .max_by_key(|(_, n)| n.col)
        .map(|(i, _)| i)
        .unwrap_or(sel)
}

/// R: back to 1:1 zoom at the origin.
pub(super) fn reset_view(chart: &Chart, view: &mut View) {
    view.zoom = 1.0;
    view.target_zoom = 1.0;
    view.scroll_x = 0.0;
    view.scroll_y = 0.0;
    clamp_view(chart, view);
}

pub(super) fn focus_view(chart: &Chart, sel: usize, view: &mut View) {
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

pub(super) fn clamp_view(chart: &Chart, view: &mut View) {
    let max_col = chart.nodes.iter().map(|n| n.col).max().unwrap_or(0);
    let max_lane = chart.nodes.iter().map(|n| n.lane).max().unwrap_or(0);
    let width = (world_x(max_col) + NODE_W as f32 + 40.0) * view.zoom;
    let height = (world_y(max_lane) + NODE_H as f32 + 16.0) * view.zoom;
    let max_x = (width - W as f32).max(0.0);
    let max_y = (height - (PREVIEW_TOP - CHART_TOP) as f32).max(0.0);
    view.scroll_x = view.scroll_x.clamp(0.0, max_x);
    view.scroll_y = view.scroll_y.clamp(0.0, max_y);
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    fn chart_of(nodes: Vec<Node>) -> Chart {
        Chart { nodes, here: None, bands: Vec::new() }
    }

    fn node(id: &str, col: i32, lane: i32, next: Vec<usize>) -> Node {
        Node {
            id: id.into(),
            chapter: chapters::other(),
            col,
            lane,
            next: next
                .into_iter()
                .map(|to| Edge { to, kind: EdgeKind::Sequential, label: String::new() })
                .collect(),
            root: false,
            ending: false,
            shot: Vec::new(),
        }
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
        let chart = chart_of(vec![node("1", 20, 5, vec![])]);
        let mut view = View { zoom: 1.0, target_zoom: 1.5, scroll_x: 200.0, scroll_y: 0.0 };
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
    fn left_right_follow_the_graph_edge() {
        // 0 -> 1 is a long jump; 2 sits in a nearer column but is not linked.
        let chart = chart_of(vec![
            node("a", 0, 0, vec![1]),
            node("b", 9, 0, vec![]),
            node("c", 2, 0, vec![]),
        ]);
        assert_eq!(step(&chart, 0, 1), 1, "the jump target wins over the nearer column");
        // No back edge from b: the nearest scene to the left in the lane is
        // the fallback, not a graph neighbour.
        assert_eq!(step(&chart, 1, -1), 2);
        assert_eq!(home(&chart), 0);
        assert_eq!(end(&chart, 0), 1);
    }

    #[test]
    fn tab_visits_one_chapter_at_a_time() {
        let mut nodes = vec![node("a", 0, 0, vec![]), node("b", 1, 1, vec![]), node("c", 2, 2, vec![])];
        nodes[1].chapter = &chapters::CHAPTERS[1];
        nodes[2].chapter = &chapters::CHAPTERS[2];
        let bands = super::super::chapters::bands(&nodes);
        let chart = Chart { nodes, here: None, bands };
        // Bands only exist for chapters that have nodes: band 0 is the
        // first table chapter present (CHAPTERS[1]), band 1 the next one.
        let b0 = step_chapter(&chart, 0, 1);
        assert_eq!(chart.nodes[b0].chapter.key, chapters::CHAPTERS[1].key);
        let b1 = step_chapter(&chart, b0, 1);
        assert_eq!(chart.nodes[b1].chapter.key, chapters::CHAPTERS[2].key);
        let back = step_chapter(&chart, b1, -1);
        assert_eq!(chart.nodes[back].chapter.key, chapters::CHAPTERS[1].key);
    }

    #[test]
    fn reset_view_returns_to_the_origin() {
        let chart = chart_of(vec![node("a", 30, 5, vec![])]);
        let mut view = View { zoom: 1.9, target_zoom: 2.2, scroll_x: 900.0, scroll_y: 200.0 };
        reset_view(&chart, &mut view);
        assert_eq!(view.zoom, 1.0);
        assert_eq!(view.target_zoom, 1.0);
        assert_eq!(view.scroll_x, 0.0);
        assert!(view.scroll_y <= 0.0);
    }

    #[test]
    fn place_and_hit_test_agree() {
        let chart = chart_of(vec![node("a", 2, 3, vec![])]);
        let view = View { zoom: 1.0, target_zoom: 1.0, scroll_x: 0.0, scroll_y: 0.0 };
        let (x, y, w, h) = place(&chart.nodes[0], &view);
        assert_eq!(hit_node(&chart, &view, x + w / 2, y + h / 2), Some(0));
        assert_eq!(hit_node(&chart, &view, x + w / 2, y - 20), None);
        let _ = (HashMap::<u8, u8>::new(), DRAG_SLOP, ZOOM_MIN, ZOOM_MAX);
    }

    // ---- synthetic charts for the graph-first movement rules ----

    fn chapter(key: &str) -> &'static Chapter {
        chapters::CHAPTERS
            .iter()
            .find(|c| c.key == key)
            .unwrap_or_else(|| panic!("chapter {key} is not in the table"))
    }

    fn chapter_node(id: &str, col: i32, lane: i32, chapter: &'static Chapter) -> Node {
        Node {
            id: id.into(),
            chapter,
            col,
            lane,
            next: Vec::new(),
            root: false,
            ending: false,
            shot: Vec::new(),
        }
    }

    fn band(chapter: &'static Chapter, first_col: i32, last_col: i32) -> Band {
        Band { chapter, first_col, last_col, min_lane: 0, max_lane: 5 }
    }

    fn chart_of_nodes(nodes: Vec<Node>, bands: Vec<Band>) -> Chart {
        Chart { nodes, here: None, bands }
    }

    #[test]
    fn right_follows_the_nearest_branch_edge() {
        // Three exits, two of them to the right: the near one wins.
        let mut a = node("a", 0, 0, vec![1, 2, 3]);
        a.next = vec![
            Edge { to: 1, kind: EdgeKind::Branch, label: "far".into() },
            Edge { to: 2, kind: EdgeKind::Branch, label: "near".into() },
            Edge { to: 3, kind: EdgeKind::Branch, label: "back".into() },
        ];
        let chart = chart_of(vec![
            a,
            node("far", 8, 0, vec![]),
            node("near", 2, 0, vec![]),
            node("back", -3, 0, vec![]),
        ]);
        assert_eq!(chart.nodes[step(&chart, 0, 1)].id, "near");
        // Only the backward edge points left, so that is the step.
        assert_eq!(chart.nodes[step(&chart, 0, -1)].id, "back");
        // At the rightmost node of the lane there is nowhere further right.
        assert_eq!(step(&chart, 1, 1), 1);
    }

    #[test]
    fn up_prefers_the_band_that_spans_the_current_column() {
        // Real chart shape: a CODA·雪菜 scene sits above the main CODA row.
        // Pressing up must stay in the CODA band that covers the current
        // column, not jump to an early CC 共通 scene that happens to have the
        // nearest thumbnail. The band term is the first tie-break.
        let coda_setsuna = chapter("coda_setsuna");
        let coda = chapter("coda");
        let common = chapter("cc_common");
        let nodes = vec![
            chapter_node("3201", 40, coda_setsuna.lane, coda_setsuna),
            chapter_node("2019", 39, common.lane, common),
            chapter_node("3001", 38, coda.lane, coda),
            chapter_node("3024", 52, coda.lane, coda),
        ];
        let bands = vec![band(common, 39, 39), band(coda, 38, 52)];
        let chart = chart_of_nodes(nodes, bands);
        let up = step_lane(&chart, 0, -1);
        assert_eq!(chart.nodes[up].chapter.key, "coda", "landed on {}", chart.nodes[up].id);
        assert_ne!(chart.nodes[up].chapter.key, "cc_common");
        // Inside the winning band the nearest scene of that lane is chosen.
        assert_eq!(chart.nodes[up].id, "3001");
    }

    #[test]
    fn a_vertical_step_prefers_the_nearest_lane() {
        // Both candidates' bands cover the current column, so the lane gap and
        // then the column distance decide.
        let nodes = vec![
            chapter_node("cur", 10, chapter("coda").lane, chapter("coda")),
            chapter_node("lane1", 12, chapter("cc_koharu").lane, chapter("cc_koharu")),
            chapter_node("lane0", 10, chapter("cc_chiaki").lane, chapter("cc_chiaki")),
        ];
        let bands = vec![band(chapter("cc_koharu"), 5, 15), band(chapter("cc_chiaki"), 5, 15)];
        let chart = chart_of_nodes(nodes, bands);
        assert_eq!(chart.nodes[step_lane(&chart, 0, -1)].id, "lane1", "one lane away beats two");

        // Down goes to the nearer lane on the other side.
        let nodes = vec![
            chapter_node("cur", 10, chapter("coda").lane, chapter("coda")),
            chapter_node("lane3", 9, chapter("coda_setsuna").lane, chapter("coda_setsuna")),
            chapter_node("lane4", 10, chapter("coda_kazusa").lane, chapter("coda_kazusa")),
        ];
        let bands = vec![band(chapter("coda_setsuna"), 0, 25), band(chapter("coda_kazusa"), 0, 25)];
        let chart = chart_of_nodes(nodes, bands);
        assert_eq!(chart.nodes[step_lane(&chart, 0, 1)].id, "lane3");
    }

    #[test]
    fn a_vertical_step_prefers_the_nearest_column() {
        // Same lane and the same band for both candidates, so the column
        // distance is the last tie-break.
        let koharu = chapter("cc_koharu");
        let nodes = vec![
            chapter_node("cur", 10, chapter("coda").lane, chapter("coda")),
            chapter_node("near", 7, koharu.lane, koharu),
            chapter_node("far", 20, koharu.lane, koharu),
        ];
        let chart = chart_of_nodes(nodes, vec![band(koharu, 0, 25)]);
        assert_eq!(chart.nodes[step_lane(&chart, 0, -1)].id, "near");
    }

    #[test]
    fn a_vertical_step_stays_put_at_the_edge() {
        let chart = chart_of_nodes(
            vec![chapter_node("only", 3, 0, chapter("coda"))],
            Vec::new(),
        );
        assert_eq!(step_lane(&chart, 0, -1), 0);
        assert_eq!(step_lane(&chart, 0, 1), 0);
    }

    #[test]
    fn home_end_and_reset_follow_the_story() {
        let mut a = chapter_node("a", 4, 1, chapter("ic"));
        a.root = true;
        let mut b = chapter_node("b", 10, 1, chapter("cc_common"));
        b.ending = true;
        let mut c = chapter_node("c", 7, 0, chapter("ic"));
        c.root = true;
        let chart = chart_of_nodes(vec![a, b, c], Vec::new());
        assert_eq!(chart.nodes[home(&chart)].id, "a", "Home is the earliest root");
        assert_eq!(chart.nodes[end(&chart, 1)].id, "b");
        let mut view = View { zoom: 2.0, target_zoom: 2.0, scroll_x: 500.0, scroll_y: 200.0 };
        reset_view(&chart, &mut view);
        assert_eq!((view.zoom, view.target_zoom, view.scroll_x), (1.0, 1.0, 0.0));
    }

    #[test]
    fn focus_view_brings_the_selection_inside_the_safe_area() {
        let chart = chart_of_nodes(
            vec![chapter_node("far", 40, chapter("coda").lane, chapter("coda"))],
            Vec::new(),
        );
        let mut view = View { zoom: 1.0, target_zoom: 1.0, scroll_x: 0.0, scroll_y: 0.0 };
        let (x, _, _, _) = place(&chart.nodes[0], &view);
        assert!(x > W, "the node starts off screen at {x}");
        focus_view(&chart, 0, &mut view);
        let (x, y, w, h) = place(&chart.nodes[0], &view);
        assert!(x >= 0 && x + w <= W, "x {x}..{x}+{w} outside {W}");
        assert!(y >= CHART_TOP && y + h <= PREVIEW_TOP, "y {y}..{y}+{h} outside the chart");
    }
}
