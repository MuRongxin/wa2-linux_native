//! Runtime image loading for game art.
//!
//! TGA is decoded in-process.  Upscaled art is lossless WebP, which is decoded
//! through the ffmpeg executable already required by the engine for movies and
//! audio.  ffmpeg writes a PAM RGBA frame to stdout; keeping this boundary in
//! one module means the renderer does not care which asset format was used.

use super::tga::{self, TgaImage};
use std::io::Read;
use std::path::Path;
use std::process::Command;
use std::sync::atomic::{AtomicU64, Ordering};

/// Art scale implied by a `*_2x`, `*_4x` or `*_8x` sibling directory.
/// Used to reduce pixel-aligned assets (UI sheets, 8x screenshots) back to
/// logical 1280x720 coordinates no matter which sibling tree supplied them.
pub fn dir_art_scale(path: &Path) -> u32 {
    // Callers pass either an asset path or the sibling directory itself.
    // Inspect the final component first so this also works before a directory
    // has been created; `Path::parent()` on `data/bg_8x` would erase the scale.
    let direct_name = path.file_name().and_then(|s| s.to_str()).unwrap_or("").to_ascii_lowercase();
    let dir = if ["_8x", "_4x", "_2x"].iter().any(|suffix| direct_name.ends_with(suffix)) {
        path
    } else {
        path.parent().unwrap_or(path)
    };
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

/// Width and height straight from the file header - never decodes pixels.
pub fn image_dimensions(path: &Path) -> Option<(u32, u32)> {
    let ext = path.extension()?.to_str()?.to_ascii_lowercase();
    if ext == "tga" {
        let mut file = std::fs::File::open(path).ok()?;
        let mut bytes = [0u8; 16];
        file.read_exact(&mut bytes).ok()?;
        if bytes.len() < 16 { return None; }
        return Some((u16::from_le_bytes([bytes[12], bytes[13]]) as u32,
            u16::from_le_bytes([bytes[14], bytes[15]]) as u32));
    }
    if ext != "webp" { return None; }
    // All supported WebP dimension fields are in the RIFF header or the
    // first few bytes of the VP8/VP8L payload. Never read the full image just
    // to decide which scale to request from the decoder.
    let file = std::fs::File::open(path).ok()?;
    let mut bytes = Vec::with_capacity(64);
    file.take(64).read_to_end(&mut bytes).ok()?;
    if bytes.len() < 16 || &bytes[0..4] != b"RIFF" || &bytes[8..12] != b"WEBP" { return None; }
    match &bytes[12..16] {
        b"VP8X" if bytes.len() >= 30 => {
            let w = 1 + (bytes[24] as u32 | ((bytes[25] as u32) << 8) | ((bytes[26] as u32) << 16));
            let h = 1 + (bytes[27] as u32 | ((bytes[28] as u32) << 8) | ((bytes[29] as u32) << 16));
            Some((w, h))
        }
        b"VP8 " if bytes.len() >= 34 => {
            let p = bytes.windows(4).position(|w| w == [0x9d, 0x01, 0x2a])? + 4;
            if p + 4 > bytes.len() { return None; }
            Some((u16::from_le_bytes([bytes[p], bytes[p + 1]]) as u32 & 0x3fff,
                u16::from_le_bytes([bytes[p + 2], bytes[p + 3]]) as u32 & 0x3fff))
        }
        b"VP8L" if bytes.len() >= 25 && bytes[20] == 0x2f => {
            // VP8L stores width-1 and height-1 as adjacent 14-bit fields
            // immediately after its 0x2f signature byte.
            let bits = u32::from_le_bytes([bytes[21], bytes[22], bytes[23], bytes[24]]);
            Some(((bits & 0x3fff) + 1, ((bits >> 14) & 0x3fff) + 1))
        }
        _ => None,
    }
}

/// Use the actual dimensions when a supposedly 8x directory contains a 4x
/// or 2x file. This handles partially regenerated asset trees.
pub fn source_art_scale(path: &Path) -> u32 {
    let declared = dir_art_scale(path);
    if declared <= 1 { return 1; }
    let Some(actual) = image_dimensions(path) else { return declared; };
    let dir = path.parent().unwrap_or(path);
    let name = dir.file_name().and_then(|s| s.to_str()).unwrap_or("");
    let base_dir = name.strip_suffix("_8x").or_else(|| name.strip_suffix("_4x")).or_else(|| name.strip_suffix("_2x"));
    let Some(base_dir) = base_dir else { return declared; };
    let Some(stem) = path.file_stem() else { return declared; };
    let original = dir.parent().unwrap_or(dir).join(base_dir).join(stem);
    let original = ["tga", "webp", "TGA", "WEBP", "bmp", "BMP"]
        .iter()
        .map(|ext| original.with_extension(ext))
        .find(|p| p.is_file());
    let Some(original) = original else { return declared; };
    let Some(base) = image_dimensions(&original) else { return declared; };
    let sx = actual.0 as f64 / base.0.max(1) as f64;
    let sy = actual.1 as f64 / base.1.max(1) as f64;
    if (sx - sy).abs() > 0.08 { return declared; }
    [8, 4, 2, 1].into_iter().min_by(|a, b| {
        ((sx - *a as f64).abs()).partial_cmp(&(sx - *b as f64).abs()).unwrap()
    }).unwrap_or(declared)
}

/// UI sheets are authored in logical coordinates. Their scaled siblings
/// (`*_2x`, `*_4x`, `*_8x`) must still be used, but are reduced at load time
/// before pixel-aligned UI code consumes them.
/// Logical-size UI cache written by `tools/build_ui_cache.py`.
///
/// UI sheets are always consumed at their 1280x720 logical size, so decoding
/// a 59 MP 8x WebP only to throw away 63/64 of the pixels is pure waste. The
/// tool pre-renders the logical TGA once; the loader prefers it and falls
/// back to decoding the 8x WebP whenever the cache file is absent.
fn ui_cache_path(path: &Path) -> Option<std::path::PathBuf> {
    if path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("webp")) != Some(true) {
        return None;
    }
    let parent = path.parent()?;
    let tree = parent.file_name()?.to_str()?.to_ascii_lowercase();
    if !(tree.ends_with("_2x") || tree.ends_with("_4x") || tree.ends_with("_8x")) {
        return None;
    }
    let root = parent.parent()?;
    let stem = path.file_stem()?.to_string_lossy();
    Some(root.join("cache").join("ui").join(tree).join(format!("{stem}.tga")))
}

