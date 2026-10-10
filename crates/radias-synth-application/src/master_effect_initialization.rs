use radias_synth_domain::{
    master_effect_control::{MasterControlState, MasterControlTables},
    master_effect_initialization::{
        MasterInitialization, MasterInitializationTables, PreparedMasterInitialization,
    },
};
pub trait MasterInitializationPort {
    type Error;
    fn accept_master_initialization(
        &mut self,
        prepared: &PreparedMasterInitialization,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum MasterInitializationError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn initialize_master_effect<P: MasterInitializationPort>(
    state: &mut MasterControlState,
    port: &mut P,
    controllers: &MasterControlTables,
    tables: &MasterInitializationTables,
    edit: MasterInitialization,
) -> Result<(), MasterInitializationError<P::Error>> {
    let prepared = controllers
        .prepare_master_initialization(state, tables, edit)
        .ok_or(MasterInitializationError::InvalidPreparation)?;
    port.accept_master_initialization(&prepared)
        .map_err(MasterInitializationError::Port)?;
    *state = prepared.next;
    Ok(())
}
