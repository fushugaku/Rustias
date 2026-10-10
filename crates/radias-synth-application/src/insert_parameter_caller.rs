use radias_synth_domain::{
    effect_parameter_caller::{EffectParameterCallerTables, PreparedEffectParameterCaller},
    insert_effect_control::{InsertControlContext, InsertControlState, InsertControlTables},
    insert_parameter_caller::InsertParameterChange,
    program::Program,
};
pub trait InsertParameterCallerPort {
    type Error;
    fn accept_insert_parameter_caller(
        &mut self,
        prepared: &PreparedEffectParameterCaller,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum InsertParameterCallerError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn change_insert_parameter<P: InsertParameterCallerPort>(
    state: &mut InsertControlState,
    program: &mut Program,
    port: &mut P,
    control: &InsertControlTables,
    tables: &EffectParameterCallerTables,
    edit: InsertParameterChange,
    context: InsertControlContext<'_>,
) -> Result<(), InsertParameterCallerError<P::Error>> {
    let prepared = control
        .prepare_insert_parameter_caller(state, edit, context, tables)
        .ok_or(InsertParameterCallerError::InvalidPreparation)?;
    port.accept_insert_parameter_caller(&prepared)
        .map_err(InsertParameterCallerError::Port)?;
    *state = prepared.next;
    *program = prepared.program;
    Ok(())
}
