//! Background PCM decoder draining and queue-depth-controlled SDL playback.
//!
//! The game/render thread never feeds PCM. Decoder pipes are nonblocking, drained
//! into bounded reserves, and SDL's queue depth (not wall-clock sleeps) determines
//! how much to mix. Process creation and teardown never hold the mixer mutex.

use std::collections::VecDeque;
use std::io::Read;
use std::os::fd::AsRawFd;
use std::process::{Child, Command, Stdio};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

unsafe extern "C" { fn fcntl(fd: i32, cmd: i32, ...) -> i32; }
const F_GETFL: i32 = 3;
const F_SETFL: i32 = 4;
#[cfg(target_os = "macos")]
const O_NONBLOCK: i32 = 0x0004;
#[cfg(not(target_os = "macos"))]
const O_NONBLOCK: i32 = 0x0800;

/// Preserve native descriptor flags; a blocking pipe must never enter the mixer.
fn set_nonblocking(fd: i32) -> std::io::Result<()> {
    unsafe {
        let flags = fcntl(fd, F_GETFL);
        if flags < 0 || fcntl(fd, F_SETFL, flags | O_NONBLOCK) < 0 {
            return Err(std::io::Error::last_os_error());
        }
    }
    Ok(())
}

pub const SAMPLE_RATE: u32 = 48_000;
pub const CHANNELS: u16 = 2;
const BLOCK_FRAMES: usize = 1024;
const FRAME_BYTES: usize = CHANNELS as usize * 2;
const BLOCK_BYTES: usize = BLOCK_FRAMES * FRAME_BYTES;
// ~85ms device headroom, bounded even if the worker wakes early or often.
const QUEUE_TARGET_BYTES: usize = BLOCK_BYTES * 4;
// Drain ahead rather than reading exactly one output block per worker wake.
const PCM_RESERVE_SAMPLES: usize = SAMPLE_RATE as usize * CHANNELS as usize / 2;
const PREROLL_SAMPLES: usize = BLOCK_FRAMES * CHANNELS as usize * 4;
const POLL_INTERVAL: Duration = Duration::from_millis(2);

pub const CAT_ALL: usize = 0;
pub const CAT_BGM: usize = 1;
pub const CAT_SE: usize = 2;
pub const CAT_VOICE: usize = 3;
pub const CAT_COUNT: usize = 4;

static PULLED: AtomicU64 = AtomicU64::new(0);
static QUEUED: AtomicU64 = AtomicU64::new(0);
pub fn pulled_samples() -> u64 { PULLED.load(Ordering::Relaxed) }
pub fn queued_bytes() -> u64 { QUEUED.load(Ordering::Relaxed) }

/// Per-mixer continuity diagnostics. Silence in media is NOT an underrun.
/// `device_underruns` counts observed empty SDL queues after initial prefill;
/// SDL does not expose actual hardware underruns, so this is a lower-bound proxy.
#[derive(Clone, Copy, Debug, Default)]
pub struct MixerStats {
    pub submitted_frames: u64,
    pub decoder_underrun_frames: u64,
    pub decoder_underrun_events: u64,
    pub device_underruns: u64,
    pub queue_errors: u64,
    pub decoder_errors: u64,
    pub min_queued_frames: u64,
    pub max_queued_frames: u64,
    pub max_worker_gap_us: u64,
    pub started_streams: u64,
}

pub struct Stream {
    cat: usize,
    carry: Vec<u8>,
    child: Option<Child>,
    pcm: VecDeque<i16>,
    pub volume: f32,
    pub looping: bool,
    eos: bool,
    started: bool,
    starved: bool,
    decoded_samples: u64,
    ramp: Option<(f32, f32, Instant, Duration, bool)>,
}

