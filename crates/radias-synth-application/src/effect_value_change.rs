use radias_synth_domain::{
    effect_parameter_caller::EffectParameterCallerTables,
    effect_property::{EffectPropertyChange, EffectPropertyTables},
    effect_value_change::PreparedEffectValueChange,
    insert_effect_control::{InsertControlState, InsertControlTables},
    master_initial_mask::MasterInitialMaskContext,
    program::Program,
};
pub trait EffectValueChangePort {
    type Error;
    fn accept_effect_value_change(
        &mut self,
        prepared: &PreparedEffectValueChange,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum EffectValueChangeError<E> {
    InvalidPreparation,
    Port(E),
}
pub struct EffectValueChangeTables<'a> {
    pub control: &'a InsertControlTables,
    pub properties: &'a EffectPropertyTables,
    pub callers: &'a EffectParameterCallerTables,
}
pub fn change_effect_value<P: EffectValueChangePort>(
    state: &mut InsertControlState,
    program: &mut Program,
    port: &mut P,
    tables: EffectValueChangeTables<'_>,
    edit: EffectPropertyChange,
    context: MasterInitialMaskContext<'_>,
) -> Result<i32, EffectValueChangeError<P::Error>> {
    let prepared = tables
        .control
        .prepare_effect_value_change(state, tables.properties, tables.callers, edit, context)
        .ok_or(EffectValueChangeError::InvalidPreparation)?;
    port.accept_effect_value_change(&prepared)
        .map_err(EffectValueChangeError::Port)?;
    *state = prepared.call.next;
    *program = prepared.call.program;
    Ok(prepared.value)
}
