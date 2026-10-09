//! Complete SYS014f40 note refresh, with live Virtual Patch publication plan.
use crate::{
    actor_amplifier_preparation::{ActorAmplifierPorts, InvalidUnisonGainBank},
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorPlan,
    actor_envelope_initialization::ActorEnvelope,
    actor_filter_preparation::FilterPreparation,
    actor_lfo_initialization::{ActorLfo, ActorLfoState, InvalidLfoShape},
    actor_pitch_preparation::ActorPitchPorts,
    actor_virtual_patch::ActorVirtualPatchPorts,
    lfo::LfoTables,
    modulation::{ModulationTables, ModulationTargets},
    virtual_patch_live::{
        LiveCompilationError, LiveCompilerPorts, LiveCompilerTables, LiveVirtualPatchRequest,
    },
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct NoteRefreshPorts {
    pub virtual_patch: ActorVirtualPatchPorts,
    pub live_compilers: LiveCompilerPorts,
    pub pitch: ActorPitchPorts,
    pub amplifier: ActorAmplifierPorts,
}
#[derive(Clone, Copy)]
pub struct NoteRefreshRequest<'a> {
    pub body: &'a [u8; 104],
    pub lfos: [ActorLfoState; 2],
    pub ports: NoteRefreshPorts,
}
pub struct NoteRefreshTables<'a> {
    pub lfo: &'a LfoTables,
    pub compilers: &'a LiveCompilerTables<'a>,
    pub modulation: &'a ModulationTables,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteRefreshError {
    LfoShape(InvalidLfoShape),
    GainBank(InvalidUnisonGainBank),
    Live(LiveCompilationError),
}
pub struct RefreshedNoteControls {
    pub controller: ActorControlState,
    pub modulations: ModulationTargets,
    pub publication: DescriptorPlan,
}
impl ActorControlState {
    /// Recompute note identity/EG/LFO before live modulation, then initial
    /// pitch/primary/filter/AMP caches. This does not advance EG or LFO phase.
    pub fn compile_note_refresh(
        &self,
        request: NoteRefreshRequest<'_>,
        tables: &NoteRefreshTables<'_>,
    ) -> Result<RefreshedNoteControls, NoteRefreshError> {
        let mut controller = *self;
        // Wrapper has32 clocks before the VP call and36 after its return.
        let mut before = 32 + controller.prepare_note_identity();
        for eg in [
            ActorEnvelope::Filter,
            ActorEnvelope::Amplifier,
            ActorEnvelope::Modulation,
        ] {
            before += controller.publish_envelope_level(eg);
        }
        for (index, lfo) in [ActorLfo::First, ActorLfo::Second].into_iter().enumerate() {
            before += controller
                .publish_lfo_level(lfo, request.lfos[index], request.body, tables.lfo)
                .map_err(NoteRefreshError::LfoShape)?
                .controller_clocks;
        }
        let live = controller
            .compile_live_virtual_patches(
                LiveVirtualPatchRequest {
                    body: request.body,
                    sources: request.ports.virtual_patch,
                    compilers: request.ports.live_compilers,
                },
                tables.compilers,
                tables.modulation,
            )
            .map_err(NoteRefreshError::Live)?;
        controller = live.controller;
        let mut after = 36
            + controller
                .prepare_primary_pitch(request.body, request.ports.pitch, tables.compilers.fine)
                .controller_clocks;
        after += controller.primary_preparation_clocks();
        controller.refresh_primary();
        for service in [
            FilterPreparation::FirstKey,
            FilterPreparation::FirstFrequency,
            FilterPreparation::SecondKey,
            FilterPreparation::SecondFrequency,
        ] {
            after += controller.prepare_filter_control(service, request.body, tables.compilers);
        }
        after += controller.prepare_amplifier_key(request.body, tables.compilers.amplifier);
        after += controller
            .prepare_amplifier_target(
                request.body,
                request.ports.amplifier,
                tables.compilers.amplifier,
            )
            .map_err(NoteRefreshError::GainBank)?;
        let mut publication = DescriptorPlan::default();
        publication.work(before);
        publication.append_compacted(&live.publication);
        let mut finish = DescriptorPlan::default();
        finish.work(after);
        publication.append_compacted(&finish);
        Ok(RefreshedNoteControls {
            controller,
            modulations: live.targets,
            publication,
        })
    }
}
