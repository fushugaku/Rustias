use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::reverb_time::{ReverbTimeEdit, ReverbTimeTables};
#[derive(Debug, PartialEq, Eq)]
pub enum ReverbTimeError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_reverb_time<Q: EffectParameterQueue>(
    queue: &mut Q,
    edit: ReverbTimeEdit,
    tables: &ReverbTimeTables,
) -> Result<(), ReverbTimeError<Q::Error>> {
    let batch = tables
        .prepare(edit)
        .ok_or(ReverbTimeError::ParameterValue)?;
    queue
        .enqueue_parameter(&batch)
        .map_err(ReverbTimeError::Queue)
}
