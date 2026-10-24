//! Movie playback: WA2 `mv*.pak` are RAW ASF/WMV files (no archive wrapper).
//!
//! ffmpeg decodes video to a raw RGBA frame stream on a pipe; we upload each
//! frame to an SDL texture. Audio for the movie is decoded by a second ffmpeg
//! into the shared `Mixer`.

use std::io::Read;
use std::process::{Child, ChildStdout, Command, Stdio};

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
            // Passthrough (`-vsync 0`) is essential: WA2's ASF containers declare
            // a bogus `r_frame_rate` of 1000/1, so ffmpeg's default frame-rate
            // conversion duplicates every decoded frame ~33x. The engine would
            // then read 1000 identical frames per second of wall clock and show
            // only the first instants of the movie (usually a black fade-in) -
            // playback looked frozen and black. Use the equivalent legacy
            // option so native fallbacks also work with older distro FFmpeg.
            // Passthrough emits each decoded
            // frame exactly once and `fps` (packets / duration) paces it.
            .args(&["-hide_banner", "-loglevel", "error", "-i", path,
                    "-vsync", "0",
                    "-f", "rawvideo", "-pix_fmt", "rgba",
                    "-s", &format!("{}x{}", width, height), "-"])
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().ok()?;
        let stdout = child.stdout.take()?;
        Some(Movie { child, stdout, width, height, fps,
                 has_audio: probe_audio(path),
                 path: path.to_string(),
                 frame_bytes: (width * height * 4) as usize })
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

    /// Read the next RGBA frame. Returns false at EOF.
    pub fn read_frame(&mut self, buf: &mut Vec<u8>) -> bool {
        buf.clear();
        buf.resize(self.frame_bytes, 0);
        let mut read = 0usize;
        while read < self.frame_bytes {
            match self.stdout.read(&mut buf[read..]) {
                Ok(0) => break,
                Ok(n) => read += n,
                Err(_) => break,
            }
        }
        read == self.frame_bytes
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
