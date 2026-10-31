//! Movie playback: WA2 `mv*.pak` are RAW ASF/WMV files (no archive wrapper).
//!
//! ffmpeg decodes video to a raw RGBA frame stream on a pipe; we upload each
//! frame to an SDL texture. Audio for the movie is decoded by a second ffmpeg
//! into the shared `Mixer`.

use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};
use std::sync::OnceLock;

fn passthrough_args(help: &str) -> [&'static str; 2] {
    // Match the option itself, not a mention of it in a deprecation message.
    let modern = help.lines().any(|line| {
        line.split_whitespace().next()
            .is_some_and(|option| option.split([':', '[']).next() == Some("-fps_mode"))
    });
    if modern { ["-fps_mode", "passthrough"] } else { ["-vsync", "0"] }
}

fn ffmpeg_passthrough_args() -> &'static [&'static str; 2] {
    static ARGS: OnceLock<[&'static str; 2]> = OnceLock::new();
    ARGS.get_or_init(|| {
        // Probe once per process rather than guessing from version strings:
        // distributions backport options, and development builds lack a major.
        match Command::new("ffmpeg").args(["-hide_banner", "-h", "full"]).output() {
            Ok(output) if output.status.success() => {
                let help = format!("{}\n{}", String::from_utf8_lossy(&output.stdout),
                    String::from_utf8_lossy(&output.stderr));
                passthrough_args(&help)
            }
            _ => ["-vsync", "0"], // preserve the legacy Linux fallback
        }
    })
}

pub struct Movie {
    child: Child,
    stdout: ChildStdout,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    /// True when ffprobe reports at least one audio stream in the container.
    pub has_audio: bool,
    /// Source path, kept so the audio track can be handed to the Mixer.
    path: String,
    frame_bytes: usize,
    finished: bool,
    decoder_failed: bool,
}

impl Movie {
    /// Probe dimensions/fps with ffprobe and spawn an ffmpeg rawvideo decoder.
    pub fn open(path: &str) -> Option<Movie> {
        // Probe: WA2 movies are ASF/WMV whose `r_frame_rate` is a bogus 1000/1
        // (timescale) and `avg_frame_rate` is 0/0, so derive fps from the decoded
        // packet count and the duration instead.
        let out = Command::new("ffprobe")
            .args(&["-v", "error", "-select_streams", "v:0", "-count_packets",
                    "-show_entries", "stream=width,height,duration,nb_read_packets",
                    "-of", "csv=p=0", path])
            .output().ok()?;
        if !out.status.success() {
            eprintln!("movie: ffprobe failed for {path}: {}", String::from_utf8_lossy(&out.stderr).trim());
            return None;
        }
        let s = String::from_utf8_lossy(&out.stdout);
        let mut width = 1280u32; let mut height = 720u32; let mut fps = 30000.0/1001.0;
        if let Some(first) = s.lines().next() {
            let parts: Vec<&str> = first.trim().split(',').collect();
            if parts.len() >= 2 {
                width = parts[0].trim().parse().unwrap_or(1280);
                height = parts[1].trim().parse().unwrap_or(720);
            }
            if parts.len() >= 4 {
                let dur: f64 = parts[2].trim().parse().unwrap_or(0.0);
                let n: f64 = parts[3].trim().parse().unwrap_or(0.0);
                if dur > 0.0 && n > 0.0 { fps = n / dur; }
            }
        }
        let mut child = Command::new("ffmpeg")
            // Passthrough is essential: WA2's ASF containers declare
            // a bogus `r_frame_rate` of 1000/1, so ffmpeg's default frame-rate
            // conversion duplicates every decoded frame ~33x. The engine would
            // then read 1000 identical frames per second of wall clock and show
            // only the first instants of the movie (usually a black fade-in) -
            // playback looked frozen and black. New FFmpeg removed -vsync;
            // capability detection keeps pre-5 distro FFmpeg working as well.
            // Emit each decoded frame once, paced by `fps` (packets / duration).
            .args(["-hide_banner", "-loglevel", "error", "-i", path])
            .args(ffmpeg_passthrough_args())
            .args(["-f", "rawvideo", "-pix_fmt", "rgba",
                    "-s", &format!("{}x{}", width, height), "-"])
            // Inherit stderr so decoder errors are visible and cannot fill an
            // unread pipe while the renderer is waiting for the next frame.
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::inherit())
            .spawn().map_err(|error| eprintln!("movie: cannot start ffmpeg: {error}")).ok()?;
        let stdout = child.stdout.take()?;
        Some(Movie { child, stdout, width, height, fps,
                 has_audio: probe_audio(path),
                 path: path.to_string(),
                 frame_bytes: (width * height * 4) as usize,
                 finished: false, decoder_failed: false })
    }

    /// Start decoding the movie's own audio track into the mixer.
    ///
    /// The mixer spawns its own ffmpeg (`-i <movie> -f s16le -ar 48000 -ac 2 -`),
    /// which picks the container's only/best audio stream, so the WMA track of
    /// the ASF is decoded and mixed like any other BGM. Returns the stream slot
    /// so the caller can stop it on skip/EOF; `None` for silent movies or when
    /// the decoder could not be spawned.
    pub fn play_audio(&self, mixer: &crate::audio::Mixer, cat: usize) -> Option<usize> {
        if !self.has_audio { return None; }
        let slot = mixer.play_cat(&self.path, 1.0, false, cat);
        if slot == usize::MAX { None } else { Some(slot) }
    }

    /// Read the next RGBA frame. Returns false at EOF or a reported decoder
    /// error; callers can distinguish failure with `decoder_failed()`.
    pub fn read_frame(&mut self, buf: &mut Vec<u8>) -> bool {
        buf.clear();
        if self.finished { return false; }
        buf.resize(self.frame_bytes, 0);
        let mut read = 0usize;
        while read < self.frame_bytes {
            match self.stdout.read(&mut buf[read..]) {
                Ok(0) => break,
                Ok(n) => read += n,
                Err(error) if error.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(error) => {
                    eprintln!("movie: decoder read failed for {}: {error}", self.path);
                    self.decoder_failed = true;
                    // A failed read must not leave wait() blocked on a decoder
                    // that is still trying to write into our pipe.
                    let _ = self.child.kill();
                    break;
                }
            }
        }
        if read == self.frame_bytes { return true; }
        self.finished = true;
        if read > 0 {
            eprintln!("movie: truncated RGBA frame for {} ({read}/{} bytes)", self.path, self.frame_bytes);
            self.decoder_failed = true;
        }
        match self.child.wait() {
            Ok(status) if status.success() => {}
            Ok(status) => {
                eprintln!("movie: ffmpeg decoder failed for {}: {status}", self.path);
                self.decoder_failed = true;
            }
            Err(error) => {
                eprintln!("movie: cannot reap ffmpeg decoder for {}: {error}", self.path);
                self.decoder_failed = true;
            }
        }
        buf.clear();
        false
    }

    pub fn decoder_failed(&self) -> bool {
        self.decoder_failed
    }
}

