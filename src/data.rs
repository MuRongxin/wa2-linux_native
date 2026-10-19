//! Data-tree layout for the detached build.
//!
//! Every piece of game content lives as a loose file under one root directory
//! (default `./data`, override with `$WA2_DATA`). The engine never opens a
//! `.pak` archive and never needs the original installation.
//!
//! ```text
//! data/
//!   script/     *.bnr + *.txt    LSCR bytecode + token pool        (Japanese)
//!   script_cn/  *.bnr + *.txt    same, CK-GAL Chinese patch text
//!   bg/         *.tga, *.amp     backgrounds and tint LUTs
//!   sprite/     *.tga            standing character sprites
//!   bg_8x/      *.webp           optional high-resolution art sibling
//!   sprite_8x/  *.webp           optional high-resolution sprite sibling
//!   voice/      *.ogg            voice
//!   bgm/        *.ogg            music
//!   movie/      *.wmv            movies (raw ASF, renamed from mv*.pak)
//!   font/       *_body80.tga     glyph atlas
//!               wa2_cell_map_*.tsv  char -> atlas cell table
//! ```
//!
//! `WA2_LANG=cn` switches the script directory and the font pair to the Chinese
//! patch (`script_cn/`, `cn_body80.tga` + `wa2_cell_map_cn.tsv`).
//! `WA2_ART_SCALE=8` selects the corresponding `*_8x/` art siblings.

use std::path::{Path, PathBuf};

#[derive(Clone)]
pub struct Data {
    pub root: PathBuf,
    /// Chinese patch data (script_cn / cn_body80 / cell_map_cn).
    pub cn: bool,
    /// Art scale selected by `WA2_ART_SCALE` (1 keeps the original TGA tree).
    pub art_scale: u32,
}

impl Data {
    /// Locate the data root: `$WA2_DATA`, else `./data`.
    ///
    /// Returns `None` (with a hint on stderr) when the tree is missing, so the
    /// CLI can print a useful message instead of failing halfway through a scene.
    pub fn resolve() -> Option<Data> {
        let root = match std::env::var_os("WA2_DATA") {
            Some(p) => PathBuf::from(p),
            None => PathBuf::from("data"),
        };
        if !root.join("script").is_dir() {
            eprintln!(
                "error: data tree not found at {} (expected {}/script)\n\
                 hint: set WA2_DATA=/path/to/data, or run tools/build_data_tree.py",
                root.display(),
                root.display()
            );
            return None;
        }
        // Language selection:
        //   WA2_LANG=cn|zh|chs  -> Chinese patch data (only if script_cn exists)
        //   WA2_LANG=jp|ja      -> Japanese
        //   unset               -> Chinese when the tree ships script_cn (the
        //                          integrated edition this port targets is the
        //                          CK-GAL Chinese patch), else Japanese.
        let want = std::env::var("WA2_LANG").unwrap_or_default().to_ascii_lowercase();
        let has_cn = root.join("script_cn").is_dir();
        let cn = match want.as_str() {
            "cn" | "zh" | "chs" | "zh-cn" => {
                if !has_cn {
                    eprintln!("error: WA2_LANG={} but {} has no script_cn/", want, root.display());
                    return None;
                }
                true
            }
            "jp" | "ja" | "jpn" => false,
            "" => has_cn,
            other => {
                eprintln!("error: unknown WA2_LANG={:?} (use cn or jp)", other);
                return None;
            }
        };
        let requested_scale = std::env::var("WA2_ART_SCALE")
            .ok()
            .and_then(|v| v.parse::<u32>().ok())
            .filter(|&v| matches!(v, 1 | 2 | 4 | 8))
            // The 8x WebP trees are the default source. A checkout that only
            // has the original TGA tree falls back to 1x below, so running from
            // the bare binary behaves the same as ./run.sh.
            .unwrap_or(8);
        let art_scale = if requested_scale > 1 {
            let suffix = format!("{}x", requested_scale);
            let ready = root.join(format!("bg_{}", suffix)).is_dir()
                && root.join(format!("sprite_{}", suffix)).is_dir()
                && (!cn || root.join(format!("bg_cn_{}", suffix)).is_dir());
            if ready { requested_scale } else {
                eprintln!("art: WA2_ART_SCALE={} requested but sibling tree is incomplete; using original art", requested_scale);
                1
            }
        } else {
            1
        };
        if art_scale > 1 {
            // Publish the effective source scale so diagnostics (the startup
            // "art: source Nx, decode Mx" line) see the same value the loader
            // uses. The fallback below already pins the variable to 1.
            std::env::set_var("WA2_ART_SCALE", art_scale.to_string());
            eprintln!("art: using {}x sibling directories when present", art_scale);
        } else if requested_scale > 1 {
            // The renderer reads the effective scale for layout decisions too;
            // do not let a partial sibling tree make original TGA look like 8x
            // art after the data resolver has fallen back.
            std::env::set_var("WA2_ART_SCALE", "1");
        }
        Some(Data { root, cn, art_scale })
    }

