//! Adapt stored program and recovered table assets to the application compiler.
use crate::prepared::ControlMap;
use radias_synth_application::stored_program::ProgramFilterTables;
pub use radias_synth_application::stored_program::{CompiledProgram, CompiledTimbre};
use radias_synth_domain::{
    filter::FilterCoefficients, filter_control::FilterMixTable, program::Program,
};
/// Prepare the native vocoder outside the callback. Current source zero is
/// the cold patch's modulation input; subsequent source changes use its port.
pub fn compile_vocoder(
    program: &CompiledProgram,
) -> Result<Option<Box<radias_synth_application::vocoder::VocoderRenderer>>, String> {
    let stored = radias_synth_domain::vocoder_control::VocoderProgram {
        bytes: &program.vocoder,
    };
    if !stored.enabled() {
        return Ok(None);
    }
    let inputs = radias_synth_domain::vocoder_control::VocoderControlInputs {
        carrier_flags: program.carrier_common_flags[stored.carrier_timbre()],
        frequency_source: 0,
    };
    radias_synth_application::vocoder::VocoderRenderer::from_program(
        stored,
        inputs,
        &crate::vocoder_tables::original(),
        crate::vocoder_tables::interpolation(),
    )
    .map(|renderer| Some(Box::new(renderer)))
    .map_err(|e| format!("Native vocoder compilation failed: {e:?}"))
}
pub fn compile_program(
    program: &Program,
    global_channel: u8,
    controls: &ControlMap,
    mix: &FilterMixTable,
    base: FilterCoefficients,
) -> Result<CompiledProgram, String> {
    CompiledProgram::compile(
        program,
        global_channel,
        &ProgramFilterTables {
            frequencies: &controls.frequencies,
            resonances: &controls.resonances,
            input_gains: &controls.input_gains,
            normalization: controls.normalization,
            mix,
        },
        base,
    )
    .map_err(|e| format!("Stored program compilation failed: {e:?}"))
}

pub fn compile_drum_kit(
    program: &Program,
    kit: radias_synth_domain::drum::DrumKit,
    controls: &ControlMap,
    mix: &FilterMixTable,
    base: FilterCoefficients,
) -> Result<radias_synth_application::drum_program::CompiledDrumKit, String> {
    radias_synth_application::drum_program::CompiledDrumKit::compile(
        program,
        kit,
        &ProgramFilterTables {
            frequencies: &controls.frequencies,
            resonances: &controls.resonances,
            input_gains: &controls.input_gains,
            normalization: controls.normalization,
            mix,
        },
        base,
    )
    .map_err(|e| format!("Drum-kit compilation failed: {e:?}"))
}
