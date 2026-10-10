use crate::{
    effect_transition_queue::{EffectProgramSource, EffectQueueServiceInputs},
    effects::EffectProgramPort,
    master_pressure_type_change::{
        MasterPressureStreamingError, MasterPressureTypeChangeIo, MasterPressureTypeChangePort,
        rebuild_master_type_with_queue_service,
    },
};
use radias_synth_domain::{
    effect_property::{EffectProperty, EffectPropertyChange, EffectPropertyTables},
    effect_rack_initialization::EffectRackInitializationContext,
    effect_rack_rebuild::{EffectRackRebuildContext, EffectRackRebuildTables},
    insert_effect_control::{InsertControlContext, InsertControlState},
    master_pressure_type_value::{
        MasterPressureTypeValueOperation, MasterPressureTypeValueRequest,
    },
    mixed_effect_parameter::EffectParameterTarget,
    program::Program,
};

pub struct MasterPressureTypeValueTables<'a> {
    pub properties: &'a EffectPropertyTables,
    pub rebuild: &'a EffectRackRebuildTables,
}
#[derive(Debug, PartialEq, Eq)]
pub enum MasterPressureTypeValueError<F, P> {
    InvalidProperty,
    Pressure(MasterPressureStreamingError<F, P>),
}

pub fn change_master_type_under_pressure_with_queue_service<
    F: EffectProgramPort,
    P: MasterPressureTypeChangePort,
    S: EffectProgramSource,
    C: EffectQueueServiceInputs,
>(
    state: &mut InsertControlState,
    program: &mut Program,
    pressure_marker: &mut u32,
    operation: &mut MasterPressureTypeValueOperation,
    io: &mut MasterPressureTypeChangeIo<'_, F, P, S, C>,
    tables: MasterPressureTypeValueTables<'_>,
    request: MasterPressureTypeValueRequest<'_>,
) -> Result<i32, MasterPressureTypeValueError<F::Error, P::Error>> {
    if operation.value().is_none() {
        let stored = tables
            .properties
            .prepare(
                state,
                program,
                EffectPropertyChange {
                    target: EffectParameterTarget::Master,
                    property: EffectProperty::Kind,
                    value: request.value,
                },
            )
            .ok_or(MasterPressureTypeValueError::InvalidProperty)?;
        // SYS08017A stores before SYS07C17E can suspend. Retain this write
        // and its normalized return value across missing service inputs.
        *state = stored.next;
        *program = stored.program;
        operation.begin(stored.value);
    }
    let context = EffectRackRebuildContext {
        rack: EffectRackInitializationContext {
            common: InsertControlContext {
                program,
                ..request.rebuild.rack.common
            },
            ..request.rebuild.rack
        },
        ..request.rebuild
    };
    rebuild_master_type_with_queue_service(
        state,
        pressure_marker,
        operation.pressure_operation(),
        io,
        tables.rebuild,
        context,
    )
    .map_err(MasterPressureTypeValueError::Pressure)?;
    operation
        .value()
        .ok_or(MasterPressureTypeValueError::InvalidProperty)
}
