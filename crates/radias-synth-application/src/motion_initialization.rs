//! Atomically initialize the owned note actor and shared motion tracks.
use crate::{dsp_transport::SendQueueError, synthesis_transport::SynthesisParameterTransport};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    manual_parameters::{ManualCompilerPorts, ManualCompilerTables},
    motion_initialization::{MotionControlState, MotionInitializationError, MotionNoteRequest},
};

pub struct MotionNotePublication<'a> {
    pub clock: u64,
    pub slot: usize,
    pub controls: MotionNoteRequest<'a>,
}

pub struct MotionAssignmentPublication<'a> {
    pub clock: u64,
    pub slot: usize,
    pub assignment: u8,
    pub track: u8,
    pub body: &'a [u8; 104],
    pub ports: ManualCompilerPorts,
}

pub fn initialize_motion_assignment(
    request: MotionAssignmentPublication<'_>,
    controller: &mut ActorControlState,
    motion: &mut MotionControlState,
    tables: &ManualCompilerTables<'_>,
    transport: &mut SynthesisParameterTransport,
) -> Result<(), MotionNotePublicationError> {
    let initialized = controller
        .initialize_motion_assignment(
            request.assignment,
            request.track,
            *motion,
            request.body,
            request.ports,
            tables,
        )
        .map_err(MotionNotePublicationError::Compile)?;
    transport
        .publish_actor_descriptors(request.clock, request.slot, &initialized.publication)
        .map_err(MotionNotePublicationError::Transport)?;
    *controller = initialized.controller;
    *motion = initialized.motion;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MotionNotePublicationError {
    Compile(MotionInitializationError),
    Transport(SendQueueError),
}

pub fn initialize_note_motion(
    request: MotionNotePublication<'_>,
    controller: &mut ActorControlState,
    motion: &mut MotionControlState,
    tables: &ManualCompilerTables<'_>,
    transport: &mut SynthesisParameterTransport,
) -> Result<(), MotionNotePublicationError> {
    let initialized = controller
        .initialize_motion_note(request.controls, *motion, tables)
        .map_err(MotionNotePublicationError::Compile)?;
    transport
        .publish_actor_descriptors(request.clock, request.slot, &initialized.publication)
        .map_err(MotionNotePublicationError::Transport)?;
    *controller = initialized.controller;
    *motion = initialized.motion;
    Ok(())
}
