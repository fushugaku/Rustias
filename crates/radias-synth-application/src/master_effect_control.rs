use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::master_effect_control::{
    MasterControlState, MasterControlTables, MasterEdit, PreparedMasterEdit,
};
#[derive(Debug, PartialEq, Eq)]
pub enum MasterControlError<E> {
    ParameterValue,
    ProgramPortRequired,
    Queue(E),
}
pub fn change_master_parameter<Q: EffectParameterQueue>(
    state: &mut MasterControlState,
    queue: &mut Q,
    tables: &MasterControlTables,
    edit: MasterEdit,
) -> Result<(), MasterControlError<Q::Error>> {
    let p = tables
        .prepare(state, edit)
        .ok_or(MasterControlError::ParameterValue)?;
    if p.program_writes.iter().any(Option::is_some) || p.body_program.is_some() {
        return Err(MasterControlError::ProgramPortRequired);
    }
    queue
        .enqueue_parameter(&p.batch)
        .map_err(MasterControlError::Queue)?;
    *state = p.next;
    Ok(())
}
pub trait MasterEffectPort {
    type Error;
    fn accept_master_edit(&mut self, edit: &PreparedMasterEdit) -> Result<(), Self::Error>;
}
pub fn change_master_effect<P: MasterEffectPort>(
    state: &mut MasterControlState,
    port: &mut P,
    tables: &MasterControlTables,
    edit: MasterEdit,
) -> Result<(), MasterControlError<P::Error>> {
    let prepared = tables
        .prepare(state, edit)
        .ok_or(MasterControlError::ParameterValue)?;
    port.accept_master_edit(&prepared)
        .map_err(MasterControlError::Queue)?;
    *state = prepared.next;
    Ok(())
}
pub fn update_master_rotary_midi<Q: EffectParameterQueue>(
    state: &mut MasterControlState,
    queue: &mut Q,
    tables: &MasterControlTables,
    edit: MasterEdit,
) -> Result<(), MasterControlError<Q::Error>> {
    let prepared = tables
        .prepare_rotary_midi(state, edit)
        .ok_or(MasterControlError::ParameterValue)?;
    queue
        .enqueue_parameter(&prepared.batch)
        .map_err(MasterControlError::Queue)?;
    *state = prepared.next;
    Ok(())
}
pub fn update_master_talking_midi<Q: EffectParameterQueue>(
    state: &mut MasterControlState,
    queue: &mut Q,
    tables: &MasterControlTables,
    edit: MasterEdit,
) -> Result<(), MasterControlError<Q::Error>> {
    let prepared = tables
        .prepare_talking_midi(state, edit)
        .ok_or(MasterControlError::ParameterValue)?;
    queue
        .enqueue_parameter(&prepared.batch)
        .map_err(MasterControlError::Queue)?;
    *state = prepared.next;
    Ok(())
}
