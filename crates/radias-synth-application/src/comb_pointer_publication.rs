//! Atomically commit a Comb pointer publication on the common synthesis FIFO.
use crate::{dsp_transport::SendQueueError, synthesis_transport::SynthesisParameterTransport};
use radias_synth_domain::{
    actor_control_state::ActorControlState, comb_pointer_publication::InvalidCombControllerSlot,
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CombPointerPublicationError {
    Compile(InvalidCombControllerSlot),
    Transport(SendQueueError),
}
pub fn publish_comb_pointers(
    clock: u64,
    slot: usize,
    controller: &mut ActorControlState,
    transport: &mut SynthesisParameterTransport,
) -> Result<(), CombPointerPublicationError> {
    let compiled = controller
        .compile_comb_pointer_publication()
        .map_err(CombPointerPublicationError::Compile)?;
    transport
        .publish_actor_descriptors(clock, slot, &compiled.publication)
        .map_err(CombPointerPublicationError::Transport)?;
    *controller = compiled.controller;
    Ok(())
}