impl Stream {
    pub fn spawn_ffmpeg(input: &str, looping: bool) -> Option<Stream> {
        let mut command = Command::new("ffmpeg");
        command.args(["-hide_banner", "-loglevel", "error", "-nostdin"]);
        if looping { command.args(["-stream_loop", "-1"]); }
        let mut child = command.args(["-i", input, "-vn", "-f", "s16le", "-ar", "48000", "-ac", "2", "-"])
            .stdin(Stdio::null()).stdout(Stdio::piped()).stderr(Stdio::null()).spawn().ok()?;
        let result = child.stdout.as_ref()
            .ok_or_else(|| std::io::Error::new(std::io::ErrorKind::Other, "missing decoder stdout"))
            .and_then(|out| set_nonblocking(out.as_raw_fd()));
        if let Err(error) = result {
            eprintln!("audio: cannot make decoder pipe nonblocking: {error}");
            let _ = child.kill();
            let _ = child.wait();
            return None;
        }
        Some(Stream { cat: CAT_ALL, carry: Vec::new(), child: Some(child),
            pcm: VecDeque::new(), volume: 1.0, looping, eos: false, started: false,
            starved: false, decoded_samples: 0, ramp: None })
    }

    /// At most half a second per stream per pass; WouldBlock never means EOF.
    fn refill(&mut self, stats: &mut MixerStats) {
        let mut buf = [0u8; 16_384];
        while !self.eos && self.pcm.len() < PCM_RESERVE_SAMPLES {
            let room = ((PCM_RESERVE_SAMPLES - self.pcm.len()) * 2)
                .saturating_sub(self.carry.len()).min(buf.len());
            if room == 0 { break; }
            let Some(out) = self.child.as_mut().and_then(|c| c.stdout.as_mut()) else { break; };
            match out.read(&mut buf[..room]) {
                Ok(0) => {
                    self.eos = true;
                    if self.decoded_samples == 0 || !self.carry.is_empty() || self.looping {
                        stats.decoder_errors += 1;
                    }
                }
                Ok(n) => self.append_pcm(&buf[..n]),
                Err(e) if e.kind() == std::io::ErrorKind::WouldBlock => break,
                Err(e) if e.kind() == std::io::ErrorKind::Interrupted => continue,
                Err(_) => { self.eos = true; stats.decoder_errors += 1; }
            }
        }
    }

    fn append_pcm(&mut self, bytes: &[u8]) {
        self.carry.extend_from_slice(bytes);
        // Keep *stereo frames* intact even if a pipe returns odd byte counts.
        let usable = self.carry.len() / FRAME_BYTES * FRAME_BYTES;
        for ch in self.carry[..usable].chunks_exact(2) {
            self.pcm.push_back(i16::from_le_bytes([ch[0], ch[1]]));
        }
        self.decoded_samples += (usable / 2) as u64;
        PULLED.fetch_add((usable / 2) as u64, Ordering::Relaxed);
        self.carry.drain(..usable);
    }
}

pub struct Mixer {
    inner: Arc<Mutex<MixerInner>>,
    worker: Option<thread::JoinHandle<()>>,
    reaper: Option<thread::JoinHandle<()>>,
    cleanup: Option<mpsc::Sender<Child>>,
}

struct MixerInner {
    streams: Vec<Option<Stream>>,
    // Reserved for interface feedback only; never aliases a story SE handle.
    ui_slot: Option<usize>,
    gains: [f32; CAT_COUNT],
    quit: bool,
    stats: MixerStats,
}

impl MixerInner {
    fn new() -> Self {
        Self { streams: Vec::new(), ui_slot: None, gains: [1.0; CAT_COUNT], quit: false,
            stats: MixerStats { min_queued_frames: u64::MAX, ..MixerStats::default() } }
    }

    fn replace_ui(&mut self, mut stream: Stream) -> (usize, Option<Stream>) {
        stream.cat = CAT_SE;
        let slot = *self.ui_slot.get_or_insert_with(|| {
            self.streams.push(None);
            self.streams.len() - 1
        });
        let previous = self.streams[slot].replace(stream);
        (slot, previous)
    }
}

