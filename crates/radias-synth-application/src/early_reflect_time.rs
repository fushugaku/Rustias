use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::early_reflect_time::{EarlyReflectTimeEdit, EarlyReflectTimeTables};
#[derive(Debug, PartialEq, Eq)]
pub enum EarlyReflectTimeError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_early_reflect_time<Q: EffectParameterQueue>(
    queue: &mut Q,
    edit: EarlyReflectTimeEdit,
    tables: &EarlyReflectTimeTables,
) -> Result<(), EarlyReflectTimeError<Q::Error>> {
    let batch = tables
        .prepare(edit)
        .ok_or(EarlyReflectTimeError::ParameterValue)?;
    queue
        .enqueue_parameter(&batch)
        .map_err(EarlyReflectTimeError::Queue)
}
