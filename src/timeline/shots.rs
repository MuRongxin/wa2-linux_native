//! Thumbnail selection: one representative picture per scene, plus the
//! pre-rendered cache in data/cache/timeline.
//!
//! The chart only has 104x59 pixels per node and a 208x117 preview, so it must
//! never decode a full 8x CG: tools/build_timeline_cache.py pre-renders 320x180
//! TGA thumbnails and the runtime reads those. Everything else here picks
//! which picture represents the scene.

use super::*;
use crate::codec::tga::TgaImage;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};

/// Load every node's chart thumbnail and preview image.
///
/// Keys: the scene id for the 3x node image, "preview:{id}" for the preview
/// bar. Missing pictures simply do not appear in the map.
pub(super) fn load_thumbs(dirs: &[&Path], chart: &Chart, cache_dir: &Path) -> ThumbSet {
    let mut imgs = HashMap::new();
    let mut info = HashMap::new();
    for (id, path) in scene_files_with(dirs, &chart.nodes, true) {
        // tools/build_timeline_cache.py pre-renders small TGA thumbnails from
        // the 8x WebP tree: decoding a 59 MP lossless background just to draw a
        // 104x59 node would otherwise make opening the chart take minutes.
        let stem = path
            .file_stem()
            .map(|s| s.to_string_lossy().to_ascii_lowercase())
            .unwrap_or_default();
        let cached = cache_dir.join(format!("{stem}.tga"));
        let img = if let Ok(img) = crate::codec::image::decode_file(&cached) {
            img
        } else if let Ok(img) = crate::codec::image::decode_ui_file(&path) {
            img
        } else {
            continue;
        };
        info.insert(id.clone(), ThumbInfo { kind: ShotKind::Entry, path: path.clone() });
        imgs.insert(format!("preview:{id}"), fit(&img, PREVIEW_W as u32, PREVIEW_H_THUMB as u32));
        // Three times the on-screen node, so a zoom still has pixels to sample
        // instead of stretching a nearest-neighbor crumb.
        imgs.insert(id, fit(&img, (NODE_W * 3) as u32, (NODE_H * 3) as u32));
    }
    ThumbSet { imgs, info }
}

struct Pic {
    variant: i32,
    /// A 256x144 tv* stand-in of this same picture.
    thumb: bool,
    /// A b* background. A v*/tv* file fills a background id only when no b*
    /// was shipped for that number - otherwise it is an event CG that happens
    /// to share the six digits (b101700 is a room, tv101700 is not).
    background: bool,
    path: PathBuf,
}

struct PictureIndex {
    /// Background id -> files named b/tv + {id:04}{variant:02}.
    bg: HashMap<i64, Vec<Pic>>,
    /// Event-CG id -> files named v/tv + {id * 10 + variant:06}.
    cg: HashMap<i64, Vec<Pic>>,
}

/// One picture per scene. The file shared by the fewest scenes wins, so a
/// script that begins by repeating the previous location does not stamp that
/// location onto every following node. An event CG breaks a tie.
fn scene_files(dirs: &[&Path], nodes: &[Node]) -> HashMap<String, PathBuf> {
    scene_files_with(dirs, nodes, false)
}

