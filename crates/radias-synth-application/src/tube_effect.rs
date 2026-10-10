use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::{
    effect_updates::EffectCoefficientAssignments,
    tube_effect::{TubeEffectTables, TubeParameterEdit},
};
#[derive(Debug, PartialEq, Eq)]
pub enum TubeEffectError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_tube_parameter<Q: EffectParameterQueue>(
    assignments: &mut EffectCoefficientAssignments,
    queue: &mut Q,
    edit: TubeParameterEdit,
    tables: &TubeEffectTables,
) -> Result<(), TubeEffectError<Q::Error>> {
    let prepared = tables
        .prepare(assignments, edit)
        .ok_or(TubeEffectError::ParameterValue)?;
    queue
        .enqueue_parameter(&prepared.batch)
        .map_err(TubeEffectError::Queue)?;
    *assignments = prepared.next;
    Ok(())
}
