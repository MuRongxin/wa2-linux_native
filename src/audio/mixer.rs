//! Audio mixer: SDL2 audio device + software mixing of multiple PCM streams.
//!
//! Stream decoding (Ogg Vorbis from LAC, WMA from ASF) is delegated to an
//! `ffmpeg` subprocess that writes raw interleaved s16le PCM to a pipe; this
//! module pulls that PCM on a background thread, keeps a ring buffer per
//! stream, applies per-stream volume/pan, mixes to a master buffer, and hands
//! it to SDL_QueueAudio. The SDL audio callback is not used (we push from the
//! mixer thread instead) which keeps mixing simple and deterministic.

use std::collections::VecDeque;
use std::io::Read;
use std::process::{Child, Command, Stdio};
use std::sync::{Arc, Mutex};
use std::thread;

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;

use std::sync::atomic::{AtomicU64, Ordering};
static PULLED: AtomicU64 = AtomicU64::new(0);
static QUEUED: AtomicU64 = AtomicU64::new(0);

/// Total PCM samples pulled from decoder subprocesses (diagnostic).
pub fn pulled_samples() -> u64 { PULLED.load(Ordering::Relaxed) }
/// Total bytes pushed to the SDL audio device (diagnostic).
pub fn queued_bytes() -> u64 { QUEUED.load(Ordering::Relaxed) }

/// One decoded audio stream (BGM, SE, voice, ...).
pub struct Stream {
    child: Option<Child>,
    /// raw s16le frames not yet mixed
    pcm: VecDeque<i16>,
    pub volume: f32,
    pub looping: bool,
    eos: bool,
}

impl Stream {
    /// Spawn `ffmpeg -i <in> -f s16le -ar 48000 -ac 2 -` and stream-decode it.
    pub fn spawn_ffmpeg(input: &str) -> Option<Stream> {
        let child = Command::new("ffmpeg")
            .args(&["-hide_banner", "-loglevel", "error", "-i", input,
                    "-f", "s16le", "-ar", "48000", "-ac", "2", "-"])
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .spawn()
            .ok()?;
        Some(Stream { child: Some(child), pcm: VecDeque::new(), volume: 1.0, looping: false, eos: false })
    }
}

/// The mixer owns the SDL audio device and all active streams.
pub struct Mixer {
    inner: Arc<Mutex<MixerInner>>,
    worker: Option<thread::JoinHandle<()>>,
}

struct MixerInner {
    streams: Vec<Option<Stream>>,
    quit: bool,
}

impl Mixer {
    /// Open the SDL audio device and start the mixing thread.
    /// Returns None if SDL audio could not be initialised.
    pub fn new(sdl: &crate::sdl2::Sdl) -> Option<Mixer> {
        let dev = crate::sdl2::sdl_open_audio(SAMPLE_RATE, CHANNELS);
        let inner = Arc::new(Mutex::new(MixerInner { streams: Vec::new(), quit: false }));
        let inner2 = Arc::clone(&inner);
        let worker = thread::spawn(move || {
            let mut carry: Vec<u8> = Vec::new();
            while !inner2.lock().unwrap().quit {
                // pull any available PCM from children
                {
                    let mut g = inner2.lock().unwrap();
                    for s in g.streams.iter_mut() {
                        if let Some(st) = s {
                            let mut buf = [0u8; 4096];
                            if let Some(child) = st.child.as_mut() {
                                if let Some(out) = child.stdout.as_mut() {
                                    match out.read(&mut buf) {
                                        Ok(0) => { st.eos = true; }
                                        Ok(n) => {
                                            carry.extend_from_slice(&buf[..n]);
                                            let usable = carry.len() - (carry.len() % 2);
                                            for ch in carry[..usable].chunks_exact(2) {
                                                st.pcm.push_back(i16::from_le_bytes([ch[0], ch[1]]));
                                            }
                                            PULLED.fetch_add((usable / 2) as u64, Ordering::Relaxed);
                                            carry.drain(..usable);
                                        }
                                        Err(_) => st.eos = true,
                                    }
                                }
                            }
                        }
                    }
                }
                // mix & push
                let block = mix_once(&inner2);
                QUEUED.fetch_add(block.len() as u64, Ordering::Relaxed);
                crate::sdl2::sdl_queue_audio(dev, &block);
                thread::sleep(std::time::Duration::from_millis(10));
            }
        });
        crate::sdl2::sdl_pause_audio(dev, false);
        Some(Mixer { inner, worker: Some(worker) })
    }

    /// Allocate a stream slot and start an ffmpeg decode into it.
    pub fn play(&self, input: &str, volume: f32, looping: bool) -> usize {
        let mut g = self.inner.lock().unwrap();
        let mut s = match Stream::spawn_ffmpeg(input) {
            Some(s) => s, None => return usize::MAX,
        };
        s.volume = volume;
        s.looping = looping;
        g.streams.push(Some(s));
        g.streams.len() - 1
    }

    pub fn stop(&self, idx: usize) {
        let mut g = self.inner.lock().unwrap();
        if idx < g.streams.len() {
            if let Some(mut s) = g.streams[idx].take() {
                if let Some(mut c) = s.child.take() { let _ = c.kill(); let _ = c.wait(); }
            }
        }
    }

    pub fn stop_all(&self) {
        let n = { self.inner.lock().unwrap().streams.len() };
        for i in 0..n { self.stop(i); }
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        self.inner.lock().unwrap().quit = true;
        if let Some(h) = self.worker.take() { let _ = h.join(); }
        self.stop_all();
    }
}

/// Mix one block (assume 1024 stereo frames) from all active streams.
fn mix_once(inner: &Arc<Mutex<MixerInner>>) -> Vec<u8> {
    const BLOCK: usize = 1024;
    let frames = BLOCK * CHANNELS as usize;
    let mut acc = vec![0i32; frames];
    {
        let mut g = inner.lock().unwrap();
        for s in g.streams.iter_mut() {
            if let Some(st) = s {
                let vol = (st.volume * 32767.0) as i32;
                for i in 0..frames {
                    let v = match st.pcm.pop_front() {
                        Some(x) => x as i32,
                        None => {
                            if st.eos {
                                0
                            } else {
                                break;
                            }
                        }
                    };
                    acc[i] += (v * vol) >> 16;
                }
            }
        }
    }
    let mut out = Vec::with_capacity(frames * 2);
    for v in acc {
        let c = v.clamp(-32768, 32767) as i16;
        out.extend_from_slice(&c.to_le_bytes());
    }
    out
}