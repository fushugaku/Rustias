//! Publish a complete note-preparation candidate and commit its shared state.
use crate::{dsp_transport::SendQueueError, synthesis_transport::SynthesisParameterTransport};
use radias_synth_domain::actor_note_preparation::{
    ActorNotePreparationError, ActorNotePreparationRequest, ActorNotePreparationState,
    ActorNotePreparationTables,
};
pub struct ActorNotePreparationPublication<'a> {
    pub clock: u64,
    pub slot: usize,
    pub controls: ActorNotePreparationRequest<'a>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorNotePreparationPublicationError {
    Compile(ActorNotePreparationError),
    Transport(SendQueueError),
}
pub fn prepare_actor_note(
    request: ActorNotePreparationPublication<'_>,
    state: &mut ActorNotePreparationState,
    tables: &ActorNotePreparationTables<'_>,
    transport: &mut SynthesisParameterTransport,
) -> Result<(), ActorNotePreparationPublicationError> {
    let prepared = state
        .prepare_note_controls(request.controls, tables)
        .map_err(ActorNotePreparationPublicationError::Compile)?;
    transport
        .publish_actor_descriptors(request.clock, request.slot, &prepared.publication)
        .map_err(ActorNotePreparationPublicationError::Transport)?;
    *state = prepared.state;
    Ok(())
}
