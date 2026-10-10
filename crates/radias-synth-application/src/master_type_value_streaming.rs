use crate::{
    effect_transition_queue::{EffectProgramSource, EffectQueueServiceInputs},
    effects::EffectProgramPort,
    master_pressure_type_change::{
        MasterPressureStreamingError, MasterPressureTypeChangeError, MasterPressureTypeChangeIo,
        MasterPressureTypeChangePort,
    },
    master_pressure_type_value::{
        MasterPressureTypeValueError, MasterPressureTypeValueTables,
        change_master_type_under_pressure_with_queue_service,
    },
    master_type_value_change::{
        MasterTypeValueError, MasterTypeValuePort, MasterTypeValueTables, change_master_type_value,
    },
};
use radias_synth_domain::{
    insert_effect_control::{InsertControlContext, InsertControlState},
    master_initial_mask::MasterInitialMaskContext,
    master_pressure_type_change::MasterPressureTypeError,
    master_pressure_type_value::MasterPressureTypeValueRequest,
    master_type_value_change::MasterTypeValueContext,
    master_type_value_streaming::{MasterTypeValueOperation, MasterTypeValuePath},
    program::Program,
};

#[derive(Debug, PartialEq, Eq)]
pub enum MasterTypeValueStreamingError<F, P> {
    Pressure(MasterPressureTypeValueError<F, P>),
    Idle(MasterTypeValueError<P>),
    MissingLatchedValue,
}

pub fn change_master_type_with_queue_service<
    F: EffectProgramPort,
    P: MasterPressureTypeChangePort
        + MasterTypeValuePort<Error = <P as MasterPressureTypeChangePort>::Error>,
    S: EffectProgramSource,
    C: EffectQueueServiceInputs,
>(
    state: &mut InsertControlState,
    program: &mut Program,
    pressure_marker: &mut u32,
    operation: &mut MasterTypeValueOperation,
    io: &mut MasterPressureTypeChangeIo<'_, F, P, S, C>,
    tables: MasterPressureTypeValueTables<'_>,
    request: MasterPressureTypeValueRequest<'_>,
) -> Result<i32, MasterTypeValueStreamingError<F::Error, <P as MasterPressureTypeChangePort>::Error>>
{
    if operation.is_complete() {
        return operation
            .value()
            .ok_or(MasterTypeValueStreamingError::MissingLatchedValue);
    }
    let result = change_master_type_under_pressure_with_queue_service(
        state,
        program,
        pressure_marker,
        operation.inner(),
        io,
        MasterPressureTypeValueTables { ..tables },
        request,
    );
    match result {
        Ok(value) => {
            operation.finish(MasterTypeValuePath::Pressure);
            Ok(value)
        }
        Err(MasterPressureTypeValueError::Pressure(MasterPressureStreamingError::Rebuild(
            MasterPressureTypeChangeError::Preparation(MasterPressureTypeError::BelowThreshold),
        ))) => {
            let value = operation
                .value()
                .ok_or(MasterTypeValueStreamingError::MissingLatchedValue)?;
            let stored = program.clone();
            let rack = request.rebuild.rack;
            let (value, _, marker) = change_master_type_value(
                state,
                program,
                io.rebuild,
                MasterTypeValueTables {
                    control: &tables.rebuild.rack.control,
                    properties: tables.properties,
                    initialization: &tables.rebuild.rack.master_coefficients,
                },
                value,
                MasterTypeValueContext {
                    parameters: MasterInitialMaskContext {
                        common: InsertControlContext {
                            program: &stored,
                            ..rack.common
                        },
                        prefix_origin: rack.master_prefix,
                        body_origin: rack.master_body,
                        relocation_origin: rack.master_relocation,
                    },
                    queue: io.queue.state(),
                    pressure_marker: *pressure_marker,
                },
            )
            .map_err(MasterTypeValueStreamingError::Idle)?;
            *pressure_marker = marker;
            operation.finish(MasterTypeValuePath::Idle);
            Ok(value)
        }
        Err(e) => Err(MasterTypeValueStreamingError::Pressure(e)),
    }
}
