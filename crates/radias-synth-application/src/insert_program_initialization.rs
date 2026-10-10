use radias_synth_domain::{
    effect_control::EffectOrigins,
    insert_effect_construction::InsertEffectInstance,
    insert_program_initialization::{
        InsertProgramInitializationTables, PreparedInsertProgramInitialization,
    },
};
pub trait InsertProgramInitializationPort {
    type Error;
    fn accept_insert_program_initialization(
        &mut self,
        prepared: &PreparedInsertProgramInitialization,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum InsertProgramInitializationError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn initialize_insert_program<P: InsertProgramInitializationPort>(
    instance: &mut InsertEffectInstance,
    port: &mut P,
    tables: &InsertProgramInitializationTables,
    origins: EffectOrigins,
) -> Result<(), InsertProgramInitializationError<P::Error>> {
    let prepared = tables
        .prepare(instance, origins)
        .ok_or(InsertProgramInitializationError::InvalidPreparation)?;
    port.accept_insert_program_initialization(&prepared)
        .map_err(InsertProgramInitializationError::Port)?;
    *instance = prepared.next;
    Ok(())
}
