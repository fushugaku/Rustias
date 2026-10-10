use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::reverb_effect::{
    ReverbEffectRack, ReverbEffectTables, ReverbParameterEdit,
};
#[derive(Debug, PartialEq, Eq)]
pub enum ReverbEffectError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_reverb_parameter<Q: EffectParameterQueue>(
    rack: &mut ReverbEffectRack,
    queue: &mut Q,
    edit: ReverbParameterEdit,
    tables: &ReverbEffectTables,
) -> Result<(), ReverbEffectError<Q::Error>> {
    let p = tables
        .prepare(rack, edit)
        .ok_or(ReverbEffectError::ParameterValue)?;
    queue
        .enqueue_parameter(&p.batch)
        .map_err(ReverbEffectError::Queue)?;
    *rack = p.next;
    Ok(())
}