/// A whole-block top-up cannot exceed the target plus one partial block.
fn blocks_to_queue(queued: usize) -> usize {
    QUEUE_TARGET_BYTES.saturating_sub(queued).div_ceil(BLOCK_BYTES)
}

impl Mixer {
    pub fn new(_sdl: &crate::sdl2::Sdl) -> Option<Mixer> {
        let dev = crate::sdl2::sdl_open_audio(SAMPLE_RATE, CHANNELS);
        if dev == 0 { return None; }
        let inner = Arc::new(Mutex::new(MixerInner::new()));
        let inner2 = Arc::clone(&inner);
        // kill/wait may be arbitrarily slow: not on the mixer thread, and never
        // while holding the shared mutex (including fade-to-stop and EOF).
        let (cleanup, retired) = mpsc::channel::<Child>();
        let reaper = thread::spawn(move || {
            for mut child in retired {
                let _ = child.kill();
                let _ = child.wait();
            }
        });
        let worker_cleanup = cleanup.clone();
        let worker = thread::spawn(move || {
            let mut running = false;
            let mut last_wake = Instant::now();
            loop {
                let now = Instant::now();
                let gap = now.duration_since(last_wake).as_micros() as u64;
                last_wake = now;
                let queued = crate::sdl2::sdl_queued_audio_size(dev) as usize;
                let blocks = blocks_to_queue(queued);
                let mut g = inner2.lock().unwrap();
                if g.quit { break; }
                g.stats.max_worker_gap_us = g.stats.max_worker_gap_us.max(gap);
                if running {
                    g.stats.min_queued_frames = g.stats.min_queued_frames.min((queued / FRAME_BYTES) as u64);
                    if queued == 0 { g.stats.device_underruns += 1; }
                }
                let MixerInner { streams, stats, .. } = &mut *g;
                for st in streams.iter_mut().flatten() {
                    st.refill(stats);
                    if st.eos {
                        if let Some(child) = st.child.take() { let _ = worker_cleanup.send(child); }
                    }
                }
                // Release between blocks: UI setters only wait for bounded PCM
                // work, never subprocesses or SDL calls.
                drop(g);
                for _ in 0..blocks {
                    let block = {
                        let mut g = inner2.lock().unwrap();
                        mix_once(&mut g, &worker_cleanup)
                    };
                    if crate::sdl2::sdl_queue_audio(dev, &block) {
                        QUEUED.fetch_add(block.len() as u64, Ordering::Relaxed);
                        inner2.lock().unwrap().stats.submitted_frames += BLOCK_FRAMES as u64;
                    } else {
                        inner2.lock().unwrap().stats.queue_errors += 1;
                        break;
                    }
                }
                let filled = crate::sdl2::sdl_queued_audio_size(dev) as u64 / FRAME_BYTES as u64;
                let mut g = inner2.lock().unwrap();
                g.stats.max_queued_frames = g.stats.max_queued_frames.max(filled);
                drop(g);
                // SDL opens paused. Prefill before the first callback can run.
                if !running && filled >= BLOCK_FRAMES as u64 {
                    crate::sdl2::sdl_pause_audio(dev, false);
                    running = true;
                }
                thread::sleep(POLL_INTERVAL);
            }
            crate::sdl2::sdl_pause_audio(dev, true);
            crate::sdl2::sdl_close_audio(dev);
        });
        Some(Mixer { inner, worker: Some(worker), reaper: Some(reaper), cleanup: Some(cleanup) })
    }

    pub fn stats(&self) -> MixerStats {
        let mut stats = self.inner.lock().unwrap().stats;
        if stats.min_queued_frames == u64::MAX { stats.min_queued_frames = 0; }
        stats
    }

    /// Decoder progress for a specific stream (frames, excluding other streams).
    pub fn stream_decoded_frames(&self, idx: usize) -> Option<u64> {
        self.inner.lock().unwrap().streams.get(idx).and_then(|s| s.as_ref())
            .map(|s| s.decoded_samples / CHANNELS as u64)
    }

