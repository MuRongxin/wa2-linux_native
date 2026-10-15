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

/// One label placed on a shared canvas. `x,y,w,h` is the box it sits in.
/// `center` puts the line in the middle of that box; otherwise it is left
/// aligned with a small pad and vertically centered.
pub struct Stamp {
    pub text: String,
    pub x: i32,
    pub y: i32,
    pub w: i32,
    pub h: i32,
    pub size: u32,
    pub center: bool,
}

/// Rasterize every stamp in one pass. White glyphs, alpha = coverage.
/// An empty list is a clear canvas.
pub fn stamps_rgba(w: u32, h: u32, stamps: &[Stamp]) -> Option<(u32, u32, Vec<u8>)> {
    if w == 0 || h == 0 {
        return None;
    }
    let mut buf = vec![0u8; (w as usize) * (h as usize) * 4];
    if stamps.is_empty() {
        return Some((w, h, buf));
    }
    let mut filter = String::new();
    let mut files = Vec::new();
    for (i, s) in stamps.iter().enumerate() {
        if s.text.is_empty() || s.size == 0 {
            continue;
        }
        let path = std::env::temp_dir().join(format!("wa2_stamp_{}.txt", i));
        let path = path.to_string_lossy().into_owned();
        if std::fs::write(&path, s.text.as_bytes()).is_err() {
            return None;
        }
        if !filter.is_empty() {
            filter.push(',');
        }
        let x = if s.center {
            format!("{}+({}-text_w)/2", s.x, s.w)
        } else {
            format!("{}", s.x + 4)
        };
        let y = format!("{}+({}-text_h)/2", s.y, s.h);
        filter.push_str(&format!(
            "drawtext=fontfile={}:textfile={}:fontsize={}:fontcolor=white:x={}:y={}",
            DEFAULT_FONT, path, s.size, x, y
        ));
        files.push(path);
    }
    if filter.is_empty() {
        return Some((w, h, buf));
    }
    let out = Command::new("ffmpeg")
        .args([
            "-hide_banner",
            "-loglevel",
            "error",
            "-f",
            "lavfi",
            "-i",
            &format!("color=c=black@0.0:s={}x{}", w, h),
            "-vf",
            &filter,
            "-frames:v",
            "1",
            "-f",
            "rawvideo",
            "-pix_fmt",
            "rgba",
            "-",
        ])
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .output()
        .ok()?;
    let need = buf.len();
    buf = out.stdout;
    if buf.len() < need {
        buf.resize(need, 0);
    }
    buf.truncate(need);
    for px in buf.chunks_exact_mut(4) {
        let lum = px[0].max(px[1]).max(px[2]);
        px[0] = 255;
        px[1] = 255;
        px[2] = 255;
        px[3] = lum;
    }
    let _ = files;
    Some((w, h, buf))
}

/// One line of system-font text, sized to the string. White on transparent.
pub fn line_rgba(text: &str, fontsize: u32) -> Option<(u32, u32, Vec<u8>)> {
    if text.is_empty() {
        return None;
    }
    let w = (text.chars().count() as u32)
        .saturating_mul(fontsize)
        .saturating_add(24)
        .max(8);
    let h = fontsize.saturating_add(16).max(8);
    let path = std::env::temp_dir().join("wa2_line.txt");
    let path = path.to_string_lossy().into_owned();
    if std::fs::write(&path, text.as_bytes()).is_err() {
        return None;
    }
    let filter = format!(
        "drawtext=fontfile={}:textfile={}:fontsize={}:fontcolor=white:x=4:y=2",
        DEFAULT_FONT, path, fontsize
    );
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
    // The transparent plate comes back as black. Coverage is the luminance,
    // so the glyphs blend onto the dialog instead of sitting in a black box.
    for px in buf.chunks_exact_mut(4) {
        let lum = px[0].max(px[1]).max(px[2]);
        px[0] = 255;
        px[1] = 255;
        px[2] = 255;
        px[3] = lum;
    }
    Some((w, h, buf))
}