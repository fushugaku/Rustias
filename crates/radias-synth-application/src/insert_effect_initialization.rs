use radias_synth_domain::insert_effect_initialization::{
    InsertInitialization, InsertInitializationState, InsertInitializationTables,
    PreparedInsertInitialization,
};
pub trait InsertInitializationPort {
    type Error;
    fn accept_insert_initialization(
        &mut self,
        prepared: &PreparedInsertInitialization,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum InsertInitializationError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn initialize_insert_effect<P: InsertInitializationPort>(
    state: &mut InsertInitializationState,
    port: &mut P,
    tables: &InsertInitializationTables,
    edit: InsertInitialization,
) -> Result<(), InsertInitializationError<P::Error>> {
    let prepared = tables
        .prepare(state, edit)
        .ok_or(InsertInitializationError::InvalidPreparation)?;
    port.accept_insert_initialization(&prepared)
        .map_err(InsertInitializationError::Port)?;
    *state = prepared.next;
    Ok(())
}
