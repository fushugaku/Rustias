use radias_synth_domain::insert_effect_construction::{
    InsertConstructionTables, InsertEffectInstance, InsertPatch,
};
pub trait InsertConstructionPort {
    type Error;
    fn accept_insert_construction(
        &mut self,
        prepared: &InsertEffectInstance,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum InsertConstructionError<E> {
    InvalidDefinition,
    Port(E),
}
pub fn construct_stored_insert<P: InsertConstructionPort>(
    instance: &mut InsertEffectInstance,
    patch: InsertPatch,
    kind: u8,
    tables: &InsertConstructionTables,
    port: &mut P,
) -> Result<(), InsertConstructionError<P::Error>> {
    let next = tables
        .construct_stored(instance, patch, kind)
        .ok_or(InsertConstructionError::InvalidDefinition)?;
    port.accept_insert_construction(&next)
        .map_err(InsertConstructionError::Port)?;
    *instance = next;
    Ok(())
}
