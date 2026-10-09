//! Commit a complete manual callback and its DSP publications atomically.
use crate::{dsp_transport::SendQueueError, synthesis_transport::SynthesisParameterTransport};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    manual_parameters::{ManualCompilationError, ManualCompilerPorts, ManualCompilerTables},
};

pub struct ManualParameterRequest<'a> {
    pub clock: u64,
    pub slot: usize,
    pub parameter: u8,
    pub value: i16,
    pub body: &'a [u8; 104],
    pub ports: ManualCompilerPorts,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ManualParameterError {
    Compile(ManualCompilationError),
    Transport(SendQueueError),
}

pub fn apply_manual_parameter(
    request: ManualParameterRequest<'_>,
    controller: &mut ActorControlState,
    tables: &ManualCompilerTables<'_>,
    transport: &mut SynthesisParameterTransport,
) -> Result<(), ManualParameterError> {
    let compiled = controller
        .compile_manual_parameter(
            request.parameter,
            request.value,
            request.body,
            request.ports,
            tables,
        )
        .map_err(ManualParameterError::Compile)?;
    transport
        .publish_actor_descriptors(request.clock, request.slot, &compiled.publication)
        .map_err(ManualParameterError::Transport)?;
    *controller = compiled.controller;
    Ok(())
}
