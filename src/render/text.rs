//! System-font text rendering fallback.
//!
//! The shipped game font's Japanese glyph order is unknown, so we optionally
//! rasterize dialog text with a system CJK font (Noto Sans CJK) via an ffmpeg
//! `drawtext` filter chain and blit the resulting RGBA bitmap into the scene.
//! Each line is its own `drawtext` so we fully control line placement (ffmpeg's
//! in-text newline handling otherwise emits stray placeholder glyphs).

use std::io::Read;
use std::process::{Command, Stdio};

/// Default system font (Noto Sans CJK).
pub const DEFAULT_FONT: &str = "/usr/share/fonts/opentype/noto/NotoSansCJK-Regular.ttc";

/// Rasterize `lines` into a WxH RGBA bitmap (white text on transparent) using
/// ffmpeg drawtext with `font`. Returns (width, height, rgba) or None.
pub fn block_rgba(
    lines: &[String],
    w: u32,
    h: u32,
    fontsize: u32,
    fontfile: &str,
) -> Option<(u32, u32, Vec<u8>)> {
    if lines.is_empty() {
        return Some((w, h, vec![0u8; (w * h * 4) as usize]));
    }
    let lh = fontsize + 8;
    let mut filter = String::new();
    let mut files = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        let path = std::env::temp_dir().join(format!("wa2_txt_{}.txt", i));
        let path = path.to_string_lossy().into_owned();
        if std::fs::write(&path, line.as_bytes()).is_err() {
            return None;
        }
        if i > 0 {
            filter.push(',');
        }
        filter.push_str(&format!(
            "drawtext=fontfile={}:textfile={}:fontsize={}:fontcolor=white:x=0:y={}",
            fontfile, path, fontsize, i as u32 * lh
        ));
        files.push(path);
    }
    let out = Command::new("ffmpeg")
        .args([
            "-hide_banner", "-loglevel", "error",
            "-f", "lavfi", "-i", &format!("color=c=black@0.0:s={}x{}", w, h),
            "-vf", &filter,
            "-frames:v", "1",
            "-f", "rawvideo", "-pix_fmt", "rgba", "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let need = (w * h * 4) as usize;
    let mut buf = out.stdout;
    if buf.len() < need {
        buf.resize(need, 0);
    }
    buf.truncate(need);
    Some((w, h, buf))
}