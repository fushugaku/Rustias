//! Firmware data and audio/reference adapters for the standalone synthesis core.
#[cfg(feature = "desktop-io")]
pub mod audio;
pub mod firmware;
pub mod oracle;
pub mod prepared;
pub mod program;
pub mod rdl;
pub mod stored_program;
pub mod wav;
