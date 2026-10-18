//! Derived art cache: decode a high-resolution source once, reuse the pixels.
//!
//! The 8x/4x WebP trees are the only high-quality source, but decoding a 59 MP
//! lossless WebP costs a quarter of a second to over a second, which is far too
//! slow to happen while the player is waiting for a scene change. Two caches
//! sit in front of the decoder:
//!
//!   * a bounded in-memory LRU shared by every loader, so a picture the player
//!     comes back to inside one session is free;
//!   * a disk cache of already-scaled TGA files under data/cache/art/<N>x/,
//!     so the second and later sessions read a plain file instead of decoding
//!     the 8x source again.
//!
//! Both are keyed by the source path **and** the scale the decoded pixels
//! represent, so a window resize never mixes pixel scales.
//!
//! A background thread (prefetch) decodes pictures the script has not reached
//! yet while the player is reading, which is what actually removes the stall:
//! by the time the script asks for the picture it is already cached.

use crate::codec::image;
use crate::codec::tga::{self, TgaImage};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::SystemTime;

/// data/cache/art, the derived tree. Never part of the repository.
fn root() -> PathBuf {
    let base = std::env::var_os("WA2_DATA")
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("data"));
    base.join("cache").join("art")
}

fn mtime(path: &Path) -> Option<SystemTime> {
    path.metadata().ok()?.modified().ok()
}

/// <root>/<scale>x/<tree>/<stem>.tga for a source data/<tree>/<stem>.webp.
fn disk_path(src: &Path, scale: u32) -> Option<PathBuf> {
    let tree = src.parent()?.file_name()?;
    let stem = src.file_stem()?;
    Some(
        root()
            .join(format!("{scale}x"))
            .join(tree)
            .join(format!("{}.tga", stem.to_string_lossy())),
    )
}

/// The disk cache is only worth its bytes when reading it is much cheaper than
/// decoding the source. A 1x TGA source is decoded by reading it, so caching it
/// would only copy it; and a full 8x decode would write a 236 MB file.
fn worth_caching(src: &Path, target: u32, source: u32) -> bool {
    if !std::env::var("WA2_ART_CACHE")
        .ok()
        .and_then(|v| v.parse::<u32>().ok())
        .map(|v| v != 0)
        .unwrap_or(true)
    {
        return false;
    }
    if target > 4 {
        return false;
    }
    let webp = src
        .extension()
        .and_then(|s| s.to_str())
        .is_some_and(|s| s.eq_ignore_ascii_case("webp"));
    webp || target < source
}

// ---------------------------------------------------------------- memory ----

#[derive(PartialEq, Eq, Hash, Clone)]
struct Key {
    path: PathBuf,
    scale: u32,
}

struct Entry {
    img: Arc<TgaImage>,
    bytes: usize,
    /// The background on screen and the one being faded to are never evicted.
    pinned: bool,
    used: u64,
}

struct Mem {
    map: HashMap<Key, Entry>,
    bytes: usize,
    budget: usize,
    clock: u64,
}

fn mem() -> &'static Mutex<Mem> {
    static MEM: OnceLock<Mutex<Mem>> = OnceLock::new();
    MEM.get_or_init(|| {
        // 1x backgrounds are 3.7 MB each; 512 MB holds a whole scene's worth of
        // 1x art or a handful of 8x frames, which is all the renderer needs.
        let budget = std::env::var("WA2_ART_CACHE_MB")
            .ok()
            .and_then(|v| v.parse::<usize>().ok())
            .unwrap_or(512)
            * 1024
            * 1024;
        Mutex::new(Mem { map: HashMap::new(), bytes: 0, budget, clock: 0 })
    })
}

fn evict(m: &mut Mem) {
    if m.bytes <= m.budget {
        return;
    }
    let mut victims: Vec<(Key, u64)> = m
        .map
        .iter()
        .filter(|(_, e)| !e.pinned)
        .map(|(k, e)| (k.clone(), e.used))
        .collect();
    if victims.is_empty() {
        return;
    }
    victims.sort_by_key(|(_, used)| *used);
    // The newest entry is the picture on screen: keep it however big it is,
    // so a burst of prefetching can never push the current scene out.
    let newest = victims.pop();
    for (k, _) in victims {
        if m.bytes <= m.budget {
            break;
        }
        if let Some(e) = m.map.remove(&k) {
            m.bytes = m.bytes.saturating_sub(e.bytes);
        }
    }
    let _ = newest;
}

