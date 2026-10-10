use radias_synth_domain::{
    insert_effect_construction::InsertPatch,
    insert_effect_control::{
        InsertControlContext, InsertControlState, InsertControlTables, PreparedInsertInitialMask,
    },
};
pub trait InsertInitialMaskPort {
    type Error;
    fn accept_insert_initial_mask(
        &mut self,
        prepared: &PreparedInsertInitialMask,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum InsertInitialMaskError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn initialize_insert_parameters<P: InsertInitialMaskPort>(
    state: &mut InsertControlState,
    port: &mut P,
    tables: &InsertControlTables,
    slot: u8,
    patch: InsertPatch,
    context: InsertControlContext<'_>,
) -> Result<(), InsertInitialMaskError<P::Error>> {
    let prepared = tables
        .prepare_initial_mask(state, slot, patch, context)
        .ok_or(InsertInitialMaskError::InvalidPreparation)?;
    port.accept_insert_initial_mask(&prepared)
        .map_err(InsertInitialMaskError::Port)?;
    *state = prepared.next;
    Ok(())
}
