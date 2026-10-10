use radias_synth_domain::{
    insert_effect_control::{InsertControlState, InsertControlTables},
    master_initial_mask::MasterInitialMaskContext,
    mixed_effect_parameter::{MixedEffectParameterEdit, PreparedMixedEffectParameter},
};
pub trait MixedEffectParameterPort {
    type Error;
    fn accept_mixed_effect_parameter(
        &mut self,
        prepared: &PreparedMixedEffectParameter,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum MixedEffectParameterError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn change_mixed_effect_parameter<P: MixedEffectParameterPort>(
    state: &mut InsertControlState,
    port: &mut P,
    tables: &InsertControlTables,
    edit: MixedEffectParameterEdit,
    context: MasterInitialMaskContext<'_>,
) -> Result<(), MixedEffectParameterError<P::Error>> {
    let prepared = tables
        .prepare_mixed_effect_parameter(state, edit, context)
        .ok_or(MixedEffectParameterError::InvalidPreparation)?;
    accept_parameter(state, port, prepared)
}
pub fn change_stored_mixed_effect_parameter<P: MixedEffectParameterPort>(
    state: &mut InsertControlState,
    port: &mut P,
    tables: &InsertControlTables,
    edit: MixedEffectParameterEdit,
    context: MasterInitialMaskContext<'_>,
) -> Result<(), MixedEffectParameterError<P::Error>> {
    let prepared = tables
        .prepare_stored_mixed_effect_parameter(state, edit, context)
        .ok_or(MixedEffectParameterError::InvalidPreparation)?;
    accept_parameter(state, port, prepared)
}
fn accept_parameter<P: MixedEffectParameterPort>(
    state: &mut InsertControlState,
    port: &mut P,
    prepared: PreparedMixedEffectParameter,
) -> Result<(), MixedEffectParameterError<P::Error>> {
    port.accept_mixed_effect_parameter(&prepared)
        .map_err(MixedEffectParameterError::Port)?;
    *state = prepared.next;
    Ok(())
}
