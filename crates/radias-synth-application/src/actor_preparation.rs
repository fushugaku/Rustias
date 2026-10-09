//! Prepare controller shadows and publish descriptors on the shared DSP FIFO.
use crate::{dsp_transport::SendQueueError, synthesis_transport::SynthesisParameterTransport};
use radias_synth_domain::{
    actor_control_state::ActorControlState,
    actor_descriptors::DescriptorPlan,
    actor_virtual_patch::{ActorVirtualPatchError, ActorVirtualPatchPorts},
    amplifier_control::AmplifierTables,
    modulation::{ModulationTables, ModulationTargets},
    parameter_template::{ParameterTemplateTables, TemplateCompilationError},
};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ActorPreparationError {
    Compile(TemplateCompilationError),
    Publish(SendQueueError),
    VirtualPatch(ActorVirtualPatchError),
}

pub struct ActorPreparationRequest<'a> {
    pub clock: u64,
    pub slot: usize,
    pub body: &'a [u8; 104],
    pub owner_mode: u8,
    pub ports: ActorVirtualPatchPorts,
}
pub struct ActorPreparationTables<'a> {
    pub amplifier: &'a AmplifierTables,
    pub modulation: &'a ModulationTables,
    pub descriptors: &'a ParameterTemplateTables,
}

/// Three complete preparation/descriptor calls on the common transport.
/// This is a component sequence; the actual full note constructor remains
/// responsible for its other services, physical state and interrupt ordering.
pub fn prepare_virtual_patches_and_publish_descriptors(
    request: ActorPreparationRequest<'_>,
    controller: &mut ActorControlState,
    tables: ActorPreparationTables<'_>,
    transport: &mut SynthesisParameterTransport,
) -> Result<(), ActorPreparationError> {
    let prepared = prepare_primary_and_virtual_patches(
        controller,
        request.body,
        request.owner_mode,
        request.ports,
        tables.amplifier,
        tables.modulation,
    )
    .map_err(ActorPreparationError::VirtualPatch)?;
    let plan = DescriptorPlan::compile(
        12,
        request.body,
        prepared.controller.descriptor_cache(),
        tables.descriptors,
    )
    .map_err(ActorPreparationError::Compile)?
    .with_preparation_work(prepared.primary_work_clocks + prepared.virtual_patch_work.total());
    transport
        .publish_actor_descriptors(request.clock, request.slot, &plan)
        .map_err(ActorPreparationError::Publish)?;
    *controller = prepared.controller;
    Ok(())
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PreparedActorControls {
    pub controller: ActorControlState,
    pub modulations: ModulationTargets,
    pub virtual_patch_work: radias_synth_domain::virtual_patch_work::VirtualPatchWork,
    pub primary_work_clocks: u16,
}

/// Whole SYS01e968 then SYS021832(r0=1), without live compiler/HPI effects.
/// Other note services and caller timing are assembled separately.
pub fn prepare_primary_and_virtual_patches(
    prior: &ActorControlState,
    body: &[u8; 104],
    owner_mode: u8,
    ports: ActorVirtualPatchPorts,
    amplifier: &AmplifierTables,
    tables: &ModulationTables,
) -> Result<PreparedActorControls, ActorVirtualPatchError> {
    let mut controller = *prior;
    controller.prepare_from_body(body, owner_mode);
    let primary_work_clocks = 45 + controller.primary_preparation_clocks();
    let (modulations, virtual_patch_work) =
        controller.prepare_virtual_patches_with_work(body, ports, amplifier, tables)?;
    Ok(PreparedActorControls {
        controller,
        modulations,
        virtual_patch_work,
        primary_work_clocks,
    })
}

/// Whole SYS01e968 preparation followed by SYS01e9e4 descriptor publication.
/// Other controller inputs remain owned by upstream note/modulation services.
/// Rejecting a request leaves both prior shadows and the FIFO unchanged.
pub fn prepare_actor_descriptors(
    clock: u64,
    slot: usize,
    body: &[u8; 104],
    owner_mode: u8,
    controller: &mut ActorControlState,
    tables: &ParameterTemplateTables,
    transport: &mut SynthesisParameterTransport,
) -> Result<(), ActorPreparationError> {
    let mut prepared = *controller;
    prepared.prepare_from_body(body, owner_mode);
    let plan = DescriptorPlan::compile(12, body, prepared.descriptor_cache(), tables)
        .map_err(ActorPreparationError::Compile)?
        .with_preparation_work(45 + prepared.primary_preparation_clocks());
    transport
        .publish_actor_descriptors(clock, slot, &plan)
        .map_err(ActorPreparationError::Publish)?;
    *controller = prepared;
    Ok(())
}
