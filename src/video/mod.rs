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
    frame_bytes: usize,
}

impl Movie {
    /// Probe dimensions/fps with ffprobe and spawn an ffmpeg rawvideo decoder.
    pub fn open(path: &str) -> Option<Movie> {
        // probe
        let out = Command::new("ffprobe")
            .args(&["-v", "error", "-select_streams", "v:0",
                    "-show_entries", "stream=width,height,r_frame_rate",
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
            if parts.len() >= 3 {
                let rr = parts[2].trim();
                if let Some((n, d)) = rr.split_once('/') {
                    let n: f64 = n.parse().unwrap_or(0.0);
                    let d: f64 = d.parse().unwrap_or(1.0);
                    if d != 0.0 && n > 0.0 { fps = n / d; }
                }
            }
        }
        let mut child = Command::new("ffmpeg")
            .args(&["-hide_banner", "-loglevel", "error", "-i", path,
                    "-f", "rawvideo", "-pix_fmt", "rgba",
                    "-s", &format!("{}x{}", width, height), "-"])
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null())
            .spawn().ok()?;
        let stdout = child.stdout.take()?;
        Some(Movie { child, stdout, width, height, fps,
                 frame_bytes: (width * height * 4) as usize })
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