use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::{
    effect_equalizer::EffectEqualizerTables,
    equalizer_effect::{EqualizerEffectRack, EqualizerEffectTables, EqualizerParameterEdit},
    program::Program,
};
#[derive(Debug, PartialEq, Eq)]
pub enum EqualizerEffectError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_equalizer_parameter<Q: EffectParameterQueue>(
    rack: &mut EqualizerEffectRack,
    queue: &mut Q,
    edit: EqualizerParameterEdit,
    tables: &EqualizerEffectTables,
    core: &EffectEqualizerTables,
    program: &Program,
) -> Result<(), EqualizerEffectError<Q::Error>> {
    let prepared = tables
        .prepare(rack, edit, core, program)
        .ok_or(EqualizerEffectError::ParameterValue)?;
    queue
        .enqueue_parameter(&prepared.batch)
        .map_err(EqualizerEffectError::Queue)?;
    *rack = prepared.next;
    Ok(())
}
