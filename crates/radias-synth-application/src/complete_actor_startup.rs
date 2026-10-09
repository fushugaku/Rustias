//! Atomic complete startup publication; controller/active mask share ownership.
use crate::{dsp_transport::SendQueueError, synthesis_transport::SynthesisParameterTransport};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    complete_actor_startup::{CompleteStartupError, CompleteStartupTables},
};
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum CompleteStartupPublicationError {
    Compile(CompleteStartupError),
    Transport(SendQueueError),
}
pub fn start_complete_actor(
    clock: u64,
    slot: usize,
    body: &[u8; 104],
    controller: &mut ActorControlState,
    tables: &CompleteStartupTables<'_>,
    transport: &mut SynthesisParameterTransport,
) -> Result<(), CompleteStartupPublicationError> {
    let compiled = controller
        .compile_complete_startup(0, slot, body, transport.actor_lifecycle(), tables)
        .map_err(CompleteStartupPublicationError::Compile)?;
    transport
        .publish_actor_descriptors(clock, slot, &compiled.publication)
        .map_err(CompleteStartupPublicationError::Transport)?;
    *controller = compiled.controller;
    transport.set_actor_lifecycle(compiled.lifecycle);
    Ok(())
}