impl Drop for Movie {
    fn drop(&mut self) {
        let _ = self.child.kill();
        let _ = self.child.wait();
    }
}

/// Does the container carry an audio stream? Some `mv*.pak` (silent cuts) do not.
fn probe_audio(path: &str) -> bool {
    let out = Command::new("ffprobe")
        .args(&["-v", "error", "-select_streams", "a:0",
                "-show_entries", "stream=codec_name", "-of", "csv=p=0", path])
        .output();
    match out {
        Ok(o) => !String::from_utf8_lossy(&o.stdout).trim().is_empty(),
        Err(_) => false,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fake_decoder(script: &str) -> Movie {
        let mut child = Command::new("sh").args(["-c", script])
            .stdin(Stdio::null()).stdout(Stdio::piped()).spawn().unwrap();
        let stdout = child.stdout.take().unwrap();
        Movie { child, stdout, width: 1, height: 1, fps: 24.0,
            has_audio: false, path: "synthetic-test".into(), frame_bytes: 4,
            finished: false, decoder_failed: false }
    }

    #[test]
    fn decoder_exit_status_and_partial_frames_are_not_clean_eof() {
        let mut frame = Vec::new();
        let mut good = fake_decoder("printf abcd");
        assert!(good.read_frame(&mut frame));
        assert_eq!(&frame, b"abcd");
        assert!(!good.read_frame(&mut frame));
        assert!(!good.decoder_failed());
        assert!(frame.is_empty());
        let mut failed = fake_decoder("printf abcd; exit 7");
        assert!(failed.read_frame(&mut frame));
        assert!(!failed.read_frame(&mut frame));
        assert!(failed.decoder_failed());
        assert!(!failed.read_frame(&mut frame));
        let mut partial = fake_decoder("printf a");
        assert!(!partial.read_frame(&mut frame));
        assert!(partial.decoder_failed());
        let mut immediate = fake_decoder("exit 1");
        assert!(!immediate.read_frame(&mut frame));
        assert!(immediate.decoder_failed());
    }

    #[test]
    fn modern_ffmpeg_uses_fps_mode_passthrough() {
        assert_eq!(passthrough_args("Video options:\n-fps_mode[:stream_specifier] fps_mode set framerate mode\n"),
            ["-fps_mode", "passthrough"]);
        assert_eq!(passthrough_args("  -fps_mode mode set video framerate mode\n"),
            ["-fps_mode", "passthrough"]);
    }

    #[test]
    fn legacy_ffmpeg_keeps_vsync_zero() {
        assert_eq!(passthrough_args("-vsync number video sync method\n"), ["-vsync", "0"]);
        assert_eq!(passthrough_args(""), ["-vsync", "0"]);
    }

    #[test]
    fn prose_does_not_advertise_a_supported_option() {
        assert_eq!(passthrough_args("-vsync number deprecated; use -fps_mode\n"), ["-vsync", "0"]);
        assert_eq!(passthrough_args("-fps_mode_other value\n"), ["-vsync", "0"]);
    }
}
