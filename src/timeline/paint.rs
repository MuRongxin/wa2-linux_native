//! Drawing the timeline chart: bands, edges, nodes, the banner and the preview
//! bar. All coordinates are logical 1280x720 pixels; nav::place() maps a node's
//! world cell onto that surface.

use super::*;
use crate::codec::tga::TgaImage;
use crate::render::Surface;

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

pub(super) fn paint(s: &mut Surface, st: &PaintState) {
    let chart = st.chart;
    let view = st.view;
    s.fill_rect(0, 0, W as u32, H as u32, BG);

    // Chapter bands sit under everything else.
    for band in &chart.bands {
        band_tint(s, band, view);
    }
    // Lines next, so a thumbnail covers the joint.
    for node in &chart.nodes {
        let (x1, y1, w, h) = nav::place(node, view);
        let ax = x1 + w;
        let ay = y1 + h / 2;
        for edge in &node.next {
            let (x2, y2, _, h2) = nav::place(&chart.nodes[edge.to], view);
            let color = if edge.kind == EdgeKind::Branch {
                chapter_color(chart.nodes[edge.to].chapter)
            } else {
                LINE
            };
            elbow(s, ax, ay, x2, y2 + h2 / 2, color);
        }
    }
    for (i, node) in chart.nodes.iter().enumerate() {
        let (x, y, w, h) = nav::place(node, view);
        if x + w < 0 || x > W || y + h < CHART_TOP || y > PREVIEW_TOP {
            continue;
        }
        if let Some(img) = st.thumbs.imgs.get(&node.id) {
            blit_scaled(s, img, x, y, w, h);
        } else {
            s.fill_rect(x, y, w.max(1) as u32, h.max(1) as u32, [12, 36, 72, 255]);
        }
        let route = chapter_color(node.chapter);
        let color = if i == st.sel {
            GOLD
        } else if st.hover == Some(i) {
            CYAN
        } else if chart.here == Some(i) {
            [120, 170, 210, 255]
        } else {
            route
        };
        let thick = if i == st.sel { 3 } else { 1 };
        frame_rect(s, x, y, w, h, thick, color);
        if chart.here == Some(i) {
            s.fill_rect(x + w / 2 - 5, y + h + 4, 10, 6, CYAN);
        }
    }

    // Cover the chart where the header and the preview sit.
    s.fill_rect(0, 0, W as u32, CHART_TOP as u32, BG);
    if let Some(b) = st.banner {
        s.blit(b, 0, 0);
    }
    // Chapter headers: fixed screen size, following the band horizontally.
    for (band, header) in chart.bands.iter().zip(st.headers) {
        let Some(header) = header else { continue };
        let x = (nav::world_x(band.first_col) * view.zoom - view.scroll_x).round() as i32 + 2;
        if x > W - 4 || x + header.w as i32 + 140 < 0 {
            continue;
        }
        s.fill_rect(x.max(0), 42, (header.w + 10).min((W - x).max(0) as u32), 24, [3, 30, 70, 220]);
        blit_tinted(s, header, x + 5, 45, route_ink(band.chapter));
    }
    s.fill_rect(0, PREVIEW_TOP, W as u32, PREVIEW_H as u32, [1, 29, 89, 255]);
    s.fill_rect(0, PREVIEW_TOP, W as u32, 2, CYAN);

    let node = &chart.nodes[st.sel];
    let thumb_x = 28;
    let thumb_y = PREVIEW_TOP + 26;
    if let Some(big) = st.thumbs.imgs.get(&format!("preview:{}", node.id)) {
        s.blit(big, thumb_x, thumb_y);
    } else if let Some(img) = st.thumbs.imgs.get(&node.id) {
        s.blit(img, thumb_x, thumb_y);
    }
    frame_rect(s, thumb_x, thumb_y, PREVIEW_W, PREVIEW_H_THUMB, 2, GOLD);

    let text_x = thumb_x + PREVIEW_W + 20;
    let mut badge_right = text_x;
    if let Some(badge) = &st.preview.badge {
        blit_tinted(s, badge, text_x, PREVIEW_TOP + 14, route_ink(node.chapter));
        badge_right = text_x + badge.w as i32;
    }
    if let Some(scene) = &st.preview.scene {
        blit_tinted(s, scene, badge_right + 16, PREVIEW_TOP + 14, [235, 240, 250]);
    }
    if let Some(hint) = &st.preview.hint {
        blit_tinted(s, hint, text_x, PREVIEW_TOP + 42, [150, 186, 214]);
    }
    let mut ty = PREVIEW_TOP + 92;
    for line in &st.preview.quote {
        s.blit_rgba(&line.rgba, line.w, line.h, text_x, ty);
        ty += line.h as i32 + 2;
    }
    if let Some(p) = st.pager {
        s.blit(p, W - p.width as i32 - 16, H - p.height as i32 - 16);
    }
}

fn route_ink(chapter: &Chapter) -> [u8; 3] {
    [chapter.color[0], chapter.color[1], chapter.color[2]]
}

fn chapter_color(chapter: &Chapter) -> [u8; 4] {
    let [r, g, b, _] = chapter.color;
    [(r as u16 * 55 / 100) as u8, (g as u16 * 55 / 100) as u8, (b as u16 * 55 / 100) as u8, 255]
}

