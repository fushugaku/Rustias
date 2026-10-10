use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::delay_effect::{DelayEffectRack, DelayEffectTables, DelayParameterEdit};
#[derive(Debug, PartialEq, Eq)]
pub enum DelayEffectError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_delay_parameter<Q: EffectParameterQueue>(
    rack: &mut DelayEffectRack,
    queue: &mut Q,
    edit: DelayParameterEdit,
    tables: &DelayEffectTables,
) -> Result<(), DelayEffectError<Q::Error>> {
    let prepared = tables
        .prepare(rack, edit)
        .ok_or(DelayEffectError::ParameterValue)?;
    queue
        .enqueue_parameter(&prepared.batch)
        .map_err(DelayEffectError::Queue)?;
    *rack = prepared.next;
    Ok(())
}
