//! Audio decoding and playback.
//!
//! Streams come from LAC archives (raw Ogg Vorbis) or ASF movies (WMA).
//! Decoding is delegated to `ffmpeg` (raw s16le PCM over a pipe); mixing and
//! output go through SDL2 — see `mixer`.

pub mod mixer;
pub mod acceptance;
pub use mixer::Mixer;