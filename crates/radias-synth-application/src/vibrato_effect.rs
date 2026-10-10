use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::vibrato_effect::{VibratoEdit, VibratoRack, VibratoTables};
#[derive(Debug, PartialEq, Eq)]
pub enum VibratoError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_vibrato_parameter<Q: EffectParameterQueue>(
    rack: &mut VibratoRack,
    queue: &mut Q,
    tables: &VibratoTables,
    edit: VibratoEdit,
) -> Result<(), VibratoError<Q::Error>> {
    let p = tables
        .prepare(rack, edit)
        .ok_or(VibratoError::ParameterValue)?;
    queue
        .enqueue_parameter(&p.batch)
        .map_err(VibratoError::Queue)?;
    *rack = p.next;
    Ok(())
}
