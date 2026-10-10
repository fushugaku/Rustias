use radias_synth_domain::{
    effect_rack_initialization::{
        EffectRackInitializationContext, EffectRackInitializationTables,
        PreparedEffectRackInitialization,
    },
    insert_effect_control::InsertControlState,
};
pub trait EffectRackInitializationPort {
    type Error;
    fn accept_effect_rack_initialization(
        &mut self,
        prepared: &PreparedEffectRackInitialization,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum EffectRackInitializationError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn initialize_effect_rack<P: EffectRackInitializationPort>(
    state: &mut InsertControlState,
    port: &mut P,
    tables: &EffectRackInitializationTables,
    context: EffectRackInitializationContext<'_>,
) -> Result<(), EffectRackInitializationError<P::Error>> {
    let prepared = tables
        .prepare(state, context)
        .ok_or(EffectRackInitializationError::InvalidPreparation)?;
    port.accept_effect_rack_initialization(&prepared)
        .map_err(EffectRackInitializationError::Port)?;
    *state = prepared.next;
    Ok(())
}