fn insert(src: &Path, scale: u32, img: Arc<TgaImage>) {
    let mut m = match mem().lock() {
        Ok(m) => m,
        Err(e) => e.into_inner(),
    };
    let key = Key { path: src.to_path_buf(), scale };
    let bytes = img.rgba.len();
    if let Some(old) = m.map.remove(&key) {
        m.bytes = m.bytes.saturating_sub(old.bytes);
    }
    let used = m.clock + 1;
    m.clock = used;
    m.bytes += bytes;
    m.map.insert(key, Entry { img, bytes, pinned: false, used });
    evict(&mut m);
}

/// Look the decoded pixels up without decoding anything. Counts as a use, so
/// a picture that is still being asked for is not the one evicted next.
pub fn peek(src: &Path) -> Option<Arc<TgaImage>> {
    let (_, scale) = image::art_scales(src);
    let mut m = match mem().lock() {
        Ok(m) => m,
        Err(e) => e.into_inner(),
    };
    m.clock += 1;
    let clock = m.clock;
    let e = m.map.get_mut(&Key { path: src.to_path_buf(), scale })?;
    e.used = clock;
    Some(Arc::clone(&e.img))
}

// ------------------------------------------------------------------ disk ----

/// Read a scaled TGA written by a previous session. None means the caller has
/// to decode the source.
fn read_disk(src: &Path, scale: u32, want: Option<(u32, u32)>) -> Option<TgaImage> {
    let p = disk_path(src, scale)?;
    // A regenerated source is newer than the file derived from it.
    if mtime(&p)? < mtime(src)? {
        return None;
    }
    let data = std::fs::read(&p).ok()?;
    let img = tga::decode(&data).ok()?;
    if let Some((w, h)) = want {
        if img.width != w || img.height != h {
            return None;
        }
    }
    Some(img)
}

/// Write through a temporary file and rename, so a reader never sees half an
/// image (the prefetch thread runs while the main thread is loading).
fn write_disk(src: &Path, scale: u32, img: &TgaImage) {
    let Some(p) = disk_path(src, scale) else { return };
    let Some(dir) = p.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    // The prefetch thread and the main thread can decode the same file at the
    // same time, so the temporary name has to be unique per attempt, not per
    // process. The rename onto the final name is what makes the write atomic.
    static TICK: AtomicUsize = AtomicUsize::new(0);
    let tmp = p.with_extension(format!(
        "tga.{}.{}.tmp",
        std::process::id(),
        TICK.fetch_add(1, Ordering::Relaxed)
    ));
    if std::fs::write(&tmp, tga::encode(img)).is_ok() {
        let _ = std::fs::rename(&tmp, &p);
    }
    let _ = std::fs::remove_file(&tmp);
}

/// Load src at the scale the current window needs, going through the memory
/// cache, then the disk cache, and only then the WebP decoder.
pub fn load(src: &Path) -> Option<Arc<TgaImage>> {
    let (source, target) = image::art_scales(src);
    let key = Key { path: src.to_path_buf(), scale: target };
    {
        let mut m = match mem().lock() {
            Ok(m) => m,
            Err(e) => e.into_inner(),
        };
        m.clock += 1;
        let clock = m.clock;
        if let Some(e) = m.map.get_mut(&key) {
            e.used = clock;
            return Some(Arc::clone(&e.img));
        }
    }
    // Expected size of the decoded pixels, used to reject a stale cache file
    // whose source changed resolution under an unchanged mtime.
    let want = image::image_dimensions(src).map(|(w, h)| {
        (
            ((w as u64 * target as u64) / source.max(1) as u64).max(1) as u32,
            ((h as u64 * target as u64) / source.max(1) as u64).max(1) as u32,
        )
    });
    if let Some(img) = read_disk(src, target, want) {
        let img = Arc::new(img);
        insert(src, target, Arc::clone(&img));
        return Some(img);
    }
    let img = Arc::new(image::decode_art_file(src).ok()?);
    if worth_caching(src, target, source) {
        write_disk(src, target, &img);
    }
    insert(src, target, Arc::clone(&img));
    Some(img)
}

