//! Opt-in real-device continuity stress. Does not modify game settings or saves.
//! This measures decoder/SDL continuity, not subjective or hardware output.
use super::mixer::{Mixer, CAT_BGM, CAT_SE, CAT_VOICE, SAMPLE_RATE};
use std::path::Path;
use std::process::Command;
use std::thread;
use std::time::{Duration, Instant};

pub fn run(win: &crate::sdl2::Sdl, input: &str, seconds: f64) -> Result<(), String> {
    if !seconds.is_finite() || seconds < 1.0 { return Err("duration must be finite and >=1 second".into()); }
    if !Path::new(input).is_file() { return Err(format!("media not found: {input}")); }
    let duration = Command::new("ffprobe").args(["-v", "error", "-show_entries", "format=duration",
        "-of", "default=noprint_wrappers=1:nokey=1", input]).output()
        .ok().filter(|o| o.status.success())
        .and_then(|o| String::from_utf8_lossy(&o.stdout).trim().parse::<f64>().ok())
        .filter(|s| s.is_finite() && *s > 0.0);
    let se = std::env::var("WA2_AUDIO_TEST_SE").unwrap_or_else(|_| input.into());
    let voice = std::env::var("WA2_AUDIO_TEST_VOICE").unwrap_or_else(|_| input.into());
    for path in [&se, &voice] {
        if !Path::new(path).is_file() { return Err(format!("overlap media not found: {path}")); }
    }
    let driver = crate::sdl2::sdl_audio_driver();
    if driver == "dummy" || driver == "disk" || driver == "none" {
        return Err(format!("real-device acceptance rejects audio driver {driver}"));
    }
    let mixer = Mixer::new(win).ok_or("audio device unavailable")?;
    let bgm = mixer.play_cat(input, 0.65, true, CAT_BGM);
    if bgm == usize::MAX { return Err("cannot spawn BGM decoder".into()); }
    let warmup = Instant::now();
    while mixer.stats().started_streams == 0 {
        if warmup.elapsed() > Duration::from_secs(10) || mixer.finished(bgm) {
            return Err(format!("decoder failed startup: {:?}", mixer.stats()));
        }
        thread::sleep(Duration::from_millis(10));
    }
    // Start/end queue depth variation is bounded; startup silence isn't a gap.
    let before = mixer.stats();
    let start = Instant::now();
    let mut cycle = 0usize;
    let mut overlap = Vec::new();
    let stalls = [100, 500, 2000];
    while start.elapsed().as_secs_f64() < seconds {
        // No game-thread audio feeding, polling, or event loop runs during these
        // long pauses. Existing BGM must continue while new decoders spawn/stop.
        let slot = mixer.play_cat(if cycle % 2 == 0 { &se } else { &voice }, 0.08,
            false, if cycle % 2 == 0 { CAT_SE } else { CAT_VOICE });
        if slot == usize::MAX { return Err("cannot spawn overlap decoder".into()); }
        overlap.push(slot);
        if overlap.len() > 3 { mixer.fade(overlap.remove(0), 0.0, 6, true); }
        let remaining = (seconds - start.elapsed().as_secs_f64()).max(0.0);
        thread::sleep(Duration::from_secs_f64((stalls[cycle % stalls.len()] as f64 / 1000.0).min(remaining)));
        cycle += 1;
    }
    let elapsed = start.elapsed().as_secs_f64();
    let after = mixer.stats();
    let frames = after.submitted_frames.saturating_sub(before.submitted_frames);
    let rate = frames as f64 / elapsed;
    let decoded = mixer.stream_decoded_frames(bgm).unwrap_or(0);
    // Require wall playback to pass the boundary too: decoding alone includes
    // up to half a second of read-ahead that may not yet have reached the device.
    let loop_crossed = duration.map(|d| elapsed > d + 1.0 &&
        decoded as f64 > (d + 0.1) * SAMPLE_RATE as f64).unwrap_or(false);
    println!("audio_continuity driver={driver} seconds={elapsed:.3} source={input:?} source_seconds={duration:?}");
    println!("submitted_frames={frames} submission_rate={rate:.3} main_stall_cycles={cycle} max_main_stall_ms=2000 bgm_decoded_frames={decoded} loop_crossed={loop_crossed}");
    println!("before={before:?}\nafter={after:?}");
    println!("Evidence boundary: SDL queue/decoder metrics only; listen or record native output to certify perceived continuity.");
    if after.decoder_underrun_frames != before.decoder_underrun_frames ||
        after.device_underruns != before.device_underruns ||
        after.queue_errors != 0 || after.decoder_errors != 0 || mixer.finished(bgm) {
        return Err("continuity stress detected starvation or audio errors; see counters".into());
    }
    // SDL callbacks consume whole blocks; tolerate endpoint phase, not drift.
    if (frames as f64 - elapsed * SAMPLE_RATE as f64).abs() > 8192.0 + elapsed * SAMPLE_RATE as f64 * 0.02 {
        return Err(format!("device submission rate inconsistent with 48kHz: {rate}"));
    }
    if !loop_crossed {
        return Err(format!("no loop boundary verified; rerun longer than source duration {duration:?} (ffprobe required)"));
    }
    println!("audio_continuity=PASS (not a subjective listening certification)");
    Ok(())
}
