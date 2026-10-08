//! File, diagnostic protocol and audio/MIDI adapters; no synthesis arithmetic.
#![recursion_limit = "512"]
pub mod artifacts;
#[cfg(feature = "desktop-io")]
pub mod audio;
pub mod capture;
pub mod diagnostics;
pub mod flash;
#[cfg(feature = "desktop-io")]
pub mod midi;
pub mod pcm;
pub mod state;
pub mod wav;
