//! Atomic note preparation and shared PRNG ownership; no DSP publication here.
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    actor_note_initialization::{ActorNoteInitializationPorts, ActorNoteInitializationTables},
    actor_virtual_patch::ActorVirtualPatchError,
    modulation::ModulationTargets,
};
pub struct ActorNoteInitializationRequest<'a> {
    pub body: &'a [u8; 104],
    pub ports: ActorNoteInitializationPorts,
}
pub struct PreparedActorNote {
    pub modulations: ModulationTargets,
    pub controller_clocks: u16,
}
pub fn initialize_actor_note(
    request: ActorNoteInitializationRequest<'_>,
    controller: &mut ActorControlState,
    random_seed: &mut u16,
    tables: &ActorNoteInitializationTables<'_>,
) -> Result<PreparedActorNote, ActorVirtualPatchError> {
    let initialized =
        controller.initialize_actor_note(request.body, request.ports, *random_seed, tables)?;
    *controller = initialized.controller;
    *random_seed = initialized.random_seed;
    Ok(PreparedActorNote {
        modulations: initialized.modulations,
        controller_clocks: initialized.controller_clocks,
    })
}
