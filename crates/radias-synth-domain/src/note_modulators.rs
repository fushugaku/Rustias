//! Whole SYS0150d8 use case: prior modulation, EG1–3, then LFO1–2.
use crate::{
    actor_control_state::ActorControlState,
    actor_envelope_initialization::ActorEnvelope,
    actor_lfo_initialization::ActorLfoState,
    actor_virtual_patch::{ActorVirtualPatchError, ActorVirtualPatchPorts},
    amplifier_control::AmplifierTables,
    envelope_segment::{EnvelopeCurves, EnvelopeTimingTables},
    modulation::{ModulationTables, ModulationTargets},
};

pub struct NoteModulatorRequest<'a> {
    pub controller: ActorControlState,
    pub lfos: [ActorLfoState; 2],
    pub shared_lfos: [ActorLfoState; 2],
    pub random_seed: u16,
    pub body: &'a [u8; 104],
    pub ports: ActorVirtualPatchPorts,
}
pub struct NoteModulatorTables<'a> {
    pub curves: &'a EnvelopeCurves,
    pub timing: &'a EnvelopeTimingTables,
    pub amplifier: &'a AmplifierTables,
    pub modulation: &'a ModulationTables,
}
pub struct InitializedNoteModulators {
    pub controller: ActorControlState,
    pub lfos: [ActorLfoState; 2],
    pub random_seed: u16,
    pub modulations: ModulationTargets,
    pub controller_clocks: u16,
}

/// Inputs are owned candidates: a rejected route commits no voice or PRNG state.
/// Publishing EG levels and compiling initial LFO rates are separate services.
pub fn initialize_note_modulators(
    request: NoteModulatorRequest<'_>,
    tables: &NoteModulatorTables<'_>,
) -> Result<InitializedNoteModulators, ActorVirtualPatchError> {
    let mut controller = request.controller;
    let (modulations, work) = controller.prepare_virtual_patches_with_work(
        request.body,
        request.ports,
        tables.amplifier,
        tables.modulation,
    )?;
    // 35 wrapper instructions, six delayed calls and one delayed return.
    let mut controller_clocks = 42 + work.total();
    for envelope in [
        ActorEnvelope::Filter,
        ActorEnvelope::Amplifier,
        ActorEnvelope::Modulation,
    ] {
        controller_clocks += controller
            .initialize_envelope(envelope, request.body, tables.curves, tables.timing)
            .controller_clocks;
    }
    let mut lfos = request.lfos;
    let mut random_seed = request.random_seed;
    for (index, lfo) in lfos.iter_mut().enumerate() {
        controller_clocks += lfo.initialize_note(
            request.body[79 + 5 * index],
            request.body[80 + 5 * index],
            request.shared_lfos[index],
            &mut random_seed,
        );
    }
    Ok(InitializedNoteModulators {
        controller,
        lfos,
        random_seed,
        modulations,
        controller_clocks,
    })
}
