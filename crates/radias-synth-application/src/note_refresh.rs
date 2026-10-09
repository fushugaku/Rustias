//! Atomic complete note refresh on the owned synthesis transport.
use crate::{dsp_transport::SendQueueError, synthesis_transport::SynthesisParameterTransport};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    note_refresh::{NoteRefreshError, NoteRefreshRequest, NoteRefreshTables},
};
pub struct NoteRefreshPublication<'a> {
    pub clock: u64,
    pub slot: usize,
    pub controls: NoteRefreshRequest<'a>,
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum NoteRefreshPublicationError {
    Compile(NoteRefreshError),
    Transport(SendQueueError),
}
pub fn refresh_note_controls(
    request: NoteRefreshPublication<'_>,
    controller: &mut ActorControlState,
    tables: &NoteRefreshTables<'_>,
    transport: &mut SynthesisParameterTransport,
) -> Result<(), NoteRefreshPublicationError> {
    let refreshed = controller
        .compile_note_refresh(request.controls, tables)
        .map_err(NoteRefreshPublicationError::Compile)?;
    transport
        .publish_actor_descriptors(request.clock, request.slot, &refreshed.publication)
        .map_err(NoteRefreshPublicationError::Transport)?;
    *controller = refreshed.controller;
    Ok(())
}
