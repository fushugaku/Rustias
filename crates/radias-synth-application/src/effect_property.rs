use radias_synth_domain::{
    effect_property::{EffectPropertyChange, EffectPropertyTables, PreparedEffectProperty},
    insert_effect_control::InsertControlState,
    program::Program,
};
pub trait EffectPropertyPort {
    type Error;
    fn accept_effect_property(
        &mut self,
        prepared: &PreparedEffectProperty,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum EffectPropertyError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn set_effect_property<P: EffectPropertyPort>(
    state: &mut InsertControlState,
    program: &mut Program,
    port: &mut P,
    tables: &EffectPropertyTables,
    edit: EffectPropertyChange,
) -> Result<i32, EffectPropertyError<P::Error>> {
    let prepared = tables
        .prepare(state, program, edit)
        .ok_or(EffectPropertyError::InvalidPreparation)?;
    port.accept_effect_property(&prepared)
        .map_err(EffectPropertyError::Port)?;
    *state = prepared.next;
    *program = prepared.program;
    Ok(prepared.value)
}
