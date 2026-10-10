use radias_synth_domain::{
    effect_header_event::EffectHeaderChange,
    effect_parameter_caller::PreparedEffectParameterCaller,
    insert_effect_control::{InsertControlState, InsertControlTables},
    program::Program,
};
pub trait EffectHeaderEventPort {
    type Error;
    fn accept_effect_header_event(
        &mut self,
        prepared: &PreparedEffectParameterCaller,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum EffectHeaderEventError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn change_effect_header<P: EffectHeaderEventPort>(
    state: &mut InsertControlState,
    port: &mut P,
    tables: &InsertControlTables,
    program: &Program,
    edit: EffectHeaderChange,
    direct_switch: u32,
) -> Result<(), EffectHeaderEventError<P::Error>> {
    let prepared = tables
        .prepare_effect_header_event(state, program, edit, direct_switch)
        .ok_or(EffectHeaderEventError::InvalidPreparation)?;
    port.accept_effect_header_event(&prepared)
        .map_err(EffectHeaderEventError::Port)?;
    *state = prepared.next;
    Ok(())
}