    pub fn play(&self, input: &str, volume: f32, looping: bool) -> usize {
        self.play_cat(input, volume, looping, CAT_ALL)
    }

    pub fn play_cat(&self, input: &str, volume: f32, looping: bool, cat: usize) -> usize {
        // posix_spawn and resource lookup must not stall an existing BGM.
        let mut s = match Stream::spawn_ffmpeg(input, looping) {
            Some(s) => s, None => {
                self.inner.lock().unwrap().stats.decoder_errors += 1;
                return usize::MAX;
            }
        };
        s.volume = volume;
        s.cat = cat.min(CAT_COUNT - 1);
        let mut g = self.inner.lock().unwrap();
        g.streams.push(Some(s));
        g.streams.len() - 1
    }

    /// Replace interface feedback only, keeping scenario SE and music intact.
    /// A reserved slot bounds retained streams during rapid hover changes. Already
    /// queued device PCM (at most our queue headroom) cannot be selectively removed.
    pub fn play_ui(&self, input: &str, volume: f32) -> usize {
        // Creating a decoder and retiring the previous process happen off-lock.
        let mut stream = match Stream::spawn_ffmpeg(input, false) {
            Some(stream) => stream,
            None => {
                self.inner.lock().unwrap().stats.decoder_errors += 1;
                return usize::MAX;
            }
        };
        stream.volume = volume;
        let (slot, previous) = self.inner.lock().unwrap().replace_ui(stream);
        if let Some(mut previous) = previous {
            if let Some(child) = previous.child.take() {
                let _ = self.cleanup.as_ref().unwrap().send(child);
            }
        }
        slot
    }

    pub fn set_gain(&self, cat: usize, gain: f32) {
        if cat < CAT_COUNT { self.inner.lock().unwrap().gains[cat] = gain.clamp(0.0, 2.0); }
    }

    pub fn gain(&self, cat: usize) -> f32 {
        if cat >= CAT_COUNT { return 1.0; }
        self.inner.lock().unwrap().gains[cat]
    }

    pub fn fade(&self, idx: usize, target: f32, frames: i32, stop: bool) {
        if frames <= 0 && stop { self.stop(idx); return; }
        let mut g = self.inner.lock().unwrap();
        if let Some(Some(s)) = g.streams.get_mut(idx) {
            if frames <= 0 { s.volume = target; s.ramp = None; }
            else { s.ramp = Some((s.volume, target, Instant::now(),
                Duration::from_secs_f64(frames as f64 / 60.0), stop)); }
        }
    }

    pub fn settled(&self, idx: usize) -> bool {
        self.inner.lock().unwrap().streams.get(idx).and_then(|s| s.as_ref())
            .map(|s| s.ramp.is_none()).unwrap_or(true)
    }

    pub fn finished(&self, idx: usize) -> bool {
        self.inner.lock().unwrap().streams.get(idx).and_then(|s| s.as_ref())
            .map(|s| s.eos && s.pcm.is_empty()).unwrap_or(true)
    }

    pub fn stop(&self, idx: usize) {
        let stream = {
            let mut g = self.inner.lock().unwrap();
            g.streams.get_mut(idx).and_then(Option::take)
        };
        if let Some(mut s) = stream {
            if let Some(child) = s.child.take() { let _ = self.cleanup.as_ref().unwrap().send(child); }
        }
    }

    pub fn stop_all(&self) {
        // Keep slot IDs stable: callers can still hold handles for stopped streams.
        let streams: Vec<_> = self.inner.lock().unwrap().streams.iter_mut().map(Option::take).collect();
        for mut st in streams.into_iter().flatten() {
            if let Some(child) = st.child.take() { let _ = self.cleanup.as_ref().unwrap().send(child); }
        }
    }

