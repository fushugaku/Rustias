use radias_synth_domain::{
    master_effect_construction::{
        MasterConstruction, MasterEffectInstance, MasterPatch, PreparedMasterConstruction,
    },
    master_effect_control::MasterControlTables,
};
pub trait MasterConstructionPort {
    type Error;
    fn accept_master_construction(
        &mut self,
        prepared: &PreparedMasterConstruction,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum MasterConstructionError<E> {
    InvalidDefinition,
    Port(E),
}
pub fn construct_master_effect<P: MasterConstructionPort>(
    instance: &mut MasterEffectInstance,
    patch: &mut MasterPatch,
    port: &mut P,
    tables: &MasterControlTables,
    kind: u8,
    path: MasterConstruction,
) -> Result<(), MasterConstructionError<P::Error>> {
    let prepared = tables
        .construct_master(instance, *patch, kind, path)
        .ok_or(MasterConstructionError::InvalidDefinition)?;
    port.accept_master_construction(&prepared)
        .map_err(MasterConstructionError::Port)?;
    *instance = prepared.next;
    *patch = prepared.patch;
    Ok(())
}
