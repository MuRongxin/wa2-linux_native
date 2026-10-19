//! Thumbnail selection and the pre-rendered cache in data/cache/timeline.
//!
//! Two concerns live here:
//!
//! 1. **Which picture represents a scene.** The audit report's finding was that
//!    the old "shared by the fewest scenes" vote promoted a late event CG over
//!    the picture the scene actually opens on. The chart now follows the
//!    scenario instead:
//!       * the scene's first resolvable ACT146/147/148 picture is the *entry*
//!         shot (ShotKind::Entry);
//!       * if that picture is exactly what the predecessor scene left on
//!         screen, the first picture that is different inside this scene wins
//!         (ShotKind::Changed);
//!       * a scene that never changes the picture inherits the predecessor's
//!         picture (ShotKind::Inherited) instead of inventing a representative
//!         of its own.
//!    Only when a scene cannot resolve any picture does it fall back to the
//!    inherited one without a pick of its own. An event CG that reuses a
//!    background id is used only when no real b* file was shipped.
//!
//! 2. **Whether the cache is still valid.** tools/build_timeline_cache.py
//!    writes 320x180 TGA thumbnails plus manifest.tsv. A cached file is only
//!    trusted when the manifest row, the cache file's size/mtime and the
//!    recorded source file's size/mtime all agree. When any of that is stale
//!    the chart must not keep showing yesterday's picture: it decodes a fresh
//!    small thumbnail from the source art instead:
//!         cache (fresh) -> tv* small copy -> unscaled original twin ->
//!         artcache / decode_art_file -> stale cache (last resort, warn)
//!    and the stale warning is printed once per open. A full 8x WebP must
//!    never be decoded just to fill a 104x59 node.

use super::*;
use crate::codec::tga::TgaImage;
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

/// Chart nodes are drawn at NODE_W x NODE_H and zoomed up to 2.4x, so the
/// working image is three times the on-screen box: the paint pass always
/// shrinks it, which keeps a zoom from turning the picture into stairs.
const NODE_IMG_W: u32 = (NODE_W * 3) as u32;
const NODE_IMG_H: u32 = (NODE_H * 3) as u32;
/// Working thumbnail size; matches tools/build_timeline_cache.py. A picture
/// decoded on the fly for a stale row is reduced to this once, exactly like a
/// cached file would have been.
const WORK_W: u32 = 320;
const WORK_H: u32 = 180;

/// One memoized load: which source picture file, what it was reduced to, and
/// whether the pixels came from the cache or a fresh source decode.
struct MemoEntry {
    img: Option<Arc<TgaImage>>,
    from_cache: bool,
}

/// Load every node's chart thumbnail and preview image.
///
/// Keys: the scene id for the node image, "preview:{id}" for the preview bar.
/// Pictures whose art cannot be loaded simply do not appear in the map.
pub(super) fn load_thumbs(dirs: &[&Path], chart: &Chart, cache_dir: &Path) -> ThumbSet {
    let started = std::time::Instant::now();
    let index = picture_index(dirs);
    let mut provider = ThumbProvider::new(cache_dir, dirs);
    // Blank white "paper" plates are not pictures of a scene; the selection
    // looks past them (see pick_scene_pictures).
    let picks = pick_scene_pictures(&index, &chart.nodes, &mut |path| provider.is_blank(path));
    let mut imgs = HashMap::new();
    let mut info = HashMap::new();
    let mut kinds = [0usize; 3];
    for (node, pick) in chart.nodes.iter().zip(&picks) {
        let Some(pick) = pick else { continue };
        let stem = file_stem_lower(&pick.path);
        let Some(img) = provider.load(&pick.path, &stem) else { continue };
        kinds[shot_kind_index(pick.kind)] += 1;
        info.insert(node.id.clone(), ThumbInfo { kind: pick.kind, path: pick.path.clone() });
        // The cached 320x180 picture is scaled exactly once here: the node
        // working image uses area averaging, the larger preview a tent filter.
        imgs.insert(
            format!("preview:{}", node.id),
            fit_smooth(&img, PREVIEW_W as u32, PREVIEW_H_THUMB as u32),
        );
        imgs.insert(node.id.clone(), fit(&img, NODE_IMG_W, NODE_IMG_H));
    }
    provider.report(chart.nodes.len(), kinds, started.elapsed());
    ThumbSet { imgs, info }
}

fn shot_kind_index(kind: ShotKind) -> usize {
    match kind {
        ShotKind::Entry => 0,
        ShotKind::Changed => 1,
        ShotKind::Inherited => 2,
    }
}

fn file_stem_lower(path: &Path) -> String {
    path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default()
}

// ------------------------------------------------------- cache freshness ----

/// One row of data/cache/timeline/manifest.tsv.
///
/// Only the fields the runtime validates are kept: cache size/mtime and the
/// recorded source path size/mtime. source_sha256 is for --verify on the tool
/// side, not for the hot path.
struct ManifestEntry {
    /// Relative to the data root, e.g. "bg_8x/b100101.webp".
    source_path: PathBuf,
    source_size: u64,
    source_mtime: u128,
    cache_size: u64,
    cache_mtime: u128,
}

fn load_manifest(cache_dir: &Path) -> HashMap<String, ManifestEntry> {
    let Ok(text) = std::fs::read_to_string(cache_dir.join("manifest.tsv")) else {
        return HashMap::new();
    };
    let mut out = HashMap::new();
    for line in text.lines() {
        let cols: Vec<&str> = line.split('\t').collect();
        // stem source_path source_size source_mtime sha256 cache_size cache_mtime
        if cols.len() != 7 || cols[0] == "stem" {
            continue;
        }
        let (Ok(source_size), Ok(source_mtime), Ok(cache_size), Ok(cache_mtime)) = (
            cols[2].parse::<u64>(),
            cols[3].parse::<u128>(),
            cols[5].parse::<u64>(),
            cols[6].parse::<u128>(),
        ) else {
            continue;
        };
        out.insert(
            cols[0].to_string(),
            ManifestEntry {
                source_path: PathBuf::from(cols[1]),
                source_size,
                source_mtime,
                cache_size,
                cache_mtime,
            },
        );
    }
    out
}

