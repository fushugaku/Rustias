use crate::{
    effect_transition_queue::{
        EffectProgramSource, EffectPublicationError, EffectQueueServiceInputs,
    },
    effects::EffectProgramPort,
    master_assignment_release::release_master_assignments_with_service,
};
use radias_synth_domain::{
    effect_rack_rebuild::{EffectRackRebuildContext, EffectRackRebuildTables},
    effect_transition_queue::EffectTransitionQueue,
    insert_effect_control::InsertControlState,
    master_pressure_type_change::MasterPressureTypeChangeOperation,
    master_pressure_type_change::{MasterPressureTypeError, PreparedMasterPressureTypeChange},
};
pub struct MasterPressureTypeChangeIo<'a, F, P, S, C> {
    pub queue: &'a mut EffectTransitionQueue,
    pub effects: &'a mut F,
    pub rebuild: &'a mut P,
    /// Program contents before reconstruction; pending old uploads must read
    /// these buffers while the optional assignment release frees queue space.
    pub source: &'a S,
    pub inputs: &'a mut C,
}
#[derive(Debug, PartialEq, Eq)]
pub enum MasterPressureStreamingError<F, P> {
    Publication(EffectPublicationError<F>),
    Rebuild(MasterPressureTypeChangeError<P>),
}
pub fn rebuild_master_type_with_queue_service<
    F: EffectProgramPort,
    P: MasterPressureTypeChangePort,
    S: EffectProgramSource,
    C: EffectQueueServiceInputs,
>(
    state: &mut InsertControlState,
    pressure_marker: &mut u32,
    operation: &mut MasterPressureTypeChangeOperation,
    io: &mut MasterPressureTypeChangeIo<'_, F, P, S, C>,
    tables: &EffectRackRebuildTables,
    context: EffectRackRebuildContext<'_>,
) -> Result<(), MasterPressureStreamingError<F::Error, P::Error>> {
    if operation.is_complete() {
        return Ok(());
    }
    if let Some(release) = operation.pending_release() {
        release_master_assignments_with_service(
            &mut state.midi.master.control,
            release,
            io.queue,
            io.effects,
            io.source,
            io.inputs,
        )
        .map_err(MasterPressureStreamingError::Publication)?;
    }
    operation.finish_prefix();
    let context = EffectRackRebuildContext {
        queue: io.queue.state(),
        ..context
    };
    rebuild_master_type_under_pressure(state, pressure_marker, io.rebuild, tables, context)
        .map_err(MasterPressureStreamingError::Rebuild)?;
    operation.finish();
    Ok(())
}
pub trait MasterPressureTypeChangePort {
    type Error;
    fn accept_master_pressure_type_change(
        &mut self,
        prepared: &PreparedMasterPressureTypeChange,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum MasterPressureTypeChangeError<E> {
    Preparation(MasterPressureTypeError),
    Port(E),
}
pub fn rebuild_master_type_under_pressure<P: MasterPressureTypeChangePort>(
    state: &mut InsertControlState,
    pressure_marker: &mut u32,
    port: &mut P,
    tables: &EffectRackRebuildTables,
    context: EffectRackRebuildContext<'_>,
) -> Result<(), MasterPressureTypeChangeError<P::Error>> {
    let prepared = tables
        .prepare_master_pressure_type_change(state, context)
        .map_err(MasterPressureTypeChangeError::Preparation)?;
    port.accept_master_pressure_type_change(&prepared)
        .map_err(MasterPressureTypeChangeError::Port)?;
    *state = prepared.rebuild.next;
    *pressure_marker = prepared.pressure_marker;
    Ok(())
}
