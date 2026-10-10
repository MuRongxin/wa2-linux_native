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