use radias_synth_domain::insert_effect_construction::{InsertEffectInstance, InsertPatch};
use radias_synth_domain::insert_type_construction::{
    InsertTypeConstruction, InsertTypeConstructionTables, PreparedInsertTypeConstruction,
};
pub trait InsertTypeConstructionPort {
    type Error;
    fn accept_insert_type_construction(
        &mut self,
        prepared: &PreparedInsertTypeConstruction,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum InsertTypeConstructionError<E> {
    InvalidDefinition,
    Port(E),
}
#[derive(Clone, Copy)]
pub struct InsertTypeConstructionRequest {
    pub kind: u8,
    pub mode: InsertTypeConstruction,
}
pub fn construct_insert_type<P: InsertTypeConstructionPort>(
    instance: &mut InsertEffectInstance,
    patch: &mut InsertPatch,
    tables: &InsertTypeConstructionTables,
    port: &mut P,
    request: InsertTypeConstructionRequest,
) -> Result<(), InsertTypeConstructionError<P::Error>> {
    let prepared = tables
        .prepare(instance, *patch, request.kind, request.mode)
        .ok_or(InsertTypeConstructionError::InvalidDefinition)?;
    port.accept_insert_type_construction(&prepared)
        .map_err(InsertTypeConstructionError::Port)?;
    *instance = prepared.next;
    *patch = prepared.patch;
    Ok(())
}
