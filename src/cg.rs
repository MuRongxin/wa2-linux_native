//! CG mode: the list and its in-memory page thumbnails.
//!
//! The list is every event CG (`v%06d`) shipped in the picture trees. Layer
//! pieces (`_bg`, `_chr`, `_chreff`, ...) are parts of one picture, not pictures
//! of their own, so they are dropped.
//!
//! The directory order supplied by the session selects the source tree for
//! the current window: 4x in a normal window, 8x when fullscreen, or the
//! explicitly selected 2x proxy. WebP is resized inside the decoder to twice
//! the displayed cell size, while TGA is reduced with the renderer's area
//! filter. The original `tv{number}.tga` files are deliberately never used:
//! they are only 256x144 and become visibly soft on a fullscreen grid.

use crate::codec::image;
use crate::codec::tga::TgaImage;
use crate::render::Surface;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{mpsc, Arc};

/// Grid shape. Five 240px cells plus the gaps fill the 1280px layout exactly.
pub const COLS: usize = 5;
pub const ROWS: usize = 4;
pub const PER_PAGE: usize = COLS * ROWS;

/// Keep two source pixels for every displayed pixel. The actual dimensions
/// are supplied by the browser because the window can be resized while this
/// screen is open.
pub const PREVIEW_SCALE: u32 = 2;

/// Parts of a single composed picture, listed as separate files in the trees.
/// Every suffix the trees actually ship: these are the parts one composed CG
/// is assembled from (`v103200_fence` is its fence overlay, for instance).
const LAYER_SUFFIXES: [&str; 8] =
    ["_bg", "_chr", "_chreff", "_ev", "_fg", "_eff", "_fence", "_mask"];
const EXTS: [&str; 6] = ["tga", "TGA", "webp", "WEBP", "bmp", "BMP"];

/// Every event CG in the picture trees, sorted, keyed by its stem.
pub fn cg_stems(dirs: &[&Path]) -> Vec<String> {
    let mut found: HashMap<String, String> = HashMap::new();
    for dir in dirs {
        let rd = match std::fs::read_dir(dir) {
            Ok(r) => r,
            Err(_) => continue,
        };
        for ent in rd.flatten() {
            let name = ent.file_name().to_string_lossy().into_owned();
            let lower = name.to_ascii_lowercase();
            let Some(stem) = lower.rfind('.').map(|i| &lower[..i]) else { continue };
            if !stem.starts_with('v') || LAYER_SUFFIXES.iter().any(|s| stem.ends_with(s)) {
                continue;
            }
            // Keep the original casing: file names are mostly lower case, but
            // looking a picture up has to try what is actually on disk first.
            let original = name.rfind('.').map(|i| name[..i].to_string()).unwrap_or(name.clone());
            found.entry(stem.to_string()).or_insert(original);
        }
    }
    let mut stems: Vec<String> = found.into_values().collect();
    stems.sort();
    stems
}

/// Decode one CG directly from the selected art tree. The returned image is a
/// page-local preview and is never written to disk.
pub fn load_thumb(dirs: &[&Path], stem: &str, max_w: u32, max_h: u32) -> Option<TgaImage> {
    let source = picture_path(dirs, stem)?;
    let started = std::time::Instant::now();
    let img = make_thumb(&source, max_w, max_h)?;
    if debug_cg() {
        eprintln!(
            "cg: thumb {} memory={} {}x{} source={} ({}x) target={}x{} {:.1}ms",
            stem,
            "page",
            img.width,
            img.height,
            source.display(),
            source_art_scale(&source),
            max_w,
            max_h,
            started.elapsed().as_secs_f64() * 1000.0,
        );
    }
    Some(img)
}

/// A single worker for the currently visible CG page. The worker owns cloned
/// directory paths, so the UI loop stays responsive while WebP is decoded.
/// Results are kept by the caller in a page-local map and disappear when the
/// page changes or the browser closes.
pub struct PageLoader {
    rx: mpsc::Receiver<(String, Option<TgaImage>)>,
    cancel: Arc<AtomicBool>,
}

impl PageLoader {
    pub fn poll(&self) -> Vec<(String, Option<TgaImage>)> {
        self.rx.try_iter().collect()
    }
}

impl Drop for PageLoader {
    fn drop(&mut self) {
        self.cancel.store(true, Ordering::Relaxed);
    }
}

/// Start decoding one page in the background. There is only one worker per
/// page, intentionally: twenty concurrent dwebp processes would contend for
/// the same CPU and make the first page feel worse. A failed thread spawn is
/// reported to the caller so it can use the synchronous fallback.
pub fn start_page_loader(
    dirs: &[&Path],
    stems: &[String],
    max_w: u32,
    max_h: u32,
) -> Option<PageLoader> {
    if stems.is_empty() {
        return None;
    }
    let owned_dirs: Vec<PathBuf> = dirs.iter().map(|p| p.to_path_buf()).collect();
    let jobs = stems.to_vec();
    let (tx, rx) = mpsc::channel();
    let cancel = Arc::new(AtomicBool::new(false));
    let worker_cancel = Arc::clone(&cancel);
    let spawn = std::thread::Builder::new()
        .name("wa2-cg-page".into())
        .spawn(move || {
            let refs: Vec<&Path> = owned_dirs.iter().map(|p| p.as_path()).collect();
            for stem in jobs {
                if worker_cancel.load(Ordering::Relaxed) {
                    break;
                }
                let img = load_thumb(&refs, &stem, max_w, max_h);
                if tx.send((stem, img)).is_err() {
                    break;
                }
            }
        });
    if spawn.is_err() {
        return None;
    }
    Some(PageLoader { rx, cancel })
}