pub fn decode_ui_file(path: &Path) -> Result<TgaImage, String> {
    // Probing by decoding was expensive: a missing .webp candidate used to
    // spawn dwebp and then ffmpeg before failing, which cost tens of ms per
    // probe and dominated the startup cost of every UI sheet.
    if !path.is_file() {
        return Err(format!("missing {}", path.display()));
    }
    if let Some(cache) = ui_cache_path(path) {
        if let Ok(img) = decode_file(&cache) {
            return Ok(img);
        }
    }
    let scale = source_art_scale(path);
    if path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("webp")) == Some(true)
        && scale > 1
    {
        return decode_webp_scaled(path, scale);
    }
    let img = decode_file(path)?;
    Ok(downsample_image(img, 1, scale))
}

/// Candidate names for a logical asset name such as `sys_00001.tga`.
/// Upscaled trees keep the stem and switch the extension to WebP.
pub fn candidate_names(name: &str) -> Vec<String> {
    let base = name
        .trim_end_matches(".tga")
        .trim_end_matches(".TGA")
        .trim_end_matches(".webp")
        .trim_end_matches(".WEBP")
        .trim_end_matches(".bmp")
        .trim_end_matches(".BMP");
    vec![
        format!("{}.webp", base),
        format!("{}.WEBP", base),
        format!("{}.tga", base),
        format!("{}.TGA", base),
    ]
}