/// Drop every cached image decoded for a different render scale, so a window
/// resize cannot mix pixel scales. Images still on screen are pinned by the
/// caller and survive.
pub fn drop_other_scales(keep: u32) {
    let mut m = match mem().lock() {
        Ok(m) => m,
        Err(e) => e.into_inner(),
    };
    let stale: Vec<Key> = m.map.keys().filter(|k| k.scale != keep).cloned().collect();
    for k in stale {
        if let Some(e) = m.map.remove(&k) {
            m.bytes = m.bytes.saturating_sub(e.bytes);
        }
    }
}

// -------------------------------------------------------------- prefetch ----

static PENDING: AtomicUsize = AtomicUsize::new(0);

struct Worker {
    tx: Mutex<std::sync::mpsc::Sender<PathBuf>>,
}

fn worker() -> Option<&'static Worker> {
    static W: OnceLock<Option<Worker>> = OnceLock::new();
    W.get_or_init(|| {
        let (tx, rx) = std::sync::mpsc::channel::<PathBuf>();
        let ok = std::thread::Builder::new()
            .name("wa2-prefetch".into())
            .spawn(move || {
                let mut seen: std::collections::HashSet<PathBuf> = std::collections::HashSet::new();
                for path in rx.iter() {
                    if seen.insert(path.clone()) {
                        let _ = load(&path);
                    }
                    PENDING.fetch_sub(1, Ordering::Relaxed);
                }
            })
            .is_ok();
        ok.then(|| Worker { tx: Mutex::new(tx) })
    })
    .as_ref()
}

/// Decode paths on a background thread while the player reads. Paths that were
/// queued before, or that are already cached, are skipped by the worker.
pub fn prefetch(paths: Vec<PathBuf>) {
    if paths.is_empty() {
        return;
    }
    let Some(w) = worker() else { return };
    let Ok(tx) = w.tx.lock() else { return };
    for p in paths {
        if tx.send(p).is_ok() {
            PENDING.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// How many prefetch jobs have not finished yet.
pub fn pending() -> usize {
    PENDING.load(Ordering::Relaxed)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// With the data tree present, the cached result must be identical to a
    /// straight decode - the whole point of the cache is that it changes the
    /// cost, never the pixels.
    #[test]
    fn cached_load_matches_a_direct_decode() {
        let src = std::path::Path::new("data/sprite_8x/Sat018001.webp");
        if !src.is_file() {
            return; // no data tree on this machine
        }
        std::env::set_var("WA2_RENDER_SCALE", "1");
        let direct = image::decode_art_file(src).unwrap();
        let cached = load(src).expect("cache load");
        std::env::remove_var("WA2_RENDER_SCALE");
        assert_eq!(cached.width, direct.width);
        assert_eq!(cached.height, direct.height);
        assert_eq!(cached.rgba, direct.rgba);
    }

    #[test]
    fn disk_path_mirrors_the_source_tree() {
        let p = disk_path(Path::new("data/bg_8x/b100400.webp"), 2).unwrap();
        assert_eq!(p, PathBuf::from("data/cache/art/2x/bg_8x/b100400.tga"));
    }

    #[test]
    fn memory_cache_evicts_the_least_recently_used_image() {
        let dir = std::env::temp_dir().join(format!("wa2-artcache-{}", std::process::id()));
        std::fs::create_dir_all(&dir).ok();
        let one = dir.join("one.tga");
        let two = dir.join("two.tga");
        let three = dir.join("three.tga");
        std::fs::write(&one, b"x").unwrap();
        std::fs::write(&two, b"x").unwrap();
        std::fs::write(&three, b"x").unwrap();
        let px = |v: u8| TgaImage { width: 1, height: 1, rgba: vec![v; 4] };
        // Two 4-byte images fit, the third must push one out.
        mem().lock().unwrap().budget = 8;
        insert(&one, 1, Arc::new(px(1)));
        insert(&two, 1, Arc::new(px(2)));
        // Touch one so two is the least recently used entry.
        assert!(peek(&one).is_some());
        insert(&three, 1, Arc::new(px(3)));
        assert!(peek(&one).is_some(), "the recently used image must survive");
        assert!(peek(&two).is_none(), "the least recently used image must go");
        assert!(peek(&three).is_some());
        std::fs::remove_dir_all(&dir).ok();
    }
}