fn make_thumb(source: &Path, max_w: u32, max_h: u32) -> Option<TgaImage> {
    // WebP masters are resized before their pixels enter memory. This avoids
    // both the old 256x144 `tv*` fallback and a 59 MP allocation per preview.
    let preview = image::decode_file_preview(source, max_w, max_h).ok()?;
    let (w, h) = fit_size(preview.width, preview.height, max_w, max_h);
    Some(shrink(&preview, w, h))
}

fn debug_cg() -> bool {
    std::env::var_os("WA2_DEBUG_CG").is_some()
}

/// Area-average `img` down to `w`x`h`.
fn shrink(img: &TgaImage, w: u32, h: u32) -> TgaImage {
    let mut s = Surface::new(img.width, img.height, [0, 0, 0, 255]);
    s.pixels.clone_from_slice(&img.rgba);
    s.thumbnail(w, h)
}

/// Fit an image inside a destination box without changing its aspect ratio.
pub fn fit_size(width: u32, height: u32, max_w: u32, max_h: u32) -> (u32, u32) {
    let max_w = max_w.max(1);
    let max_h = max_h.max(1);
    if width == 0 || height == 0 {
        return (max_w, max_h);
    }
    if width as u64 * max_h as u64 <= height as u64 * max_w as u64 {
        (((width as u64 * max_h as u64) / height as u64).max(1) as u32, max_h)
    } else {
        (max_w, ((height as u64 * max_w as u64) / width as u64).max(1) as u32)
    }
}

/// Locate a picture across the trees in the order selected by the session.
/// This keeps a windowed overview on the 4x/2x source and switches to the 8x
/// source only when the session itself selected it for fullscreen rendering.
fn picture_path(dirs: &[&Path], stem: &str) -> Option<PathBuf> {
    for dir in dirs {
        for name in name_variants(stem) {
            let p = dir.join(&name);
            if p.is_file() {
                return Some(p);
            }
        }
    }
    None
}

/// `<stem>` plus its case variants, crossed with every extension the trees use.
fn name_variants(stem: &str) -> Vec<String> {
    let mut stems = vec![stem.to_string()];
    for v in [stem.to_lowercase(), stem.to_uppercase()] {
        if !stems.contains(&v) {
            stems.push(v);
        }
    }
    let mut out = Vec::with_capacity(stems.len() * EXTS.len());
    for s in stems {
        for e in EXTS {
            out.push(format!("{s}.{e}"));
        }
    }
    out
}

fn source_art_scale(path: &Path) -> u32 {
    image::source_art_scale(path).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write_tga(path: &Path, width: u16, height: u16) {
        let mut header = vec![0u8; 18];
        header[2] = 2;
        header[12..14].copy_from_slice(&width.to_le_bytes());
        header[14..16].copy_from_slice(&height.to_le_bytes());
        header[16] = 32;
        header[17] = 8;
        let mut file = std::fs::File::create(path).unwrap();
        use std::io::Write;
        file.write_all(&header).unwrap();
        file.write_all(&vec![0x40u8; width as usize * height as usize * 4]).unwrap();
    }

    #[test]
    fn layer_pieces_are_not_listed_as_pictures_of_their_own() {
        let base = std::env::temp_dir().join(format!("wa2-cg-test-{}", std::process::id()));
        let dir = base.join("bg");
        let _ = std::fs::create_dir_all(&dir);
        for name in ["v100100.tga", "v100200.tga", "v100200_bg.tga", "v100200_chr.tga", "b100400.tga"] {
            let _ = std::fs::write(dir.join(name), b"");
        }
        let dirs: [&Path; 1] = [dir.as_path()];
        let stems = cg_stems(&dirs);
        assert_eq!(stems, vec!["v100100".to_string(), "v100200".to_string()]);
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn name_variants_cover_case_and_extension() {
        let v = name_variants("v101700");
        assert!(v.contains(&"v101700.tga".to_string()));
        assert!(v.contains(&"V101700.TGA".to_string()));
        assert!(v.contains(&"v101700.webp".to_string()));
    }

    #[test]
    fn picture_path_follows_selected_art_tree_order() {
        let base = std::env::temp_dir().join(format!("wa2-cg-source-test-{}", std::process::id()));
        let original = base.join("bg");
        let high = base.join("bg_4x");
        std::fs::create_dir_all(&original).unwrap();
        std::fs::create_dir_all(&high).unwrap();
        write_tga(&original.join("v100100.tga"), 2, 2);
        write_tga(&high.join("v100100.tga"), 8, 8);

        let dirs: [&Path; 2] = [high.as_path(), original.as_path()];
        assert_eq!(picture_path(&dirs, "v100100"), Some(high.join("v100100.tga")));
        let dirs: [&Path; 2] = [original.as_path(), high.as_path()];
        assert_eq!(picture_path(&dirs, "v100100"), Some(original.join("v100100.tga")));
        // A tv* copy is never a substitute for the event CG itself.
        write_tga(&original.join("tv100200.tga"), 256, 144);
        assert!(picture_path(&dirs, "v100200").is_none());
        std::fs::remove_dir_all(&base).ok();
    }

    #[test]
    fn preview_box_keeps_a_cg_16_9_shape() {
        assert_eq!(fit_size(1280, 720, 480, 288), (480, 270));
        assert_eq!(fit_size(1280, 720, 720, 432), (720, 405));
        assert_eq!(fit_size(1280, 3362, 240, 144), (54, 144));
    }
}