/// A very translucent vertical tint behind one chapter, plus its boundary
/// rules. It scrolls and scales with the chart so it hugs the nodes.
fn band_tint(s: &mut Surface, band: &Band, view: &View) {
    let x0 = (nav::world_x(band.first_col) * view.zoom - view.scroll_x).round() as i32 - 16;
    let x1 = (nav::world_x(band.last_col + 1) * view.zoom - view.scroll_x).round() as i32 + 10;
    let x0 = x0.max(0);
    let x1 = x1.min(W);
    if x1 <= x0 {
        return;
    }
    let [r, g, b, _] = band.chapter.color;
    let tint = [(r as u32 * 30 / 100) as u8, (g as u32 * 30 / 100) as u8, (b as u32 * 30 / 100) as u8, 255];
    let y0 = (CHART_TOP + 24).max(40);
    let h = (PREVIEW_TOP - y0).max(0) as u32;
    if h == 0 {
        return;
    }
    // fill_rect blends an opaque colour, so blend the tint at low alpha by
    // hand: draw every other row? No - use the surface alpha blend directly.
    let _ = tint;
    let colour = [
        (r as u32 * 26 / 100 + BG[0] as u32 * 74 / 100) as u8,
        (g as u32 * 26 / 100 + BG[1] as u32 * 74 / 100) as u8,
        (b as u32 * 26 / 100 + BG[2] as u32 * 74 / 100) as u8,
        255,
    ];
    s.fill_rect(x0, y0, (x1 - x0) as u32, h, colour);
    for x in [x0, x1 - 1] {
        let colour = [
            (r as u32 * 70 / 100 + BG[0] as u32 * 30 / 100) as u8,
            (g as u32 * 70 / 100 + BG[1] as u32 * 30 / 100) as u8,
            (b as u32 * 70 / 100 + BG[2] as u32 * 30 / 100) as u8,
            255,
        ];
        s.fill_rect(x, y0, 1, h, colour);
    }
}

/// Blit coverage-only label pixels with a flat tint.
pub(super) fn blit_tinted(s: &mut Surface, label: &Label, dx: i32, dy: i32, tint: [u8; 3]) {
    for row in 0..label.h as i32 {
        let y = dy + row;
        if y < 0 || y >= s.height as i32 {
            continue;
        }
        for col in 0..label.w as i32 {
            let x = dx + col;
            if x < 0 || x >= s.width as i32 {
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

fn elbow(s: &mut Surface, x1: i32, y1: i32, x2: i32, y2: i32, color: [u8; 4]) {
    let minx = x1.min(x2);
    let maxx = x1.max(x2);
    let miny = y1.min(y2);
    let maxy = y1.max(y2);
    if maxx < 0 || minx > W || maxy < CHART_TOP || miny >= PREVIEW_TOP {
        return;
    }
    let gap = 6;
    let mid = x1 + (x2 - x1) / 2;
    hline(s, x1, y1, mid - x1, color);
    vline(s, mid, y1.min(y2), (y1 - y2).abs(), color);
    hline(s, mid, y2, (x2 - gap) - mid, color);
}

fn hline(s: &mut Surface, x: i32, y: i32, w: i32, color: [u8; 4]) {
    if w < 0 {
        hline(s, x + w, y, -w, color);
        return;
    }
    let x0 = x.max(0);
    let x1 = (x + w).min(W);
    if y < CHART_TOP || y >= PREVIEW_TOP || x1 <= x0 {
        return;
    }
    s.fill_rect(x0, y, (x1 - x0) as u32, 2, color);
}

fn vline(s: &mut Surface, x: i32, y: i32, h: i32, color: [u8; 4]) {
    if h < 0 {
        vline(s, x, y + h, -h, color);
        return;
    }
    let y0 = y.max(CHART_TOP);
    let y1 = (y + h).min(PREVIEW_TOP);
    if x < 0 || x >= W || y1 <= y0 {
        return;
    }
    s.fill_rect(x, y0, 2, (y1 - y0) as u32, color);
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
            nodes.push(Node {
                id: format!("{i}"),
                chapter: chapters::other(),
                col: i as i32 % 40,
                lane: (i % 6) as i32,
                next: vec![],
                root: false,
                ending: false,
                shot: vec![],
            });
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
        let view = View { zoom: 1.0, target_zoom: 1.0, scroll_x: 0.0, scroll_y: 0.0 };
        let preview = Preview { badge: None, scene: None, quote: Vec::new(), hint: None, line: String::new() };
        let mut frame = Surface::new(W as u32, H as u32, BG);
        let state = PaintState {
            chart: &chart,
            thumbs: &thumbs,
            banner: None,
            pager: None,
            sel: 0,
            hover: None,
            view: &view,
            preview: &preview,
            headers: &[],
        };
        paint(&mut frame, &state);
        let i = ((CHART_TOP as usize + 12) * W as usize + 40) * 4;
        assert_eq!(&frame.pixels[i..i + 4], &[40, 80, 120, 255], "thumbnail did not land on the chart");
    }
}
