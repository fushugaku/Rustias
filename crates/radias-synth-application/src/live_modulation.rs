//! Compile and publish a live modulation destination on the instrument FIFO.
use crate::{dsp_transport::SendQueueError, synthesis_transport::SynthesisParameterTransport};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    virtual_patch_live::{
        LiveCompilationError, LiveCompilerTables, LiveDestinationRequest, LiveDestinationUpdate,
        LiveVirtualPatchRequest,
    },
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum LiveModulationError {
    Compile(LiveCompilationError),
    Publish(SendQueueError),
}

pub fn publish_live_virtual_patches(
    clock: u64,
    slot: usize,
    controller: &mut ActorControlState,
    request: LiveVirtualPatchRequest<'_>,
    tables: &LiveCompilerTables<'_>,
    modulation: &radias_synth_domain::modulation::ModulationTables,
    transport: &mut SynthesisParameterTransport,
) -> Result<radias_synth_domain::modulation::ModulationTargets, LiveModulationError> {
    let compiled = controller
        .compile_live_virtual_patches(request, tables, modulation)
        .map_err(LiveModulationError::Compile)?;
    transport
        .publish_actor_descriptors(clock, slot, &compiled.publication)
        .map_err(LiveModulationError::Publish)?;
    *controller = compiled.controller;
    Ok(compiled.targets)
}

/// Calculation and publication are one transaction. A rejected compiler,
/// missing receiver context or full FIFO preserves the previous controller.
pub fn publish_live_destination(
    clock: u64,
    slot: usize,
    controller: &mut ActorControlState,
    request: LiveDestinationRequest<'_>,
    tables: &LiveCompilerTables<'_>,
    transport: &mut SynthesisParameterTransport,
) -> Result<LiveDestinationUpdate, LiveModulationError> {
    let compiled = controller
        .compile_live_destination(request, tables)
        .map_err(LiveModulationError::Compile)?;
    transport
        .publish_actor_descriptors(clock, slot, &compiled.publication)
        .map_err(LiveModulationError::Publish)?;
    *controller = compiled.controller;
    Ok(compiled.update)
}
