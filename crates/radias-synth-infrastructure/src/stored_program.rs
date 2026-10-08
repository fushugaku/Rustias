//! Adapt stored program and recovered table assets to the application compiler.
use crate::prepared::ControlMap;
use radias_synth_application::stored_program::ProgramFilterTables;
pub use radias_synth_application::stored_program::{CompiledProgram, CompiledTimbre};
use radias_synth_domain::{
    filter::FilterCoefficients, filter_control::FilterMixTable, program::Program,
};
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
