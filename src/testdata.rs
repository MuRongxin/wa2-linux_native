//! Locators for the test fixtures.
//!
//! Tests used to spell an absolute home-directory path to `wa2-native/data/...`,
//! so `cargo test` passed on exactly one machine and the source carried a home
//! directory that belongs to nobody else. Game data itself is not in the repository (it is
//! copyright content, restored from the private data release), so a test that
//! needs art must also be able to say "not installed here" and skip instead of
//! panicking on a fresh clone.
//!
//! Everything here resolves without mutating the environment:
//!
//! 1. `WA2_DATA` when it names a real tree;
//! 2. `<checkout>/data`, the directory `CARGO_MANIFEST_DIR` points at.
//!
//! Art directories mirror the runtime: the Chinese patch sheets win over the
//! base tree, a 4x decode reads the 4x sibling first, and the original trees
//! stay as the last fallback. A test therefore sees the same sheets a player
//! sees instead of depending on a separate 1x tree that may have been removed.

use std::path::{Path, PathBuf};

/// The checkout this test binary was compiled from.
pub fn checkout() -> &'static Path {
    Path::new(env!("CARGO_MANIFEST_DIR"))
}

/// The game-data root when it is installed, else `None`.
pub fn data_root() -> Option<PathBuf> {
    if let Some(value) = std::env::var_os("WA2_DATA") {
        let root = PathBuf::from(value);
        if root.join("script").is_dir() {
            return Some(root);
        }
    }
    let root = checkout().join("data");
    root.join("script").is_dir().then_some(root)
}

/// True when no game data is installed; prints one skip line and returns
/// `true` so the caller can `return` from a test.
pub fn no_data(what: &str) -> bool {
    if data_root().is_some() {
        return false;
    }
    eprintln!(
        "skipped {what}: no game data (expected {}/data; install the data release or point WA2_DATA at it)",
        checkout().display()
    );
    true
}

/// A path inside the data root, when both the root and the path exist.
pub fn data(rel: &str) -> Option<PathBuf> {
    let path = data_root()?.join(rel);
    path.exists().then_some(path)
}

/// `data/script`: the Japanese scenario tree the graph tests read.
pub fn script_dir() -> Option<PathBuf> {
    data("script")
}

/// `data/cache/timeline`: the pre-rendered thumbnails and their manifest.
pub fn timeline_cache() -> Option<PathBuf> {
    data("cache/timeline")
}

/// Art directories to hand to the runtime loaders, most specific first.
///
/// Same idea as `Session::bg_dirs`: the Chinese patch sheets override the base
/// tree, and the scaled sibling trees are searched before the 1280x720
/// originals. Only directories that exist are returned, so a tree layout with
/// the 1x originals removed keeps working.
pub fn art_dirs() -> Vec<PathBuf> {
    let Some(root) = data_root() else { return Vec::new() };
    let mut dirs = Vec::new();
    for scale in ["4x", "8x", "2x"] {
        for tree in ["bg_cn", "bg"] {
            let dir = root.join(format!("{tree}_{scale}"));
            if dir.is_dir() {
                dirs.push(dir);
            }
        }
    }
    for tree in ["bg_cn", "bg"] {
        let dir = root.join(tree);
        if dir.is_dir() {
            dirs.push(dir);
        }
    }
    dirs
}

/// Resolve one logical asset name (`b990000.tga`) to the file an art tree
/// holds: an upscaled sibling stores it as `b990000.webp`, and the sheets that
/// came from BMP sources keep their original upper-case stems
/// (`B990000.webp`), so the directory scan is case-insensitive.
pub fn art_file(name: &str) -> Option<PathBuf> {
    let wanted: Vec<String> = crate::codec::image::candidate_names(name)
        .into_iter()
        .map(|candidate| candidate.to_ascii_lowercase())
        .collect();
    for dir in art_dirs() {
        for candidate in crate::codec::image::candidate_names(name) {
            let path = dir.join(candidate);
            if path.is_file() {
                return Some(path);
            }
        }
        let Ok(entries) = std::fs::read_dir(&dir) else { continue };
        for entry in entries.flatten() {
            let Ok(file_name) = entry.file_name().into_string() else { continue };
            let lower = file_name.to_ascii_lowercase();
            if wanted.iter().any(|want| *want == lower) {
                let path = entry.path();
                if path.is_file() {
                    return Some(path);
                }
            }
        }
    }
    None
}

/// Decode a UI sheet (`sys_00001.tga`) the way the engine does: the logical
/// 1280x720 cache when it exists, the scaled sheet otherwise.
pub fn ui_image(name: &str) -> Option<crate::codec::tga::TgaImage> {
    crate::codec::image::decode_ui_file(&art_file(name)?).ok()
}

/// The Chinese bitmap font (atlas plus cell map) the legacy drawing tests use.
pub fn bitmap_font() -> Option<crate::render::Font> {
    let atlas_path = data("font/cn_body80.tga")?;
    let map_path = data("font/wa2_cell_map_cn.tsv")?;
    let atlas = crate::codec::tga::decode_font(&std::fs::read(atlas_path).ok()?).ok()?;
    crate::render::Font::from_cell_map(atlas, 40, &map_path).ok()
}

/// Scratch directory for tests that write images. Uses the platform temporary
/// directory instead of one shared absolute path.
pub fn out_dir() -> PathBuf {
    let dir = std::env::temp_dir().join("wa2-native-tests");
    let _ = std::fs::create_dir_all(&dir);
    dir
}
