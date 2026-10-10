use radias_synth_domain::{
    insert_effect_control::{InsertControlState, InsertControlTables},
    master_effect_construction::MasterPatch,
    master_initial_mask::{MasterInitialMaskContext, PreparedMasterInitialMask},
};
pub trait MasterInitialMaskPort {
    type Error;
    fn accept_master_initial_mask(
        &mut self,
        prepared: &PreparedMasterInitialMask,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum MasterInitialMaskError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn initialize_master_parameters<P: MasterInitialMaskPort>(
    state: &mut InsertControlState,
    port: &mut P,
    tables: &InsertControlTables,
    patch: MasterPatch,
    context: MasterInitialMaskContext<'_>,
) -> Result<(), MasterInitialMaskError<P::Error>> {
    let prepared = tables
        .prepare_master_initial_mask(state, patch, context)
        .ok_or(MasterInitialMaskError::InvalidPreparation)?;
    port.accept_master_initial_mask(&prepared)
        .map_err(MasterInitialMaskError::Port)?;
    *state = prepared.next;
    Ok(())
}
