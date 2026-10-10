use radias_synth_domain::{
    master_effect_construction::{MasterEffectInstance, MasterPatch},
    master_effect_control::MasterControlTables,
    master_effect_initialization::MasterInitializationTables,
    master_effect_type_change::{
        MasterTypeChange, MasterTypeChangeError, PreparedMasterTypeChange,
    },
};
pub trait MasterTypeChangePort {
    type Error;
    fn accept_master_type_change(
        &mut self,
        prepared: &PreparedMasterTypeChange,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum MasterTypeError<E> {
    Preparation(MasterTypeChangeError),
    Port(E),
}
pub fn change_master_effect_type<P: MasterTypeChangePort>(
    instance: &mut MasterEffectInstance,
    patch: &mut MasterPatch,
    port: &mut P,
    controllers: &MasterControlTables,
    tables: &MasterInitializationTables,
    edit: MasterTypeChange,
) -> Result<(), MasterTypeError<P::Error>> {
    let plan = controllers
        .prepare_master_type_change(instance, *patch, tables, edit)
        .map_err(MasterTypeError::Preparation)?;
    port.accept_master_type_change(&plan)
        .map_err(MasterTypeError::Port)?;
    *instance = plan.next;
    *patch = plan.patch;
    Ok(())
}
