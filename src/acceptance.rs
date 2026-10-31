//! Opt-in, bounded real-runtime checkpoints, not synthetic unit tests.
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};
use std::time::{Duration, Instant};

pub struct Probe {
    pub out: PathBuf,
    pub stage: usize,
    pub waits: usize,
    pub saved_pixels: Option<String>,
    pub saved_pc: usize,
    pub saved_scene: String,
    pub started: Instant,
}
static PROBE: OnceLock<Mutex<Probe>> = OnceLock::new();
pub fn active() -> bool { PROBE.get().is_some() }
pub fn probe() -> Option<std::sync::MutexGuard<'static, Probe>> {
    PROBE.get().map(|p| p.lock().unwrap())
}
pub fn start(kind: &str, out: &Path) -> Result<(), String> {
    if !cfg!(target_os = "macos") { return Err("macOS acceptance requires macOS".into()); }
    for key in ["SDL_VIDEODRIVER", "SDL_AUDIODRIVER"] {
        if std::env::var(key).is_ok_and(|s| s.to_ascii_lowercase().contains("dummy") || s == "disk") {
            return Err(format!("{key}: fake drivers cannot establish gameplay acceptance"));
        }
    }
    std::fs::create_dir_all(out).map_err(|e| e.to_string())?;
    if out.join("report.txt").exists() { return Err("output already has report.txt; choose a fresh directory".into()); }
    let seconds = std::env::var("WA2_ACCEPTANCE_TIMEOUT").ok().and_then(|s| s.parse::<u64>().ok()).unwrap_or(120).clamp(5, 600);
    let report = out.join("report.txt");
    std::fs::write(&report, format!("schema=1\nkind={kind}\nscope=real-runtime-checkpoint-not-full-playthrough\n")).map_err(|e| e.to_string())?;
    PROBE.set(Mutex::new(Probe { out: out.into(), stage: 0, waits: 0,
        saved_pixels: None, saved_pc: 0, saved_scene: String::new(), started: Instant::now() }))
        .map_err(|_| "acceptance already started")?;
    // Independent watchdog also bounds blocking decoder reads / nested UI loops.
    // The runner kills this process group on timeout to reap descendant ffmpeg.
    std::thread::spawn(move || {
        std::thread::sleep(Duration::from_secs(seconds));
        eprintln!("acceptance: exceeded {seconds}s watchdog");
        std::process::exit(124);
    });
    Ok(())
}
pub fn field(p: &Probe, key: &str, value: impl std::fmt::Display) -> Result<(), String> {
    use std::io::Write;
    let value = value.to_string().replace(['\n', '\r'], " ");
    let mut f = std::fs::OpenOptions::new().append(true).open(p.out.join("report.txt")).map_err(|e| e.to_string())?;
    writeln!(f, "{key}={value}").map_err(|e| e.to_string())
}
pub fn native(p: &Probe, audio: bool) -> Result<(), String> {
    let video = crate::sdl2::sdl_video_driver();
    field(p, "video_driver", &video)?;
    if video != "cocoa" { return Err(format!("expected cocoa, got {video}")); }
    if audio {
        let driver = crate::sdl2::sdl_audio_driver();
        field(p, "audio_driver", &driver)?;
        if driver != "coreaudio" { return Err(format!("expected coreaudio, got {driver}")); }
    }
    Ok(())
}
pub fn screenshot(p: &Probe, name: &str, surf: &crate::render::Surface) -> Result<String, String> {
    if name == "renderer" || name == "restored-renderer" {
        let first = surf.pixels.get(..4).ok_or("empty renderer capture")?;
        let changed = surf.pixels.chunks_exact(4).filter(|px| px[..3] != first[..3]).count();
        field(p, "renderer_nonuniform_pixels", changed)?;
        if changed < 1000 { return Err("SDL renderer capture is blank or uniform".into()); }
    }
    let hash = crate::sha256::hex_digest(&surf.pixels);
    surf.save_tga(p.out.join(format!("{name}.tga")).to_str().ok_or("output path is not UTF-8")?).map_err(|e| e.to_string())?;
    field(p, &format!("{name}_pixels_sha256"), &hash)?;
    field(p, &format!("{name}_size"), format!("{}x{}", surf.width, surf.height))?;
    Ok(hash)
}
pub fn chinese(p: &Probe, text: &str) -> Result<(), String> {
    let n = text.chars().filter(|c| ('\u{3400}'..='\u{9fff}').contains(c)).count();
    let missing = crate::render::text::missing_glyph_stats();
    field(p, "chinese_characters", n)?;
    field(p, "missing_glyphs", missing.len())?;
    if n == 0 || !missing.is_empty() { return Err("Chinese text absent or missing glyphs recorded".into()); }
    Ok(())
}
/// Exercise real Cocoa fullscreen/windowed geometry without touching user INI.
pub fn display_cycle(out: &Path) -> Result<(), String> {
    start("display-cycle", out)?;
    let mut win = crate::sdl2::Sdl::new("WA2 Retina display diagnostic", 1280, 720).ok_or("SDL unavailable")?;
    let original = win.window_size();
    if !win.is_resizable() { return Err("native window resize flag absent".into()); }
    for (name, full) in [("windowed", false), ("fullscreen", true), ("restored-window", false),
        ("resized-square", false), ("resized-wide", false), ("restored-size", false)] {
        win.set_fullscreen(full);
        match name {
            "resized-square" => win.resize_window(800, 800),
            "resized-wide" => win.resize_window(1100, 500),
            "restored-size" => win.resize_window(original.0, original.1),
            _ => {}
        }
        let until = Instant::now() + Duration::from_millis(700);
        while Instant::now() < until {
            if matches!(win.poll_input(), crate::sdl2::Input::Quit) { return Err("display test cancelled".into()); }
            win.delay(16);
        }
        let (lw, lh) = win.window_size();
        let (dw, dh) = win.drawable_size();
        if dw <= 0 || dh <= 0 { return Err("empty content viewport".into()); }
        if full != win.is_fullscreen() { return Err("native fullscreen flag disagrees with request".into()); }
        if (name == "restored-window" || name == "restored-size") && (lw, lh) != original { return Err("window geometry not restored".into()); }
        let center = win.surface_pos(lw / 2, lh / 2, 1280, 720);
        if (center.0-640).abs() > 1 || (center.1-360).abs() > 1 { return Err("pointer geometry changed on display switch".into()); }
        let mut surf = crate::render::Surface::new(dw as u32, dh as u32, [12, 35, 55, 255]);
        surf.fill_rect(dw/4, dh/4, dw as u32/2, dh as u32/2, [35, 130, 150, 255]);
        let size = (26 * dh / 720).max(8) as u32;
        let rendered = crate::render::text::with_ui_font(size, |f| {
            f.draw_flat(&mut surf, "Retina 原生像素／窗口全屏切换", dw/4 + 20, dh/2, [255,255,255]);
        });
        if rendered.is_none() { return Err("font unavailable in display test".into()); }
        let captured = win.present_checked(&surf)?;
        let (ox, oy) = ((captured.width - surf.width) / 2, (captured.height - surf.height) / 2);
        for y in 0..surf.height as usize {
            let a = y * surf.width as usize * 4;
            let b = ((y + oy as usize) * captured.width as usize + ox as usize) * 4;
            if captured.pixels[b..b+surf.width as usize*4] != surf.pixels[a..a+surf.width as usize*4] {
                return Err("renderer rescaled native-resolution content".into());
            }
        }
        if (ox > 0 || oy > 0) && win.surface_pos(0, 0, 1280, 720) != (-1,-1) {
            return Err("letterbox bar accepted as game input".into());
        }
        let p = probe().unwrap();
        native(&p, false)?;
        field(&p, &format!("{name}_logical"), format!("{lw}x{lh}"))?;
        field(&p, &format!("{name}_drawable"), format!("{dw}x{dh}"))?;
        field(&p, &format!("{name}_font_px"), size)?;
        screenshot(&p, name, &captured)?;
    }
    probe().unwrap().stage = 3;
    finish()
}

pub fn finish() -> Result<(), String> {
    let p = probe().ok_or("acceptance not started")?;
    if p.stage != 3 { return Err("runtime exited before acceptance checkpoint completed".into()); }
    field(&p, "elapsed_ms", p.started.elapsed().as_millis())?;
    field(&p, "status", "pass")
}
