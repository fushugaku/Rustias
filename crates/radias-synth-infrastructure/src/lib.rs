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

pub mod standalone;
pub mod synthesizer;

pub mod effect_program_buffers;
pub mod effect_delay_memory;
pub mod effects;
pub mod effect_audio;
pub mod timbre_output;
pub mod standalone_tables;

#[cfg(feature = "web-modular")]
pub mod circuit;

pub mod vocoder_tables;
mod vocoder_tables_data;

#[cfg(test)]
pub(crate) fn reference_root()->std::path::PathBuf {
    std::env::var_os("RADIAS_REFERENCE_ROOT").map(std::path::PathBuf::from)
        .unwrap_or_else(||std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../.."))
}
