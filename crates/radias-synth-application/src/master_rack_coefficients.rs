use radias_synth_domain::{
    master_effect_control::{MasterControlState, MasterControlTables},
    master_effect_initialization::MasterInitializationTables,
    master_rack_coefficients::{MasterRackCoefficientLoad, PreparedMasterRackCoefficients},
};
pub trait MasterRackCoefficientsPort {
    type Error;
    fn accept_master_rack_coefficients(
        &mut self,
        prepared: &PreparedMasterRackCoefficients,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum MasterRackCoefficientsError<E> {
    InvalidPreparation,
    Port(E),
}
pub fn load_master_rack_coefficients<P: MasterRackCoefficientsPort>(
    state: &mut MasterControlState,
    port: &mut P,
    controllers: &MasterControlTables,
    tables: &MasterInitializationTables,
    edit: MasterRackCoefficientLoad,
) -> Result<(), MasterRackCoefficientsError<P::Error>> {
    let prepared = controllers
        .prepare_master_rack_coefficients(state, tables, edit)
        .ok_or(MasterRackCoefficientsError::InvalidPreparation)?;
    port.accept_master_rack_coefficients(&prepared)
        .map_err(MasterRackCoefficientsError::Port)?;
    *state = prepared.next;
    Ok(())
}
