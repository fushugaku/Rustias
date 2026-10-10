use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::{
    effect_midi::EffectMidiSources,
    rotary_effect::{RotaryEdit, RotaryRack, RotaryTables},
};
#[derive(Debug, PartialEq, Eq)]
pub enum RotaryError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_rotary_parameter<Q: EffectParameterQueue>(
    rack: &mut RotaryRack,
    queue: &mut Q,
    tables: &RotaryTables,
    edit: RotaryEdit,
    midi: &EffectMidiSources,
) -> Result<(), RotaryError<Q::Error>> {
    let p = tables
        .prepare(rack, edit, midi)
        .ok_or(RotaryError::ParameterValue)?;
    queue
        .enqueue_parameter(&p.batch)
        .map_err(RotaryError::Queue)?;
    *rack = p.next;
    Ok(())
}

pub fn update_rotary_midi<Q: EffectParameterQueue>(
    rack: &mut RotaryRack,
    queue: &mut Q,
    tables: &RotaryTables,
    midi: &EffectMidiSources,
) -> Result<(), RotaryError<Q::Error>> {
    let p = tables
        .prepare_midi(rack, midi)
        .ok_or(RotaryError::ParameterValue)?;
    queue
        .enqueue_parameter(&p.batch)
        .map_err(RotaryError::Queue)?;
    *rack = p.next;
    Ok(())
}