pub fn decode_file(path: &Path) -> Result<TgaImage, String> {
    if !path.is_file() {
        return Err(format!("missing {}", path.display()));
    }
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

/// The scale a file is stored at, and the scale decode_art_file will decode it
/// to for the current window. artcache keys every cache entry by the second
/// value, so a window resize can never mix pixel scales.
pub fn art_scales(path: &Path) -> (u32, u32) {
    let source = source_art_scale(path);
    // Decode target. WA2_RENDER_SCALE wins when set (a number, or the value
    // `set_render_art_scale` writes in `auto` mode). Unset means "decode at the
    // source tree's own scale": an 8x WebP decodes at 8x and the compositor
    // box-filters it down to the window, so every window gets full-resolution
    // source pixels. WA2_RENDER_SCALE=auto brings back the old window-sized
    // decoding for small machines.
    let requested = std::env::var("WA2_RENDER_SCALE")
        .ok().and_then(|v| v.trim().parse::<u32>().ok()).filter(|&v| v >= 1)
        .or_else(|| {
            std::env::var("WA2_ART_SCALE")
                .ok().and_then(|v| v.trim().parse::<u32>().ok()).filter(|&v| v >= 1)
        })
        .unwrap_or(1);
    (source, requested.min(source))
}

/// Decode an art asset from a scaled sibling tree at the resolution needed by
/// the current window. The source file can remain 8x while the decoded pixel
/// buffer is only 1x/2x/4x/8x according to WA2_RENDER_SCALE.
///
/// Callers that can reuse decoded pixels should go through artcache::load
/// instead: this is the slow path (a 59 MP lossless WebP costs a quarter of a
/// second to over a second) and is only for cache misses.
pub fn decode_art_file(path: &Path) -> Result<TgaImage, String> {
    let (source, target) = art_scales(path);
    if source <= 1 {
        return decode_file(path);
    }
    if target >= source {
        return decode_file(path);
    }
    if path.extension().and_then(|s| s.to_str()).map(|s| s.eq_ignore_ascii_case("webp")).unwrap_or(false) {
        return decode_webp_ratio(path, target, source);
    }
    // A scaled tree may also contain TGA proxy files. Decode them normally,
    // then reduce the pixel buffer before handing it to the renderer.
    Ok(downsample_image(decode_file(path)?, target, source))
}

/// Reduce a decoded image from `source_scale` to `target_scale` using the
/// same nearest-neighbour rule as the pixel-aligned UI sheets. Keeping this
/// after format decoding also covers high-resolution TGA proxy trees.
fn downsample_image(img: TgaImage, target_scale: u32, source_scale: u32) -> TgaImage {
    if target_scale == 0 || source_scale <= target_scale || img.width == 0 || img.height == 0 {
        return img;
    }
    let dw = ((img.width as u64 * target_scale as u64) / source_scale as u64).max(1) as u32;
    let dh = ((img.height as u64 * target_scale as u64) / source_scale as u64).max(1) as u32;
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
    TgaImage { width: dw, height: dh, rgba }
}

/// Decode and optionally resize WebP with `dwebp` (libwebp's tool).
///
/// Lossless WebP is the bulk of the asset tree and libwebp decodes it about
/// three times faster than the ffmpeg WebP decoder (1.1s vs 4.1s for a 59 MP
/// background). `-resize` also resamples inside the decoder, so a UI sheet or
/// a window-sized background never materializes at its full 8x size. ffmpeg
/// stays as a fallback for systems without libwebp's tools.
fn decode_webp_dwebp(path: &Path, resize: Option<(u32, u32)>) -> Result<TgaImage, String> {
    static COUNTER: AtomicU64 = AtomicU64::new(0);
    let token = COUNTER.fetch_add(1, Ordering::Relaxed);
    let temp = std::env::temp_dir().join(format!("wa2-webp-{}-{}.pam", std::process::id(), token));
    let mut command = Command::new("dwebp");
    command.args(["-quiet", "-pam"]);
    if let Some((width, height)) = resize {
        command
            .arg("-resize")
            .arg(width.max(1).to_string())
            .arg(height.max(1).to_string());
    }
    command.arg(path).arg("-o").arg(&temp);
    let output = command
        .output()
        .map_err(|e| format!("dwebp spawn {}: {}", path.display(), e))?;
    if !output.status.success() {
        let _ = std::fs::remove_file(&temp);
        return Err(format!("dwebp {} failed ({})", path.display(), output.status));
    }
    let bytes = std::fs::read(&temp).map_err(|e| format!("read {}: {}", temp.display(), e));
    let _ = std::fs::remove_file(&temp);
    parse_pam_rgba(&bytes?).map_err(|e| format!("{}: {}", path.display(), e))
}

fn decode_webp(path: &Path) -> Result<TgaImage, String> {
    match decode_webp_dwebp(path, None) {
        Ok(img) => Ok(img),
        Err(dwebp_error) => {
            decode_webp_filter(path, None).map_err(|ffmpeg_error| format!("{ffmpeg_error} (dwebp: {dwebp_error})"))
        }
    }
}

fn decode_webp_scaled(path: &Path, factor: u32) -> Result<TgaImage, String> {
    let factor = factor.max(1);
    if let Some((width, height)) = image_dimensions(path) {
        let target = ((width / factor).max(1), (height / factor).max(1));
        return match decode_webp_dwebp(path, Some(target)) {
            Ok(img) => Ok(img),
            Err(dwebp_error) => decode_webp_filter(
                path,
                Some(format!("scale=iw/{factor}:ih/{factor}")),
            )
            .map_err(|ffmpeg_error| format!("{ffmpeg_error} (dwebp: {dwebp_error})")),
        };
    }
    decode_webp_filter(path, Some(format!("scale=iw/{factor}:ih/{factor}")))
}

fn decode_webp_ratio(path: &Path, target: u32, source: u32) -> Result<TgaImage, String> {
    if let Some((width, height)) = image_dimensions(path) {
        let out = (
            ((width as u64 * target as u64) / source as u64).max(1) as u32,
            ((height as u64 * target as u64) / source as u64).max(1) as u32,
        );
        return match decode_webp_dwebp(path, Some(out)) {
            Ok(img) => Ok(img),
            Err(dwebp_error) => decode_webp_filter(
                path,
                Some(format!("scale=iw*{target}/{source}:ih*{target}/{source}")),
            )
            .map_err(|ffmpeg_error| format!("{ffmpeg_error} (dwebp: {dwebp_error})")),
        };
    }
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
        let dir = std::env::temp_dir().join(format!("wa2-scale-dir-test-{}-bg_8x", std::process::id()));
        std::fs::create_dir_all(&dir).unwrap();
        assert_eq!(super::dir_art_scale(&dir), 8);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn decode_ui_file_reduces_scaled_sheets_to_logical_size() {
        use std::io::Write;
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
    fn decode_art_file_reduces_scaled_tga_proxy() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("wa2-art-tga-scale-test-{}", std::process::id()));
        let base = dir.join("bg");
        let scaled = dir.join("bg_2x");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&scaled).unwrap();

        fn write_tga(path: &std::path::Path, width: u16, height: u16) {
            let mut header = vec![0u8; 18];
            header[2] = 2;
            header[12..14].copy_from_slice(&width.to_le_bytes());
            header[14..16].copy_from_slice(&height.to_le_bytes());
            header[16] = 32;
            header[17] = 8;
            let mut file = std::fs::File::create(path).unwrap();
            file.write_all(&header).unwrap();
            file.write_all(&vec![0x40u8; width as usize * height as usize * 4]).unwrap();
        }
        write_tga(&base.join("panel.tga"), 2, 2);
        write_tga(&scaled.join("panel.tga"), 4, 4);
        let old = std::env::var_os("WA2_RENDER_SCALE");
        std::env::set_var("WA2_RENDER_SCALE", "1");
        let img = super::decode_art_file(&scaled.join("panel.tga")).unwrap();
        match old {
            Some(v) => std::env::set_var("WA2_RENDER_SCALE", v),
            None => std::env::remove_var("WA2_RENDER_SCALE"),
        }
        assert_eq!((img.width, img.height), (2, 2));
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn vp8l_dimensions_drive_actual_file_scale() {
        let dir = std::env::temp_dir().join(format!("wa2-vp8l-scale-test-{}", std::process::id()));
        let base = dir.join("bg");
        let scaled = dir.join("bg_8x");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&scaled).unwrap();
        let mut original = vec![0u8; 18];
        original[12..14].copy_from_slice(&128u16.to_le_bytes());
        original[14..16].copy_from_slice(&64u16.to_le_bytes());
        std::fs::write(base.join("panel.tga"), original).unwrap();

        let width = 512u32;
        let height = 256u32;
        let bits = (width - 1) | ((height - 1) << 14);
        let mut webp = b"RIFF\0\0\0\0WEBPVP8L\0\0\0\0\x2f".to_vec();
        webp.extend_from_slice(&bits.to_le_bytes());
        std::fs::write(scaled.join("panel.webp"), webp).unwrap();
        assert_eq!(super::source_art_scale(&scaled.join("panel.webp")), 4);
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn webp_ui_and_art_decoders_use_scaled_tree_dimensions() {
        use std::io::Write;
        let dir = std::env::temp_dir().join(format!("wa2-webp-scale-test-{}", std::process::id()));
        let base = dir.join("bg");
        let scaled = dir.join("bg_8x");
        std::fs::create_dir_all(&base).unwrap();
        std::fs::create_dir_all(&scaled).unwrap();

        fn write_tga(path: &std::path::Path, width: u16, height: u16) {
            let mut header = vec![0u8; 18];
            header[2] = 2;
            header[12..14].copy_from_slice(&width.to_le_bytes());
            header[14..16].copy_from_slice(&height.to_le_bytes());
            header[16] = 32;
            header[17] = 8;
            let mut file = std::fs::File::create(path).unwrap();
            file.write_all(&header).unwrap();
            file.write_all(&vec![0x80u8; width as usize * height as usize * 4]).unwrap();
        }
        write_tga(&base.join("panel.tga"), 2, 2);
        let source = scaled.join("panel.tga");
        write_tga(&source, 16, 16);
        let webp = scaled.join("panel.webp");
        let status = std::process::Command::new("ffmpeg")
            .args(["-v", "error", "-y", "-i"])
            .arg(&source)
            .args(["-c:v", "libwebp", "-lossless", "1"])
            .arg(&webp)
            .status();
        if !status.is_ok_and(|s| s.success()) {
            std::fs::remove_dir_all(&dir).ok();
            return;
        }
        let ui = super::decode_ui_file(&webp).unwrap();
        assert_eq!((ui.width, ui.height), (2, 2));
        let old = std::env::var_os("WA2_RENDER_SCALE");
        std::env::set_var("WA2_RENDER_SCALE", "1");
        let art = super::decode_art_file(&webp).unwrap();
        match old {
            Some(v) => std::env::set_var("WA2_RENDER_SCALE", v),
            None => std::env::remove_var("WA2_RENDER_SCALE"),
        }
        assert_eq!((art.width, art.height), (2, 2));
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