fn scene_files_with(dirs: &[&Path], nodes: &[Node], prefer_thumb: bool) -> HashMap<String, PathBuf> {
    let index = picture_index(dirs);
    let resolve_shot = |shot: &Shot| {
        if prefer_thumb {
            resolve_thumb(&index, shot)
        } else {
            resolve(&index, shot)
        }
    };
    let resolved: Vec<Vec<Option<PathBuf>>> = nodes
        .iter()
        .map(|node| node.shot.iter().map(resolve_shot).collect())
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
        // v and tv are the event CG (full frame and its small copy).
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

fn resolve(index: &PictureIndex, shot: &Shot) -> Option<PathBuf> {
    let pics = if shot.cg { index.cg.get(&shot.id)? } else { index.bg.get(&shot.id)? };
    pics.iter()
        .min_by_key(|pic| {
            (
                u8::from(pic.variant != shot.variant),
                // The shipped background wins over a CG that reuses the number.
                u8::from(!shot.cg && !pic.background),
                // Prefer the full event CG when it exists. The original engine
                // used tv* thumbnails to avoid loading large art, but this
                // renderer can downscale the full image while keeping only the
                // preview in memory. Keep tv* as a fallback for assets that
                // have no corresponding full file.
                u8::from(pic.thumb),
                u8::from(pic.variant != 0),
                pic.variant,
            )
        })
        .map(|pic| pic.path.clone())
}

/// Like resolve, but prefers the small tv* copy of a picture. Timeline nodes
/// are 104x59 and the preview is 208x117, so decoding the full 8x CG is wasted
/// I/O and memory.
fn resolve_thumb(index: &PictureIndex, shot: &Shot) -> Option<PathBuf> {
    let pics = if shot.cg { index.cg.get(&shot.id)? } else { index.bg.get(&shot.id)? };
    pics.iter()
        .min_by_key(|pic| {
            (
                u8::from(pic.variant != shot.variant),
                // The whole point of this resolver: take the small tv* copy
                // before considering whether the file is a shipped background.
                u8::from(!pic.thumb),
                u8::from(!shot.cg && !pic.background),
                u8::from(pic.variant != 0),
                pic.variant,
            )
        })
        .map(|pic| pic.path.clone())
}

fn pick_picture(
    shots: &[Shot],
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
        let cg_rank = u8::from(!shots[i].cg);
        let rank = (*owners.get(path).unwrap_or(&1), cg_rank, i);
        if best.as_ref().map(|b| rank < (b.0, b.1, b.2)).unwrap_or(true) {
            best = Some((rank.0, rank.1, rank.2, path.clone()));
        }
    }
    best.map(|(_, _, _, path)| path)
}

/// tv100100.tga is script 1001, variant 00.
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::tga::TgaImage;

    #[test]
    fn thumbnails_are_preferred_for_chart_nodes() {
        let mut bg = HashMap::new();
        bg.insert(
            1001,
            vec![
                Pic { variant: 0, thumb: false, background: true, path: PathBuf::from("b100100.tga") },
                Pic { variant: 0, thumb: true, background: false, path: PathBuf::from("tv100100.tga") },
            ],
        );
        let index = PictureIndex { bg, cg: HashMap::new() };
        let shot = Shot { cg: false, id: 1001, variant: 0, op_index: 0, source: "1001".into() };
        let thumb = resolve_thumb(&index, &shot).unwrap();
        assert!(thumb.ends_with("tv100100.tga"), "{thumb:?}");
        // The general resolver still prefers the full event CG.
        let full = resolve(&index, &shot).unwrap();
        assert!(full.ends_with("b100100.tga"), "{full:?}");
    }

    #[test]
    fn neighbouring_scenes_do_not_share_a_thumbnail() {
        let bg = Path::new("data/bg");
        if !bg.is_dir() {
            return;
        }
        let nodes = graph::build_nodes(Path::new("data/script"));
        let chosen = scene_files(&[bg], &nodes);
        let name = |id: &str| {
            chosen.get(id).and_then(|p| p.file_name()).map(|s| s.to_string_lossy().into_owned())
        };
        assert_ne!(name("1002"), name("1003"), "opening street reused");
        assert_ne!(name("1012"), name("1013"), "bedroom reused");
        assert_ne!(name("1003").as_deref(), Some("tv100100.tga"));
        let distinct = chosen.values().collect::<std::collections::HashSet<_>>().len();
        assert!(distinct > 100, "thumbnails collapsed onto {distinct} files for {} scenes", chosen.len());
        let index = picture_index(&[bg]);
        let file = |shot: Shot| {
            resolve(&index, &shot)
                .and_then(|p| p.file_name().map(|s| s.to_string_lossy().into_owned()))
                .unwrap_or_default()
        };
        let shot = |cg, id, variant| Shot { cg, id, variant, op_index: 0, source: String::new() };
        assert_eq!(file(shot(false, 1001, 0)), "v100100.tga");
        assert_eq!(file(shot(false, 1055, 0)), "b105503.tga");
        assert_eq!(file(shot(true, 10030, 0)), "v100300.tga");
        // b101700 is the room. tv101700 is the event CG that shares the number.
        assert_eq!(file(shot(false, 1017, 0)), "b101700.tga");
        assert_eq!(file(shot(true, 10170, 0)), "v101700.tga");
        assert_ne!(name("3211").as_deref(), Some("tv101700.tga"));
        assert_eq!(thumb_script_id("tv100100.tga").as_deref(), Some("1001"));
    }

    #[test]
    fn thumbnail_downscale_averages_neighbouring_pixels() {
        let img = TgaImage {
            width: 2,
            height: 2,
            rgba: vec![0, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255],
        };
        let out = fit(&img, 1, 1);
        assert_eq!(&out.rgba[..4], &[63, 63, 63, 255]);
    }
}
