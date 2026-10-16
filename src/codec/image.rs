//! Runtime image loading for game art.
//!
//! TGA is decoded in-process.  Upscaled art is lossless WebP, which is decoded
//! through the ffmpeg executable already required by the engine for movies and
//! audio.  ffmpeg writes a PAM RGBA frame to stdout; keeping this boundary in
//! one module means the renderer does not care which asset format was used.

use super::tga::{self, TgaImage};
use std::path::Path;
use std::process::Command;

/// Candidate names for a logical asset name such as `sys_00001.tga`.
/// Upscaled trees keep the stem and switch the extension to WebP.
pub fn candidate_names(name: &str) -> Vec<String> {
    let base = name
        .trim_end_matches(".tga")
        .trim_end_matches(".TGA")
        .trim_end_matches(".webp")
        .trim_end_matches(".WEBP");
    vec![
        format!("{}.webp", base),
        format!("{}.WEBP", base),
        format!("{}.tga", base),
        format!("{}.TGA", base),
    ]
}

pub fn decode_file(path: &Path) -> Result<TgaImage, String> {
    let ext = path.extension().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "tga" => {
            let data = std::fs::read(path).map_err(|e| format!("{}: {}", path.display(), e))?;
            tga::decode(&data)
        }
        "webp" => decode_webp(path),
        _ => Err(format!("unsupported image extension: {}", path.display())),
    }
}

fn decode_webp(path: &Path) -> Result<TgaImage, String> {
    let output = Command::new("ffmpeg")
        .args(["-v", "error", "-i"])
        .arg(path)
        .args(["-frames:v", "1", "-f", "image2pipe", "-c:v", "pam", "-pix_fmt", "rgba", "-"])
        .output()
        .map_err(|e| format!("ffmpeg WebP decode {}: {}", path.display(), e))?;
    if !output.status.success() {
        let detail = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(format!(
            "ffmpeg WebP decode {} failed ({}): {}",
            path.display(),
            output.status,
            if detail.is_empty() { "no ffmpeg diagnostic" } else { &detail }
        ));
    }
    parse_pam_rgba(&output.stdout).map_err(|e| format!("{}: {}", path.display(), e))
}

fn parse_pam_rgba(data: &[u8]) -> Result<TgaImage, String> {
    if !data.starts_with(b"P7\n") {
        return Err("ffmpeg did not return a PAM image".into());
    }
    let header_end = data
        .windows(7)
        .position(|w| w == b"ENDHDR\n")
        .ok_or_else(|| "PAM header truncated".to_string())?
        + 7;
    let header = std::str::from_utf8(&data[..header_end]).map_err(|_| "PAM header is not UTF-8")?;
    let mut width = None;
    let mut height = None;
    let mut depth = None;
    let mut maxval = None;
    for line in header.lines() {
        let mut it = line.split_whitespace();
        match it.next() {
            Some("WIDTH") => width = it.next().and_then(|v| v.parse::<u32>().ok()),
            Some("HEIGHT") => height = it.next().and_then(|v| v.parse::<u32>().ok()),
            Some("DEPTH") => depth = it.next().and_then(|v| v.parse::<u32>().ok()),
            Some("MAXVAL") => maxval = it.next().and_then(|v| v.parse::<u32>().ok()),
            _ => {}
        }
    }
    let width = width.ok_or_else(|| "PAM width missing".to_string())?;
    let height = height.ok_or_else(|| "PAM height missing".to_string())?;
    if depth != Some(4) || maxval != Some(255) {
        return Err(format!("unsupported PAM format depth={depth:?} maxval={maxval:?}"));
    }
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|n| n.checked_mul(4))
        .ok_or_else(|| "PAM dimensions overflow".to_string())?;
    let pixels = data.get(header_end..header_end + bytes).ok_or_else(|| {
        format!("PAM pixel payload truncated ({} < {})", data.len().saturating_sub(header_end), bytes)
    })?;
    Ok(TgaImage { width, height, rgba: pixels.to_vec() })
}

#[cfg(test)]
mod tests {
    #[test]
    fn parses_rgba_pam() {
        let mut data = b"P7\nWIDTH 2\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n".to_vec();
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let img = super::parse_pam_rgba(&data).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(img.rgba, &[1, 2, 3, 4, 5, 6, 7, 8]);
    }
}