/// Nanoseconds since the epoch, the unit the build tool records.
fn mtime_ns(meta: &std::fs::Metadata) -> Option<u128> {
    let modified: SystemTime = meta.modified().ok()?;
    Some(modified.duration_since(UNIX_EPOCH).ok()?.as_nanos())
}

/// cache_dir is data/cache/timeline, so the manifest's source_paths resolve
/// against the grandparent directory.
fn data_root_of(cache_dir: &Path) -> Option<PathBuf> {
    let cache = cache_dir.parent()?;
    let root = cache.parent()?;
    if root.as_os_str().is_empty() { None } else { Some(root.to_path_buf()) }
}

/// Loads thumbnails through the cache, warning once when it is stale and
/// walking the documented fallback chain otherwise.
struct ThumbProvider<'a> {
    cache_dir: &'a Path,
    /// Art trees searched for a cheap stand-in of a stale source picture.
    dirs: &'a [&'a Path],
    data_root: Option<PathBuf>,
    manifest: HashMap<String, ManifestEntry>,
    warned_no_manifest: bool,
    stale: usize,
    stale_first: Option<String>,
    cache_hits: usize,
    decoded: usize,
    missing: usize,
    /// Whether the last uncached load was served from the cache (memo bookkeeping).
    last_from_cache: bool,
    /// The chart asks for the same picture from several scenes; decode it once.
    memo: HashMap<String, MemoEntry>,
    /// stem -> is this a near-uniform plate (blank white paper, black card)?
    blank: HashMap<String, bool>,
}

