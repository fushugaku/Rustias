use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::{
    chorus_effect::{ChorusEffectRack, ChorusEffectTables, ChorusParameterEdit},
    effect_equalizer::EffectEqualizerTables,
};
#[derive(Debug, PartialEq, Eq)]
pub enum ChorusEffectError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_chorus_parameter<Q: EffectParameterQueue>(
    rack: &mut ChorusEffectRack,
    queue: &mut Q,
    tables: &ChorusEffectTables,
    equalizer: &EffectEqualizerTables,
    edit: ChorusParameterEdit,
) -> Result<(), ChorusEffectError<Q::Error>> {
    let p = tables
        .prepare(rack, edit, equalizer)
        .ok_or(ChorusEffectError::ParameterValue)?;
    queue
        .enqueue_parameter(&p.batch)
        .map_err(ChorusEffectError::Queue)?;
    *rack = p.next;
    Ok(())
}
