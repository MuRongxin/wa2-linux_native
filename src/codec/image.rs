//! Runtime image loading for game art.
//!
//! TGA is decoded in-process.  Upscaled art is lossless WebP, which is decoded
//! through the ffmpeg executable already required by the engine for movies and
//! audio.  ffmpeg writes a PAM RGBA frame to stdout; keeping this boundary in
//! one module means the renderer does not care which asset format was used.

use super::tga::{self, TgaImage};
use std::path::Path;
use std::process::Command;

pub fn is_hires_dir(dir: &Path) -> bool {
    dir_art_scale(dir) > 1
}

/// Art scale implied by a `*_2x`, `*_4x` or `*_8x` sibling directory.
/// Used to reduce pixel-aligned assets (UI sheets, 8x screenshots) back to
/// logical 1280x720 coordinates no matter which sibling tree supplied them.
pub fn dir_art_scale(path: &Path) -> u32 {
    let dir = path.parent().unwrap_or(path);
    let name = dir
        .file_name()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase())
        .unwrap_or_default();
    for (suffix, scale) in [("_8x", 8u32), ("_4x", 4), ("_2x", 2)] {
        if name.ends_with(suffix) {
            return scale;
        }
    }
    1
}

/// UI sheets are authored in logical coordinates. Their scaled siblings
/// (`*_2x`, `*_4x`, `*_8x`) must still be used, but are reduced at load time
/// before pixel-aligned UI code consumes them.
pub fn decode_ui_file(path: &Path) -> Result<TgaImage, String> {
    if path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("webp")) == Some(true)
        && path.parent().map(is_hires_dir).unwrap_or(false)
    {
        return decode_webp_scaled(path, dir_art_scale(path));
    }
    let img = decode_file(path)?;
    let scale = dir_art_scale(path);
    if scale <= 1 || img.height == 0 || img.width < scale {
        return Ok(img);
    }
    let dw = (img.width / scale).max(1);
    let dh = (img.height / scale).max(1);
    let mut rgba = vec![0u8; (dw * dh * 4) as usize];
    for y in 0..dh {
        for x in 0..dw {
            let sx = ((x as u64 * img.width as u64) / dw as u64) as u32;
            let sy = ((y as u64 * img.height as u64) / dh as u64) as u32;
            let si = ((sy.min(img.height - 1) * img.width + sx.min(img.width - 1)) * 4) as usize;
            let di = ((y * dw + x) * 4) as usize;
            rgba[di..di + 4].copy_from_slice(&img.rgba[si..si + 4]);
        }
    }
    Ok(TgaImage { width: dw, height: dh, rgba })
}

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

/// Decode an art asset from a scaled sibling tree at the resolution needed by
/// the current window. The source file can remain 8x while the decoded pixel
/// buffer is only 1x/2x/4x/8x according to `WA2_RENDER_SCALE`.
pub fn decode_art_file(path: &Path) -> Result<TgaImage, String> {
    let source = dir_art_scale(path);
    if source <= 1 || !path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("webp")).unwrap_or(false) {
        return decode_file(path);
    }
    let requested = std::env::var("WA2_RENDER_SCALE")
        .ok().and_then(|v| v.parse::<u32>().ok()).filter(|&v| v >= 1).unwrap_or(source);
    let target = requested.min(source);
    if target >= source {
        return decode_file(path);
    }
    decode_webp_ratio(path, target, source)
}

fn decode_webp(path: &Path) -> Result<TgaImage, String> {
    decode_webp_filter(path, None)
}

fn decode_webp_scaled(path: &Path, factor: u32) -> Result<TgaImage, String> {
    decode_webp_filter(path, Some(format!("scale=iw/{factor}:ih/{factor}")))
}

fn decode_webp_ratio(path: &Path, target: u32, source: u32) -> Result<TgaImage, String> {
    decode_webp_filter(path, Some(format!("scale=iw*{target}/{source}:ih*{target}/{source}")))
}

fn decode_webp_filter(path: &Path, filter: Option<String>) -> Result<TgaImage, String> {
    let mut command = Command::new("ffmpeg");
    command.args(["-v", "error", "-i"])
        .arg(path);
    if let Some(filter) = filter {
        command.arg("-vf").arg(filter);
    }
    let output = command
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
    fn scaled_dir_names_report_their_art_scale() {
        use std::path::PathBuf;
        let cases = [
            ("/data/bg_2x/panel.tga", 2u32),
            ("/data/sprite_4x/a.tga", 4),
            ("/data/bg_8x/sys_0001.tga", 8),
            ("/data/bg/panel.tga", 1),
        ];
        for (path, scale) in cases {
            assert_eq!(super::dir_art_scale(&PathBuf::from(path)), scale, "{path}");
        }
    }

    #[test]
    fn decode_ui_file_reduces_scaled_sheets_to_logical_size() {
        use std::io::Write;
        use std::path::PathBuf;
        // 4x4 TGA with a 32-bit descriptor, written into a `bg_2x` sibling dir.
        let dir = std::env::temp_dir().join(format!("wa2-ui-scale-test-{}", std::process::id()));
        let scaled = dir.join("bg_2x");
        std::fs::create_dir_all(&scaled).unwrap();
        let path = scaled.join("panel.tga");
        let mut header = vec![0u8; 18];
        header[2] = 2; // uncompressed true-color
        header[12..14].copy_from_slice(&4u16.to_le_bytes());
        header[14..16].copy_from_slice(&4u16.to_le_bytes());
        header[16] = 32;
        header[17] = 8; // 8 alpha bits
        let mut file = std::fs::File::create(&path).unwrap();
        file.write_all(&header).unwrap();
        file.write_all(&[0x40u8; 4 * 4 * 4]).unwrap();
        drop(file);
        let img = super::decode_ui_file(&path).unwrap();
        assert_eq!((img.width, img.height), (2, 2));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn parses_rgba_pam() {
        let mut data = b"P7\nWIDTH 2\nHEIGHT 1\nDEPTH 4\nMAXVAL 255\nTUPLTYPE RGB_ALPHA\nENDHDR\n".to_vec();
        data.extend_from_slice(&[1, 2, 3, 4, 5, 6, 7, 8]);
        let img = super::parse_pam_rgba(&data).unwrap();
        assert_eq!((img.width, img.height), (2, 1));
        assert_eq!(img.rgba, &[1, 2, 3, 4, 5, 6, 7, 8]);
    }
}