impl<'a> ThumbProvider<'a> {
    fn new(cache_dir: &'a Path, dirs: &'a [&'a Path]) -> ThumbProvider<'a> {
        ThumbProvider {
            cache_dir,
            dirs,
            data_root: data_root_of(cache_dir),
            manifest: load_manifest(cache_dir),
            warned_no_manifest: false,
            stale: 0,
            stale_first: None,
            cache_hits: 0,
            decoded: 0,
            missing: 0,
            last_from_cache: false,
            memo: HashMap::new(),
            blank: HashMap::new(),
        }
    }

    /// True for a near-uniform picture: the novel scripts open on the blank
    /// white paper plate B990000 before showing the real scene, and such a
    /// plate is never a useful chart thumbnail.
    fn is_blank(&mut self, path: &Path) -> bool {
        let stem = file_stem_lower(path);
        if let Some(blank) = self.blank.get(&stem) {
            return *blank;
        }
        // Prefer the pre-rendered 320x180 thumbnail: cheap and already at the
        // analysis size. A missing cache source only becomes blank when it can
        // be decoded cheaply; large scaled art is assumed to be a real picture.
        let cache = self.cache_dir.join(format!("{stem}.tga"));
        let img = crate::codec::image::decode_file(&cache).ok().or_else(|| {
            if source_is_small(path) {
                crate::codec::image::decode_file(path).ok()
            } else {
                None
            }
        });
        let blank = img.as_ref().is_some_and(picture_is_blank);
        self.blank.insert(stem, blank);
        blank
    }

    /// True when the manifest row, the cache file and the recorded source all
    /// agree on size and mtime. The runtime may read a different art tree (4x
    /// in windowed mode); the manifest key is the stem, so no path matching.
    fn fresh(&self, entry: &ManifestEntry, cache: &Path) -> bool {
        let Some(root) = &self.data_root else { return false };
        let Ok(cache_meta) = std::fs::metadata(cache) else { return false };
        if cache_meta.len() != entry.cache_size || mtime_ns(&cache_meta) != Some(entry.cache_mtime) {
            return false;
        }
        let Ok(source_meta) = std::fs::metadata(root.join(&entry.source_path)) else { return false };
        source_meta.len() == entry.source_size && mtime_ns(&source_meta) == Some(entry.source_mtime)
    }

    fn note_stale(&mut self, stem: &str) {
        self.stale += 1;
        if self.stale_first.is_none() {
            self.stale_first = Some(stem.to_string());
        }
    }

    /// Trust level of the cached file: fresh rows are read directly, any other
    /// state flags a stale row and forces a fresh small picture from source.
    fn cache_is_fresh(&mut self, stem: &str, cache: &Path) -> bool {
        match self.manifest.get(stem) {
            None if self.manifest.is_empty() => {
                self.warn_no_manifest();
                false
            }
            None => {
                self.note_stale(stem);
                false
            }
            Some(entry) if !self.fresh(entry, cache) => {
                self.note_stale(stem);
                false
            }
            Some(_) => true,
        }
    }

    fn load(&mut self, source: &Path, stem: &str) -> Option<Arc<TgaImage>> {
        // Several scenes can show the same picture. Counters are per request
        // (a node that shows a memoized picture is still served from the
        // cache), but the decode and the scale happen only once.
        if let Some(memo) = self.memo.get(stem) {
            let img = memo.img.clone();
            if memo.from_cache {
                self.cache_hits += 1;
            } else {
                self.decoded += 1;
            }
            return img;
        }
        let result = self.load_uncached(source, stem);
        self.memo.insert(stem.to_string(), MemoEntry { img: result.clone(), from_cache: self.last_from_cache });
        result
    }

    fn load_uncached(&mut self, source: &Path, stem: &str) -> Option<Arc<TgaImage>> {
        let cache = self.cache_dir.join(format!("{stem}.tga"));
        let fresh = self.cache_is_fresh(stem, &cache);
        self.last_from_cache = false;
        if fresh {
            match crate::codec::image::decode_file(&cache) {
                Ok(img) => {
                    self.cache_hits += 1;
                    self.last_from_cache = true;
                    return Some(Arc::new(img));
                }
                // A matching row but an unreadable file: rebuild below.
                Err(_) => self.note_stale(stem),
            }
        }
        // The cache is missing or stale. The report's P1 complaint was that an
        // outdated thumbnail kept being shown, so decode the actual source now
        // instead of trusting yesterday's picture.
        self.decoded += 1;
        if let Some(img) = self.decode_source(source) {
            return Some(reduce_to_work_size(&img));
        }
        // Source art is gone too: an outdated picture still beats a hole.
        if !fresh {
            if let Ok(img) = crate::codec::image::decode_file(&cache) {
                self.cache_hits += 1;
                self.last_from_cache = true;
                return Some(Arc::new(img));
            }
        }
        self.missing += 1;
        eprintln!("timeline: no thumbnail for {} (cache and art both failed)", source.display());
        None
    }

    /// Build a correct, small thumbnail from the source art. Everything here is
    /// cheap: the selected tv* copy or the unscaled original is a 1280x720 TGA
    /// (about ten milliseconds), never the 59 MP 8x master.
    fn decode_source(&mut self, source: &Path) -> Option<Arc<TgaImage>> {
        // 1. The selected picture is already small: a tv* stand-in or a file
        //    from the unscaled original tree.
        if source_is_small(source) {
            if let Ok(img) = crate::codec::image::decode_file(source) {
                return Some(Arc::new(img));
            }
        }
        // 2. The exact small copy of an event CG: tv{number} is the same
        //    number as v{number}. (A background can share a number with an
        //    unrelated event CG, so this mapping is only safe for v* sources.)
        if let Some(thumb) = self.event_cg_thumb(source) {
            if let Ok(img) = crate::codec::image::decode_file(&thumb) {
                return Some(Arc::new(img));
            }
        }
        // 3. The same picture from the unscaled original tree. The 4x/8x trees
        //    are upscales of those originals, so this is the same artwork for a
        //    fraction of the decode cost. Only reached for stale rows.
        if let Some(original) = self.original_twin(source) {
            if let Ok(img) = crate::codec::image::decode_file(&original) {
                return Some(Arc::new(img));
            }
        }
        // 4. Last resort: artcache decodes at the window's render scale
        //    (windowed 4x, not the 8x master) and falls back to
        //    decode_art_file internally.
        crate::artcache::load(source)
    }

    /// Exact small copy of an event CG, if one was shipped. Looks at the
    /// unscaled tree first: a 256x144 TGA decodes in about a millisecond.
    fn event_cg_thumb(&self, source: &Path) -> Option<PathBuf> {
        let stem = file_stem_lower(source);
        let number = stem.strip_prefix('v')?;
        if number.len() != 6 || !number.chars().all(|c| c.is_ascii_digit()) {
            return None;
        }
        self.lookup_picture(&format!("tv{number}"))
    }

    /// The same picture in the unscaled original tree (bg/ or bg_cn/).
    fn original_twin(&self, source: &Path) -> Option<PathBuf> {
        let stem = source.file_stem()?.to_string_lossy().to_string();
        let lower = stem.to_ascii_lowercase();
        if !lower.starts_with('b') && !lower.starts_with('v') {
            return None;
        }
        for dir in self.dirs {
            if crate::codec::image::dir_art_scale(dir) > 1 {
                continue;
            }
            for name in [&stem, &lower] {
                for ext in ["tga", "TGA", "webp", "WEBP", "bmp", "BMP"] {
                    let candidate = dir.join(format!("{name}.{ext}"));
                    if candidate.is_file() {
                        return Some(candidate);
                    }
                }
            }
        }
        None
    }

    /// Find a picture by stem, preferring the unscaled tree: its TGAs decode
    /// far faster than a 4x/8x WebP and the pixels are the same artwork.
    fn lookup_picture(&self, name: &str) -> Option<PathBuf> {
        let mut dirs: Vec<&Path> = self.dirs.iter().copied().collect();
        dirs.sort_by_key(|dir| crate::codec::image::dir_art_scale(dir));
        for dir in dirs {
            for ext in ["tga", "TGA", "webp", "WEBP"] {
                let candidate = dir.join(format!("{name}.{ext}"));
                if candidate.is_file() {
                    return Some(candidate);
                }
            }
        }
        None
    }

    fn warn_no_manifest(&mut self) {
        if self.warned_no_manifest {
            return;
        }
        self.warned_no_manifest = true;
        eprintln!(
            "timeline: no thumbnail manifest under {}; run 'python3 tools/build_timeline_cache.py --force' (rebuilding thumbnails from the source pictures)",
            self.cache_dir.display()
        );
    }

    fn report(&self, nodes: usize, kinds: [usize; 3], elapsed: std::time::Duration) {
        eprintln!(
            "timeline: {nodes} scenes, thumbnails: entry {}, changed {}, inherited {} ({} from cache, {} decoded, {} missing) in {} ms",
            kinds[0],
            kinds[1],
            kinds[2],
            self.cache_hits,
            self.decoded,
            self.missing,
            elapsed.as_millis()
        );
        if self.stale > 0 {
            eprintln!(
                "timeline: {} thumbnail cache row(s) stale (first: {}); run 'python3 tools/build_timeline_cache.py --force'",
                self.stale,
                self.stale_first.as_deref().unwrap_or("?")
            );
        }
    }
}

/// True when every channel sits inside a narrow band: the white paper plate
/// B990000 (range 1 out of 255) and black cards count as blank, a dark night
/// scene (range 255) does not.
fn picture_is_blank(img: &TgaImage) -> bool {
    let pixels = img.width as usize * img.height as usize;
    if pixels == 0 {
        return true;
    }
    // Sampling a few thousand pixels is plenty to recognise a uniform plate.
    let step = (pixels / 4096).max(1);
    let mut lo = 255u8;
    let mut hi = 0u8;
    let mut i = 0usize;
    while i < pixels {
        let si = i * 4;
        for channel in 0..3 {
            let v = img.rgba[si + channel];
            lo = lo.min(v);
            hi = hi.max(v);
        }
        i += step;
    }
    hi - lo <= 8
}

/// Reduce an on-the-fly source decode to the same working size the build tool
/// writes (320x180), so the rest of the chart always scales from one size.
fn reduce_to_work_size(img: &TgaImage) -> Arc<TgaImage> {
    if img.width <= WORK_W && img.height <= WORK_H {
        return Arc::new(TgaImage { width: img.width, height: img.height, rgba: img.rgba.clone() });
    }
    Arc::new(fit(img, WORK_W, WORK_H))
}

/// The selected path is safe to decode whole when it is a tv* small copy or a
/// file from the unscaled original tree. Backgrounds and event CGs from a
/// 2x/4x/8x tree are large and must go through the art cache instead.
fn source_is_small(path: &Path) -> bool {
    let stem = path.file_stem().map(|s| s.to_string_lossy().to_ascii_lowercase()).unwrap_or_default();
    stem.starts_with("tv") || crate::codec::image::dir_art_scale(path) <= 1
}

// -------------------------------------------------------- scene pictures ----

/// One scene's chosen chart picture.
#[derive(Clone)]
struct Pick {
    kind: ShotKind,
    path: PathBuf,
}

struct Pic {
    variant: i32,
    /// A small tv* stand-in of this same picture.
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

/// Decide every scene's representative picture along the scenario graph.
///
/// The graph edges always move forward in columns, so processing the nodes in
/// column order is a topological order: a predecessor's "last picture on
/// screen" is already known when its successor is visited.
///
/// The blank check recognises near-uniform placeholders (the white paper plate
/// that opens the novel scenes). Blank candidates are skipped when a real
/// picture exists later in the same scene; a scene with only blank plates
/// keeps one.
fn pick_scene_pictures(
    index: &PictureIndex,
    nodes: &[Node],
    is_blank: &mut impl FnMut(&Path) -> bool,
) -> Vec<Option<Pick>> {
    let resolved: Vec<Vec<Option<PathBuf>>> = nodes
        .iter()
        .map(|node| node.shot.iter().map(|shot| resolve_thumb(index, shot)).collect())
        .collect();
    let mut preds: Vec<Vec<usize>> = vec![Vec::new(); nodes.len()];
    for (i, node) in nodes.iter().enumerate() {
        for edge in &node.next {
            preds[edge.to].push(i);
        }
    }
    let mut order: Vec<usize> = (0..nodes.len()).collect();
    order.sort_by_key(|&i| (nodes[i].col, i));
    // What the scene leaves on screen, so its successor can detect a repeat.
    let mut display_after: Vec<Option<PathBuf>> = vec![None; nodes.len()];
    let mut picks: Vec<Option<Pick>> = vec![None; nodes.len()];
    for &i in &order {
        let shots = &resolved[i];
        let entry = shots.iter().flatten().next().cloned();
        let mut parents = preds[i].clone();
        parents.sort_unstable();
        // The scene opened by restating the predecessor's last picture?
        let repeated = entry.as_ref().and_then(|entry_path| {
            parents.iter().copied().find(|&p| display_after[p].as_ref() == Some(entry_path))
        });
        let inherited = parents.iter().copied().find_map(|p| display_after[p].clone());
        // The last picture the script shows is what the next scene starts
        // against; a scene without pictures leaves the previous one up.
        display_after[i] = shots.iter().flatten().last().cloned().or_else(|| inherited.clone());
        picks[i] = if let Some(pred) = repeated {
            let before = display_after[pred].clone().unwrap_or_default();
            match shots.iter().flatten().find(|path| path.as_path() != before.as_path()) {
                // The scene does show something new - that change is the
                // representative, not a later event CG.
                Some(path) => Some(Pick { kind: ShotKind::Changed, path: path.clone() }),
                // The whole scene stays on the inherited picture.
                None => Some(Pick { kind: ShotKind::Inherited, path: before }),
            }
        } else if let Some(entry) = entry {
            Some(Pick { kind: ShotKind::Entry, path: entry })
        } else {
            // No resolvable picture of its own: inherit whatever the nearest
            // predecessor left on screen.
            inherited.map(|path| Pick { kind: ShotKind::Inherited, path })
        };
        // A scene can open on a blank paper plate before the real picture; the
        // novel chain (5000 -> 9804 -> ...) does exactly this. Skip the blank
        // plate and take the first real picture inside the scene.
        if let Some(pick) = picks[i].as_ref() {
            if is_blank(&pick.path) {
                let mut real = None;
                for path in shots.iter().flatten() {
                    if !is_blank(path) {
                        real = Some(path.clone());
                        break;
                    }
                }
                if let Some(path) = real {
                    picks[i] = Some(Pick { kind: ShotKind::Changed, path });
                }
            }
        }
        // Keep the "adjacent nodes do not show the same picture" intent: when
        // the entry picture is already the predecessor's representative (two
        // consecutive scenes establishing the same location), show the first
        // picture this scene introduces instead. The two IC street scenes are
        // the clearest example: 1002 and 1003 both open on background 1001.
        if let Some(pick) = picks[i].as_ref() {
            let repeated_on_chart = parents.iter().copied().find(|&p| {
                picks[p].as_ref().is_some_and(|parent| parent.path == pick.path)
            });
            if repeated_on_chart.is_some() {
                let mut different = None;
                for path in shots.iter().flatten() {
                    if path.as_path() != pick.path.as_path() && !is_blank(path) {
                        different = Some(path.clone());
                        break;
                    }
                }
                if different.is_none() {
                    different = shots
                        .iter()
                        .flatten()
                        .find(|path| path.as_path() != pick.path.as_path())
                        .cloned();
                }
                if let Some(path) = different {
                    picks[i] = Some(Pick { kind: ShotKind::Changed, path });
                }
            }
        }
    }
    picks
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
    // Background ids that have at least one shipped `b*` file, any variant.
    // A `v{id:04}{variant:02}` file is an event CG that happens to share the
    // six digits; it must not stand in for the background when the id did
    // ship backgrounds under another variant (only b206702/b206712 exist for
    // id 2067, and v206700 is an unrelated CG).
    let mut bg_families: HashSet<i64> = HashSet::new();
    for name in files.keys() {
        let stem = if let Some(stem) = name.strip_suffix(".tga") {
            stem
        } else if let Some(stem) = name.strip_suffix(".webp") {
            stem
        } else if let Some(stem) = name.strip_suffix(".bmp") {
            stem
        } else {
            continue;
        };
        let Some(rest) = stem.strip_prefix('b') else { continue };
        if rest.len() == 6 && rest.chars().all(|c| c.is_ascii_digit()) {
            if let Ok(number) = rest.parse::<i64>() {
                bg_families.insert(number / 100);
            }
        }
    }
    let mut index = PictureIndex { bg: HashMap::new(), cg: HashMap::new() };
    for (name, path) in &files {
        let stem = if let Some(stem) = name.strip_suffix(".tga") {
            stem
        } else if let Some(stem) = name.strip_suffix(".webp") {
            stem
        } else if let Some(stem) = name.strip_suffix(".bmp") {
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
        if !bg_families.contains(&(number / 100)) {
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

/// The full-resolution file for a shot: used by tests to pin the
/// background-over-CG rule; the timeline itself always goes through
/// resolve_thumb so it can take the small tv* copy.
#[cfg(test)]
fn resolve(index: &PictureIndex, shot: &Shot) -> Option<PathBuf> {
    let pics = if shot.cg { index.cg.get(&shot.id)? } else { index.bg.get(&shot.id)? };
    pics.iter()
        .min_by_key(|pic| {
            (
                u8::from(pic.variant != shot.variant),
                // The shipped background wins over a CG that reuses the number.
                u8::from(!shot.cg && !pic.background),
                u8::from(pic.thumb),
                u8::from(pic.variant != 0),
                pic.variant,
            )
        })
        .map(|pic| pic.path.clone())
}

/// Like resolve, but prefers the small tv* copy of a picture. Timeline nodes
/// are 104x59 and the preview is 208x117, so decoding the full CG is wasted
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
                // A real background beats an event CG that reuses the number;
                // the CG is only used when no b* file was shipped.
                u8::from(!shot.cg && !pic.background),
                u8::from(pic.variant != 0),
                pic.variant,
            )
        })
        .map(|pic| pic.path.clone())
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

/// Tent-filter scale for the 208x117 preview. Area averaging keeps the tiny
/// node crisp, but at preview size a triangle kernel keeps a little more
/// mid-frequency detail without ringing halos.
fn fit_smooth(img: &TgaImage, dw: u32, dh: u32) -> TgaImage {
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
    let fx = sw as f32 / dw as f32;
    let fy = sh as f32 / dh as f32;
    let rx = fx.max(1.0);
    let ry = fy.max(1.0);
    for y in 0..dh as usize {
        let cy = (y as f32 + 0.5) * fy - 0.5;
        let y0 = (cy - ry).floor().max(0.0) as usize;
        let y1 = ((cy + ry).ceil() as usize).min(sh - 1);
        for x in 0..dw as usize {
            let cx = (x as f32 + 0.5) * fx - 0.5;
            let x0 = (cx - rx).floor().max(0.0) as usize;
            let x1 = ((cx + rx).ceil() as usize).min(sw - 1);
            let mut acc = [0f32; 4];
            let mut weight = 0f32;
            for sy in y0..=y1 {
                let wy = 1.0 - ((sy as f32 - cy) / ry).abs();
                if wy <= 0.0 {
                    continue;
                }
                let row = sy * sw * 4;
                for sx in x0..=x1 {
                    let wx = 1.0 - ((sx as f32 - cx) / rx).abs();
                    if wx <= 0.0 {
                        continue;
                    }
                    let w = wx * wy;
                    let si = row + sx * 4;
                    acc[0] += img.rgba[si] as f32 * w;
                    acc[1] += img.rgba[si + 1] as f32 * w;
                    acc[2] += img.rgba[si + 2] as f32 * w;
                    acc[3] += img.rgba[si + 3] as f32 * w;
                    weight += w;
                }
            }
            let di = (y * dw as usize + x) * 4;
            let weight = weight.max(0.0001);
            for c in 0..4 {
                rgba[di + c] = (acc[c] / weight).round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    TgaImage { width: dw, height: dh, rgba }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::codec::tga::TgaImage;

    fn pic(variant: i32, thumb: bool, background: bool, name: &str) -> Pic {
        Pic { variant, thumb, background, path: PathBuf::from(name) }
    }

    fn shot(cg: bool, id: i64, variant: i32) -> Shot {
        Shot { cg, id, variant, op_index: 0, source: String::new() }
    }

    fn node(id: &str, col: i32, shots: Vec<Shot>, next: Vec<usize>) -> Node {
        Node {
            id: id.to_string(),
            chapter: chapters::other(),
            col,
            lane: 0,
            next: next
                .into_iter()
                .map(|to| Edge { to, kind: EdgeKind::Sequential, label: String::new() })
                .collect(),
            root: false,
            ending: false,
            shot: shots,
        }
    }

    #[test]
    fn thumbnails_are_preferred_for_chart_nodes() {
        let mut bg = HashMap::new();
        bg.insert(
            1001,
            vec![pic(0, false, true, "b100100.tga"), pic(0, true, false, "tv100100.tga")],
        );
        let index = PictureIndex { bg, cg: HashMap::new() };
        let shot = shot(false, 1001, 0);
        let thumb = resolve_thumb(&index, &shot).unwrap();
        assert!(thumb.ends_with("tv100100.tga"), "{thumb:?}");
        // The general resolver still prefers the full event CG.
        let full = resolve(&index, &shot).unwrap();
        assert!(full.ends_with("b100100.tga"), "{full:?}");
    }

    #[test]
    fn entry_changed_and_inherited_follow_the_scenario() {
        let mut bg = HashMap::new();
        for (id, name) in [(1, "b000100.tga"), (2, "b000200.tga"), (3, "b000300.tga"), (4, "b000400.tga")] {
            bg.insert(id, vec![pic(0, false, true, name)]);
        }
        let index = PictureIndex { bg, cg: HashMap::new() };
        let nodes = vec![
            // scene 1: opens on 1 then moves to 2; leaves 2 on screen
            node("1", 0, vec![shot(false, 1, 0), shot(false, 2, 0)], vec![1]),
            // scene 2: restates 2 (a repeat) then changes to 3 -> Changed 3
            node("2", 1, vec![shot(false, 2, 0), shot(false, 3, 0)], vec![2]),
            // scene 3: only restates 3 -> Inherited 3
            node("3", 2, vec![shot(false, 3, 0)], vec![3]),
            // scene 4: never repeats the predecessor, so its first picture is
            // still an entry shot.
            node("4", 3, vec![shot(false, 4, 0)], vec![]),
        ];
        let picks = pick_scene_pictures(&index, &nodes, &mut |_| false);
        let name = |i: usize| {
            picks[i].as_ref().map(|p| (p.kind, p.path.file_name().unwrap().to_string_lossy().into_owned()))
        };
        assert_eq!(name(0), Some((ShotKind::Entry, "b000100.tga".to_string())));
        assert_eq!(name(1), Some((ShotKind::Changed, "b000300.tga".to_string())));
        assert_eq!(name(2), Some((ShotKind::Inherited, "b000300.tga".to_string())));
        assert_eq!(name(3), Some((ShotKind::Entry, "b000400.tga".to_string())));
    }

    #[test]
    fn a_scene_without_pictures_inherits_from_its_predecessor() {
        let mut bg = HashMap::new();
        bg.insert(1, vec![pic(0, false, true, "b000100.tga")]);
        bg.insert(2, vec![pic(0, false, true, "b000200.tga")]);
        let index = PictureIndex { bg, cg: HashMap::new() };
        let nodes = vec![
            node("1", 0, vec![shot(false, 1, 0)], vec![1]),
            // 2 has no picture of its own; 3 starts with 1 again (the picture
            // still on screen) and then changes to 2 - the inherited chain is
            // followed through the empty node.
            node("2", 1, vec![], vec![2]),
            node("3", 2, vec![shot(false, 1, 0), shot(false, 2, 0)], vec![]),
        ];
        let picks = pick_scene_pictures(&index, &nodes, &mut |_| false);
        assert_eq!(picks[0].as_ref().unwrap().kind, ShotKind::Entry);
        let inherited = picks[1].as_ref().expect("empty scene should inherit");
        assert_eq!(inherited.kind, ShotKind::Inherited);
        assert!(inherited.path.ends_with("b000100.tga"));
        let changed = picks[2].as_ref().expect("third scene picks its change");
        assert_eq!(changed.kind, ShotKind::Changed);
        assert!(changed.path.ends_with("b000200.tga"), "{:?}", changed.path);
    }

    #[test]
    fn a_blank_entry_plate_is_skipped_for_the_first_real_picture() {
        let mut bg = HashMap::new();
        // 9900/0 is the white paper plate (B990000), 9804/0 the real picture.
        bg.insert(9900, vec![pic(0, false, true, "b990000.tga")]);
        bg.insert(9804, vec![pic(0, false, true, "b980400.tga")]);
        let index = PictureIndex { bg, cg: HashMap::new() };
        let mut blank = |path: &Path| path.to_string_lossy().to_ascii_lowercase().contains("b990000");
        let nodes = vec![
            node("5000", 0, vec![shot(false, 9900, 0), shot(false, 9804, 0)], vec![1]),
            // This scene really has nothing but the empty plate.
            node("5001", 1, vec![shot(false, 9900, 0)], vec![]),
        ];
        let picks = pick_scene_pictures(&index, &nodes, &mut blank);
        let picked = |i: usize| {
            picks[i].as_ref().map(|p| (p.kind, p.path.file_name().unwrap().to_string_lossy().into_owned()))
        };
        assert_eq!(picked(0), Some((ShotKind::Changed, "b980400.tga".to_string())));
        assert_eq!(picked(1), Some((ShotKind::Entry, "b990000.tga".to_string())));
    }

    #[test]
    fn blank_detection_tells_the_paper_plate_from_a_dark_scene() {
        let cache_dir = Path::new("data/cache/timeline");
        if !cache_dir.is_dir() {
            return;
        }
        let mut provider = ThumbProvider::new(cache_dir, &[]);
        // The white paper plate is near-uniform, a night scene is not.
        assert!(provider.is_blank(Path::new("data/bg/b990000.tga")));
        assert!(!provider.is_blank(Path::new("data/bg/b980000.tga")));
        assert!(!provider.is_blank(Path::new("data/bg/b106001.tga")));
    }

    #[test]
    fn novel_roots_do_not_show_the_blank_paper_plate() {
        let bg = Path::new("data/bg");
        let cache_dir = Path::new("data/cache/timeline");
        if !bg.is_dir() || !cache_dir.is_dir() {
            return;
        }
        let nodes = graph::build_nodes(Path::new("data/script"));
        let index = picture_index(&[bg]);
        let dirs: [&Path; 1] = [bg];
        let mut provider = ThumbProvider::new(cache_dir, &dirs);
        let picks = pick_scene_pictures(&index, &nodes, &mut |path| provider.is_blank(path));
        let pick_of = |id: &str| -> Option<String> {
            let i = nodes.iter().position(|n| n.id == id)?;
            picks[i]
                .as_ref()
                .and_then(|p| p.path.file_name())
                .map(|s| s.to_string_lossy().into_owned())
        };
        for id in ["5000", "5100"] {
            let file = pick_of(id).unwrap_or_default();
            assert_ne!(file.as_str(), "b990000.tga", "{id} still shows the blank plate");
            let i = nodes.iter().position(|n| n.id == id).unwrap();
            let pick = picks[i].as_ref().unwrap();
            assert!(!provider.is_blank(&pick.path), "{id} picked a blank picture: {file:?}");
        }
        // The normal chapters are unaffected: 1001 still opens on its own art.
        assert_eq!(pick_of("1001").as_deref(), Some("b106001.tga"));
    }

    #[test]
    fn neighbouring_scenes_do_not_share_a_thumbnail() {
        let bg = Path::new("data/bg");
        if !bg.is_dir() {
            return;
        }
        let nodes = graph::build_nodes(Path::new("data/script"));
        let index = picture_index(&[bg]);
        let picks = pick_scene_pictures(&index, &nodes, &mut |_| false);
        let name = |id: &str| {
            let i = nodes.iter().position(|n| n.id == id)?;
            picks[i].as_ref().and_then(|p| p.path.file_name()).map(|s| s.to_string_lossy().into_owned())
        };
        let mut kinds = [0usize; 3];
        for pick in picks.iter().flatten() {
            kinds[shot_kind_index(pick.kind)] += 1;
        }
        let pictured = nodes.iter().filter(|n| !n.shot.is_empty()).count();
        let picked = picks.iter().flatten().count();
        assert_eq!(picked, pictured, "every pictured scene should get a pick");
        println!("timeline picks: entry {}, changed {}, inherited {} over {pictured} scenes", kinds[0], kinds[1], kinds[2]);
        assert!(kinds[0] > 40, "expected mostly entry shots, got {kinds:?}");
        assert!(kinds[1] > 0, "expected some changed scenes, got {kinds:?}");
        // Inherited is only possible for a scene that never changes the
        // picture at all; the shipped scripts always move the camera, so zero
        // is a valid count here (the synthetic tests above cover the branch).
        // Scenes that open by repeating the previous location must not both
        // show that location's picture.
        assert_ne!(name("1002"), name("1003"), "opening street reused");
        assert_ne!(name("1012"), name("1013"), "bedroom reused");
        let distinct = picks.iter().flatten().map(|p| &p.path).collect::<std::collections::HashSet<_>>().len();
        // Entry shots deliberately repeat common establishing locations (the
        // street outside the school, the apartment), so the old "least shared
        // picture" spread is gone; adjacency is what matters on the chart.
        assert!(distinct > 60, "thumbnails collapsed onto {distinct} files for {pictured} scenes");
        // Adjacent nodes only share a picture when the successor scene does not
        // contain any other usable picture - that is the chart-level promise.
        for (i, node) in nodes.iter().enumerate() {
            let Some(here) = picks[i].as_ref() else { continue };
            for edge in &node.next {
                let Some(there) = picks[edge.to].as_ref() else { continue };
                if here.path == there.path {
                    let alternatives = nodes[edge.to]
                        .shot
                        .iter()
                        .filter_map(|s| resolve_thumb(&index, s))
                        .any(|p| p != here.path);
                    assert!(!alternatives, "{} and {} share a picture but {} has alternatives", node.id, nodes[edge.to].id, nodes[edge.to].id);
                }
            }
        }
        // The event CG sharing 1017's number never wins over the real room.
        let room = name("3211").unwrap_or_default();
        assert_ne!(room, "tv101700.tga");
    }

    #[test]
    fn a_missing_cache_file_falls_back_to_the_source_picture() {
        // Isolated temp data root: the real data/cache/timeline is never touched.
        let dir = std::env::temp_dir().join(format!("wa2-shots-fallback-{}-{}", std::process::id(), line!()));
        let cache_dir = dir.join("cache").join("timeline");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let source_dir = dir.join("bg");
        std::fs::create_dir_all(&source_dir).unwrap();
        let source = source_dir.join("tv100100.tga");
        let tiny = TgaImage { width: 2, height: 2, rgba: [10u8, 20, 30, 255].repeat(4) };
        std::fs::write(&source, crate::codec::tga::encode(&tiny)).unwrap();
        let mut provider = ThumbProvider::new(&cache_dir, &[]);
        let loaded = provider.load(&source, "tv100100").expect("a small tv* source must decode");
        assert_eq!((loaded.width, loaded.height), (2, 2));
        assert_eq!(provider.cache_hits, 0);
        assert_eq!(provider.decoded, 1, "cache miss must use the source fallback");
        assert_eq!(provider.missing, 0);
        std::fs::remove_dir_all(&dir).ok();
    }

    fn write_tga(path: &Path, rgba: [u8; 4]) {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent).unwrap();
        }
        let img = TgaImage { width: 2, height: 2, rgba: rgba.repeat(4) };
        std::fs::write(path, crate::codec::tga::encode(&img)).unwrap();
    }

    fn header() -> &'static str {
        "stem\tsource_path\tsource_size\tsource_mtime\tsource_sha256\tcache_size\tcache_mtime\n"
    }

    #[test]
    fn a_stale_row_rebuilds_the_thumbnail_from_the_source() {
        let dir = std::env::temp_dir().join(format!("wa2-shots-stale-{}-{}", std::process::id(), line!()));
        let cache_dir = dir.join("cache").join("timeline");
        std::fs::create_dir_all(&cache_dir).unwrap();
        // The cached file still holds yesterday's black picture.
        write_tga(&cache_dir.join("tv100100.tga"), [0, 0, 0, 255]);
        let cache_meta = std::fs::metadata(cache_dir.join("tv100100.tga")).unwrap();
        // The source picture has since changed; the manifest row is behind.
        let source_dir = dir.join("bg");
        let source = source_dir.join("tv100100.tga");
        write_tga(&source, [200, 10, 10, 255]);
        let row = format!(
            "tv100100\tbg/tv100100.tga\t1\t1\tdeadbeef\t{}\t{}\n",
            cache_meta.len(),
            mtime_ns(&cache_meta).unwrap(),
        );
        std::fs::write(cache_dir.join("manifest.tsv"), format!("{}{row}", header())).unwrap();
        let dirs: [&Path; 1] = [source_dir.as_path()];
        let mut provider = ThumbProvider::new(&cache_dir, &dirs);
        let img = provider.load(&source, "tv100100").expect("stale row must decode the source");
        assert_eq!(&img.rgba[..4], &[200, 10, 10, 255], "the stale cache must not be shown");
        assert_eq!(provider.cache_hits, 0);
        assert_eq!(provider.decoded, 1);
        assert_eq!(provider.stale, 1);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn a_dead_source_falls_back_to_the_stale_cache() {
        let dir = std::env::temp_dir().join(format!("wa2-shots-dead-{}-{}", std::process::id(), line!()));
        let cache_dir = dir.join("cache").join("timeline");
        std::fs::create_dir_all(&cache_dir).unwrap();
        write_tga(&cache_dir.join("tv100100.tga"), [0, 200, 0, 255]);
        let cache_meta = std::fs::metadata(cache_dir.join("tv100100.tga")).unwrap();
        let source_dir = dir.join("bg");
        std::fs::create_dir_all(&source_dir).unwrap();
        // Manifest names a source file that no longer exists.
        let row = format!(
            "tv100100\tbg/tv999999.tga\t100\t100\tdeadbeef\t{}\t{}\n",
            cache_meta.len(),
            mtime_ns(&cache_meta).unwrap(),
        );
        std::fs::write(cache_dir.join("manifest.tsv"), format!("{}{row}", header())).unwrap();
        let dirs: [&Path; 1] = [source_dir.as_path()];
        let mut provider = ThumbProvider::new(&cache_dir, &dirs);
        let missing = source_dir.join("tv100100.tga");
        let img = provider.load(&missing, "tv100100").expect("an outdated picture beats a hole");
        assert_eq!(&img.rgba[..4], &[0, 200, 0, 255]);
        assert_eq!(provider.cache_hits, 1);
        assert_eq!(provider.missing, 0);
        assert_eq!(provider.stale, 1);
        std::fs::remove_dir_all(&dir).ok();
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

    #[test]
    fn preview_tent_filter_matches_the_box_on_a_2x2_source() {
        let img = TgaImage {
            width: 2,
            height: 2,
            rgba: vec![0, 0, 0, 255, 255, 0, 0, 255, 0, 255, 0, 255, 0, 0, 255, 255],
        };
        let tent = fit_smooth(&img, 1, 1);
        let boxed = fit(&img, 1, 1);
        for channel in 0..4 {
            let d = (tent.rgba[channel] as i32 - boxed.rgba[channel] as i32).abs();
            assert!(d <= 1, "tent {:?} vs box {:?}", &tent.rgba[..4], &boxed.rgba[..4]);
        }
    }

    #[test]
    fn manifest_rows_are_parsed_and_validated() {
        let dir = std::env::temp_dir().join(format!("wa2-shots-{}-{}", std::process::id(), line!()));
        let cache_dir = dir.join("cache").join("timeline");
        std::fs::create_dir_all(&cache_dir).unwrap();
        let cache = cache_dir.join("b100101.tga");
        std::fs::write(&cache, b"cache").unwrap();
        let cache_meta = std::fs::metadata(&cache).unwrap();
        let source = dir.join("bg_8x").join("b100101.webp");
        std::fs::create_dir_all(source.parent().unwrap()).unwrap();
        std::fs::write(&source, b"source").unwrap();
        let source_meta = std::fs::metadata(&source).unwrap();
        let header = "stem\tsource_path\tsource_size\tsource_mtime\tsource_sha256\tcache_size\tcache_mtime\n";
        let row = format!(
            "b100101\tbg_8x/b100101.webp\t{}\t{}\tdeadbeef\t{}\t{}\n",
            source_meta.len(),
            mtime_ns(&source_meta).unwrap(),
            cache_meta.len(),
            mtime_ns(&cache_meta).unwrap(),
        );
        std::fs::write(cache_dir.join("manifest.tsv"), format!("{header}{row}")).unwrap();
        let provider = ThumbProvider::new(&cache_dir, &[]);
        let entry = provider.manifest.get("b100101").expect("manifest row");
        assert!(provider.fresh(entry, &cache), "a matching row must be fresh");
        // A source edit invalidates the row even when the cache is untouched.
        std::fs::write(&source, b"source-edited").unwrap();
        assert!(!provider.fresh(entry, &cache), "a changed source must invalidate the cache");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_event_cg_number_does_not_replace_the_shipped_background() {
        // Scene 3005 opens on id 2067; only b206702/b206712 were shipped, and
        // v206700 is an unrelated event CG. The timeline must show b206702.
        let dir = std::env::temp_dir().join(format!("wa2-shots-bgfamily-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).unwrap();
        for name in ["b206702.tga", "b206712.tga", "v206700.tga"] {
            std::fs::write(dir.join(name), b"").unwrap();
        }
        let index = picture_index(&[dir.as_path()]);
        let shot = Shot { cg: false, id: 2067, variant: 0, op_index: 0, source: String::new() };
        let picked = resolve_thumb(&index, &shot).expect("a background should resolve");
        assert_eq!(picked.file_name().unwrap().to_string_lossy(), "b206702.tga");
        // An event CG that really is the only file still works.
        let only_dir = std::env::temp_dir().join(format!("wa2-shots-cgonly-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&only_dir);
        std::fs::create_dir_all(&only_dir).unwrap();
        std::fs::write(only_dir.join("v206700.tga"), b"").unwrap();
        let only = picture_index(&[only_dir.as_path()]);
        let picked = resolve_thumb(&only, &shot).expect("the CG stand-in should resolve");
        assert_eq!(picked.file_name().unwrap().to_string_lossy(), "v206700.tga");
        std::fs::remove_dir_all(&dir).ok();
        std::fs::remove_dir_all(&only_dir).ok();
    }
}
