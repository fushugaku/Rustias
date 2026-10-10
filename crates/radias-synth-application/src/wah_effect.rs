use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::{
    program::Program,
    wah_effect::{WahEffectRack, WahParameterEdit, WahParameterTables},
};
#[derive(Debug, PartialEq, Eq)]
pub enum WahEffectError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_wah_parameter<Q: EffectParameterQueue>(
    rack: &mut WahEffectRack,
    queue: &mut Q,
    edit: WahParameterEdit,
    tables: &WahParameterTables<'_>,
    program: &Program,
) -> Result<(), WahEffectError<Q::Error>> {
    let prepared = tables
        .coefficients
        .prepare(rack, edit, tables, program)
        .ok_or(WahEffectError::ParameterValue)?;
    queue
        .enqueue_parameter(&prepared.batch)
        .map_err(WahEffectError::Queue)?;
    *rack = prepared.next;
    Ok(())
}
