use radias_synth_domain::{
    insert_effect_control::{InsertControlState, InsertControlTables},
    master_initial_mask::MasterInitialMaskContext,
    master_parameter_caller::{MasterParameterCallerTables, PreparedMasterParameterCaller},
    program::Program,
};
pub trait MasterParameterCallerPort {
    type Error;
    fn accept_master_parameter_caller(
        &mut self,
        prepared: &PreparedMasterParameterCaller,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum MasterParameterCallerError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn change_master_parameter<P: MasterParameterCallerPort>(
    state: &mut InsertControlState,
    program: &mut Program,
    port: &mut P,
    control: &InsertControlTables,
    tables: &MasterParameterCallerTables,
    parameter: u8,
    context: MasterInitialMaskContext<'_>,
) -> Result<(), MasterParameterCallerError<P::Error>> {
    let prepared = control
        .prepare_master_parameter_caller(state, parameter, context, tables)
        .ok_or(MasterParameterCallerError::InvalidPreparation)?;
    port.accept_master_parameter_caller(&prepared)
        .map_err(MasterParameterCallerError::Port)?;
    *state = prepared.next;
    *program = prepared.program;
    Ok(())
}
