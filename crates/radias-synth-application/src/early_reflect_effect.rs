use radias_synth_domain::{
    early_reflect_effect::{
        EarlyReflectEffectRack, EarlyReflectEffectTables, EarlyReflectParameterEdit,
        PreparedEarlyReflectEdit,
    },
    program::Program,
};
pub trait EarlyReflectEffectPort {
    type Error;
    /// Accept program writes and all ordered commands together before committing.
    fn accept_early_reflect_edit(
        &mut self,
        edit: &PreparedEarlyReflectEdit,
    ) -> Result<(), Self::Error>;
}
#[derive(Debug, PartialEq, Eq)]
pub enum EarlyReflectEffectError<E> {
    ParameterValue,
    Port(E),
}
pub fn change_early_reflect_parameter<P: EarlyReflectEffectPort>(
    port: &mut P,
    rack: &mut EarlyReflectEffectRack,
    tables: &EarlyReflectEffectTables,
    program: &Program,
    edit: EarlyReflectParameterEdit,
) -> Result<(), EarlyReflectEffectError<P::Error>> {
    let prepared = tables
        .prepare(rack, program, edit)
        .ok_or(EarlyReflectEffectError::ParameterValue)?;
    port.accept_early_reflect_edit(&prepared)
        .map_err(EarlyReflectEffectError::Port)?;
    *rack = prepared.next;
    Ok(())
}