    pub fn stop_category(&self, cat: usize) {
        let streams: Vec<_> = {
            let mut g = self.inner.lock().unwrap();
            g.streams.iter_mut().filter_map(|s| {
                if s.as_ref().map(|s| s.cat == cat).unwrap_or(false) { s.take() } else { None }
            }).collect()
        };
        for mut st in streams {
            if let Some(child) = st.child.take() { let _ = self.cleanup.as_ref().unwrap().send(child); }
        }
    }
}

impl Drop for Mixer {
    fn drop(&mut self) {
        self.inner.lock().unwrap().quit = true;
        if let Some(h) = self.worker.take() { let _ = h.join(); }
        self.stop_all();
        self.cleanup.take();
        if let Some(h) = self.reaper.take() { let _ = h.join(); }
    }
}

fn mix_once(inner: &mut MixerInner, cleanup: &mpsc::Sender<Child>) -> Vec<u8> {
    let samples = BLOCK_FRAMES * CHANNELS as usize;
    // i64 avoids overflow with category/master gains > 1 or many concurrent SE.
    let mut acc = vec![0i64; samples];
    for st in inner.streams.iter_mut().flatten() {
        if let Some((from, to, start, duration, stop)) = st.ramp {
            let fraction = (start.elapsed().as_secs_f32() / duration.as_secs_f32()).min(1.0);
            st.volume = from + (to - from) * fraction;
            if fraction >= 1.0 {
                st.ramp = None;
                if stop {
                    if let Some(child) = st.child.take() { let _ = cleanup.send(child); }
                    st.looping = false;
                    st.eos = true;
                    st.pcm.clear();
                }
            }
        }
        if !st.started {
            if st.pcm.len() < PREROLL_SAMPLES && !st.eos { continue; }
            if st.pcm.is_empty() { continue; }
            st.started = true;
            inner.stats.started_streams += 1;
        }
        let available = st.pcm.len().min(samples);
        if available < samples && !st.eos {
            inner.stats.decoder_underrun_frames += ((samples - available) / CHANNELS as usize) as u64;
            if !st.starved { inner.stats.decoder_underrun_events += 1; }
            st.starved = true;
        } else { st.starved = false; }
        let gain = inner.gains[CAT_ALL] * if st.cat == CAT_ALL { 1.0 } else { inner.gains[st.cat] };
        let volume = (st.volume * gain * 65536.0) as i64;
        for dst in acc.iter_mut().take(available) {
            *dst += (st.pcm.pop_front().unwrap() as i64 * volume) >> 16;
        }
    }
    let mut out = Vec::with_capacity(BLOCK_BYTES);
    for value in acc { out.extend_from_slice(&(value.clamp(-32768, 32767) as i16).to_le_bytes()); }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::net::UnixStream;

    #[test]
    fn nonblocking_matches_the_native_flag_and_preserves_status() {
        let (mut reader, _writer) = UnixStream::pair().unwrap();
        let fd = reader.as_raw_fd();
        let original = unsafe { fcntl(fd, F_GETFL) };
        assert!(original >= 0);
        reader.set_nonblocking(true).unwrap();
        let native = unsafe { fcntl(fd, F_GETFL) };
        assert_eq!(native, original | O_NONBLOCK);
        reader.set_nonblocking(false).unwrap();
        set_nonblocking(fd).unwrap();
        assert_eq!(unsafe { fcntl(fd, F_GETFL) }, native);
        let mut byte = [0];
        assert_eq!(reader.read(&mut byte).unwrap_err().kind(), std::io::ErrorKind::WouldBlock);
        set_nonblocking(fd).unwrap();
        assert_eq!(unsafe { fcntl(fd, F_GETFL) }, native);
    }

    #[test]
    fn nonblocking_reports_an_invalid_descriptor() { assert!(set_nonblocking(-1).is_err()); }

    fn fixture() -> Stream {
        Stream { cat: CAT_BGM, carry: Vec::new(), child: None, pcm: VecDeque::new(),
            volume: 1.0, looping: false, eos: false, started: false, starved: false,
            decoded_samples: 0, ramp: None }
    }

    #[test]
    fn queue_feedback_survives_ten_minutes_of_work_and_scheduler_jitter() {
        // Model native 48kHz consumption. The old full-period sleep is in
        // addition to 1ms of work/wake latency, so it loses >26 seconds/10min.
        let rate = SAMPLE_RATE as f64 * FRAME_BYTES as f64;
        let mut old_queue = BLOCK_BYTES as f64;
        let mut old_missing = 0.0;
        let old_step = 0.021333 + 0.001;
        for _ in 0..(600.0 / old_step) as usize {
            let consumed = rate * old_step;
            old_missing += (consumed - old_queue).max(0.0);
            old_queue = (old_queue - consumed).max(0.0) + BLOCK_BYTES as f64;
        }
        let mut queue = QUEUE_TARGET_BYTES as f64;
        let mut missing = 0.0;
        let mut elapsed = 0.0;
        let mut tick = 0;
        let mut high_water = queue;
        while elapsed < 600.0 {
            // Include 40ms stalls, twenty times the nominal worker interval.
            let step = if tick % 101 == 0 { 0.040 } else { 0.003 };
            let consumed = rate * step;
            missing += (consumed - queue).max(0.0);
            queue = (queue - consumed).max(0.0);
            queue += (blocks_to_queue(queue.floor() as usize) * BLOCK_BYTES) as f64;
            high_water = high_water.max(queue);
            elapsed += step;
            tick += 1;
        }
        eprintln!("600s model: old_missing_seconds={:.3}, feedback_missing_seconds={:.3}, max_queue_frames={:.0}",
            old_missing / rate, missing / rate, high_water / FRAME_BYTES as f64);
        assert!(old_missing / rate > 26.0);
        assert_eq!(missing, 0.0);
        assert!(high_water <= (QUEUE_TARGET_BYTES + BLOCK_BYTES) as f64);
    }

    #[test]
    fn fragmented_pcm_preserves_every_stereo_frame_and_exact_samples() {
        let mut stream = fixture();
        let source: Vec<u8> = (0..BLOCK_FRAMES * 12).flat_map(|frame| {
            let left = ((frame % 30000) as i16).to_le_bytes();
            let right = (-((frame % 30000) as i16)).to_le_bytes();
            [left[0], left[1], right[0], right[1]]
        }).collect();
        for bytes in source.chunks(101) {
            stream.append_pcm(bytes);
            assert_eq!(stream.pcm.len() % CHANNELS as usize, 0);
        }
        stream.eos = true;
        let mut inner = MixerInner::new();
        inner.streams.push(Some(stream));
        let (tx, _rx) = mpsc::channel();
        let mut mixed = Vec::new();
        for _ in 0..12 { mixed.extend(mix_once(&mut inner, &tx)); }
        assert_eq!(mixed, source);
        assert_eq!(inner.stats.started_streams, 1);
        assert_eq!(inner.stats.decoder_underrun_frames, 0);
    }

    #[test]
    fn preroll_eof_and_real_underflow_are_distinguished() {
        let mut inner = MixerInner::new();
        let mut stream = fixture();
        stream.pcm.extend(std::iter::repeat_n(1000, 100));
        inner.streams.push(Some(stream));
        let (tx, _rx) = mpsc::channel();
        assert!(mix_once(&mut inner, &tx).iter().all(|b| *b == 0));
        assert_eq!(inner.streams[0].as_ref().unwrap().pcm.len(), 100);
        assert_eq!(inner.stats.decoder_underrun_frames, 0); // waiting for startup
        inner.streams[0].as_mut().unwrap().started = true;
        mix_once(&mut inner, &tx);
        assert_eq!(inner.stats.decoder_underrun_frames, 974);
        assert_eq!(inner.stats.decoder_underrun_events, 1);
        mix_once(&mut inner, &tx);
        assert_eq!(inner.stats.decoder_underrun_frames, 1998);
        assert_eq!(inner.stats.decoder_underrun_events, 1); // one prolonged event
        inner.streams[0].as_mut().unwrap().eos = true;
        mix_once(&mut inner, &tx);
        assert_eq!(inner.stats.decoder_underrun_frames, 1998); // normal EOF padding
        let mut short = fixture();
        short.pcm.extend([1234, -1234]);
        short.eos = true;
        inner.streams[0] = Some(short);
        let bytes = mix_once(&mut inner, &tx);
        assert_eq!(&bytes[..4], &[0xd2, 0x04, 0x2e, 0xfb]); // short sounds not lost
    }

    #[test]
    fn ui_replacement_is_bounded_and_never_replaces_scenario_audio() {
        let mut inner = MixerInner::new();
        let mut story = fixture();
        story.cat = CAT_SE;
        story.pcm.extend([321, -321]);
        inner.streams.push(Some(story));
        inner.streams.push(Some(fixture())); // BGM
        let (slot, previous) = inner.replace_ui(fixture());
        assert_eq!(slot, 2);
        assert!(previous.is_none());
        for value in 0..1000 {
            let mut next = fixture();
            next.pcm.extend([value, value]);
            let (same_slot, previous) = inner.replace_ui(next);
            assert_eq!(same_slot, slot);
            assert!(previous.is_some());
            assert_eq!(inner.streams.len(), 3);
        }
        assert_eq!(inner.streams[0].as_ref().unwrap().pcm.iter().copied().collect::<Vec<_>>(), [321, -321]);
        assert_eq!(inner.streams[1].as_ref().unwrap().cat, CAT_BGM);
        assert_eq!(inner.streams[slot].as_ref().unwrap().cat, CAT_SE);
        // stop(), stop_all(), and stop_category() leave the reserved slot reusable.
        inner.streams[slot] = None;
        let (same_slot, previous) = inner.replace_ui(fixture());
        assert_eq!(same_slot, slot);
        assert!(previous.is_none());
        assert_eq!(inner.streams.len(), 3);
    }

    #[test]
    fn replacing_ui_discards_old_pcm_without_summing_it_into_new_feedback() {
        let mut inner = MixerInner::new();
        let mut previous = fixture();
        previous.eos = true;
        previous.pcm.extend(std::iter::repeat_n(30_000, BLOCK_FRAMES * 2));
        inner.replace_ui(previous);
        let mut next = fixture();
        next.eos = true;
        next.pcm.extend(std::iter::repeat_n(1000, BLOCK_FRAMES * 2));
        inner.replace_ui(next);
        let (tx, _rx) = mpsc::channel();
        assert!(mix_once(&mut inner, &tx).chunks_exact(2)
            .all(|c| i16::from_le_bytes([c[0], c[1]]) == 1000));
    }

    #[test]
    fn loud_overlap_clips_without_integer_overflow_and_master_is_applied_once() {
        let mut inner = MixerInner::new();
        inner.gains[CAT_ALL] = 2.0;
        inner.gains[CAT_SE] = 2.0;
        for _ in 0..16 {
            let mut s = fixture();
            s.cat = CAT_SE;
            s.started = true;
            s.pcm.extend(std::iter::repeat_n(i16::MAX, BLOCK_FRAMES * 2));
            inner.streams.push(Some(s));
        }
        let (tx, _rx) = mpsc::channel();
        assert!(mix_once(&mut inner, &tx).chunks_exact(2).all(|c| i16::from_le_bytes([c[0], c[1]]) == i16::MAX));
        inner.streams.clear();
        let mut s = fixture();
        s.cat = CAT_ALL;
        s.started = true;
        s.pcm.extend(std::iter::repeat_n(1000, BLOCK_FRAMES * 2));
        inner.streams.push(Some(s));
        assert!(mix_once(&mut inner, &tx).chunks_exact(2).all(|c| i16::from_le_bytes([c[0], c[1]]) == 2000));
    }
}