    fn sub(&self, name: &str) -> PathBuf {
        if self.art_scale > 1 {
            let scaled = self.root.join(format!("{}_{}x", name, self.art_scale));
            if scaled.is_dir() {
                return scaled;
            }
        }
        self.root.join(name)
    }

    pub fn script(&self) -> PathBuf {
        if self.cn { self.sub("script_cn") } else { self.sub("script") }
    }
    pub fn bg(&self) -> PathBuf {
        self.sub("bg")
    }
    pub fn sprite(&self) -> PathBuf {
        self.sub("sprite")
    }
    pub fn bg_cn(&self) -> PathBuf {
        self.sub("bg_cn")
    }
    pub fn voice(&self) -> PathBuf {
        self.sub("voice")
    }
    pub fn bgm(&self) -> PathBuf {
        self.sub("bgm")
    }
    pub fn movie(&self) -> PathBuf {
        // Video remasters live in their own sibling tree (currently
        // `movie_2x/` for the 1440p pass).  Videos are always played at their
        // own resolution, so prefer the highest available video tree
        // regardless of `WA2_ART_SCALE`, then fall back to the original files.
        for scale in [8u32, 4, 2] {
            let candidate = self.root.join(format!("movie_{}x", scale));
            if candidate.is_dir() {
                return candidate;
            }
        }
        self.sub("movie")
    }

    /// Glyph atlas for the selected language.
    pub fn atlas(&self) -> PathBuf {
        self.sub("font").join(if self.cn { "cn_body80.tga" } else { "jp_body80.tga" })
    }

    /// Authoritative char -> atlas cell table (from the exe font table).
    pub fn cell_map(&self) -> PathBuf {
        self.sub("font")
            .join(if self.cn { "wa2_cell_map_cn.tsv" } else { "wa2_cell_map_jp.tsv" })
    }

    /// One-line description for startup logging.
    pub fn describe(&self) -> String {
        format!(
            "{} (lang={})",
            self.root.display(),
            if self.cn { "cn" } else { "jp" }
        )
    }

    /// Path of the `char.pak`-era legacy layout, if the caller still passed one.
    pub fn from_explicit(root: &Path) -> Data {
        Data { root: root.to_path_buf(), cn: false, art_scale: 1 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_root(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!("wa2-data-{}-{}", tag, std::process::id()))
    }

    #[test]
    fn movie_uses_scaled_sibling_when_present() {
        let root = temp_root("movie-scaled");
        std::fs::create_dir_all(root.join("movie_8x")).unwrap();
        let data = Data { root: root.clone(), cn: false, art_scale: 8 };
        assert_eq!(data.movie(), root.join("movie_8x"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn movie_prefers_video_remaster_over_art_scale() {
        let root = temp_root("movie-remaster");
        std::fs::create_dir_all(root.join("movie_2x")).unwrap();
        std::fs::create_dir_all(root.join("movie")).unwrap();
        // Even with WA2_ART_SCALE=1 the video remaster must win: videos carry
        // their own resolution and are not downsampled by the render scale.
        let data = Data { root: root.clone(), cn: false, art_scale: 1 };
        assert_eq!(data.movie(), root.join("movie_2x"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn movie_prefers_highest_video_tree() {
        let root = temp_root("movie-highest");
        std::fs::create_dir_all(root.join("movie_2x")).unwrap();
        std::fs::create_dir_all(root.join("movie_8x")).unwrap();
        let data = Data { root: root.clone(), cn: false, art_scale: 8 };
        assert_eq!(data.movie(), root.join("movie_8x"));
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn movie_falls_back_to_original_tree() {
        let root = temp_root("movie-original");
        std::fs::create_dir_all(root.join("movie")).unwrap();
        let data = Data { root: root.clone(), cn: false, art_scale: 8 };
        assert_eq!(data.movie(), root.join("movie"));
        std::fs::remove_dir_all(&root).ok();
    }
}
