use radias_synth_domain::{
    insert_effect_control::{InsertControlContext, InsertControlState, InsertControlTables},
    insert_effect_initialization::InsertInitializationTables,
    insert_paired_initialization::PreparedInsertPairedInitialization,
    insert_program_initialization::InsertProgramInitializationTables,
};
pub trait InsertPairedInitializationPort {
    type Error;
    fn accept_insert_paired_initialization(
        &mut self,
        prepared: &PreparedInsertPairedInitialization,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum InsertPairedInitializationError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn initialize_insert_pair<P: InsertPairedInitializationPort>(
    state: &mut InsertControlState,
    port: &mut P,
    control: &InsertControlTables,
    initialization: &InsertInitializationTables,
    programs: &InsertProgramInitializationTables,
    slot: u8,
    context: InsertControlContext<'_>,
) -> Result<bool, InsertPairedInitializationError<P::Error>> {
    let prepared = control
        .prepare_paired_insert_initialization(state, initialization, programs, slot, context)
        .ok_or(InsertPairedInitializationError::InvalidPreparation)?;
    port.accept_insert_paired_initialization(&prepared)
        .map_err(InsertPairedInitializationError::Port)?;
    *state = prepared.next;
    Ok(prepared.peer_initialized)
}
