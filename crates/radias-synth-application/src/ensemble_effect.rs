use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::{
    effect_updates::EffectCoefficientAssignments,
    ensemble_effect::{EnsembleEffectTables, EnsembleParameterEdit},
};
#[derive(Debug, PartialEq, Eq)]
pub enum EnsembleEffectError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_ensemble_parameter<Q: EffectParameterQueue>(
    assignments: &mut EffectCoefficientAssignments,
    queue: &mut Q,
    tables: &EnsembleEffectTables,
    edit: EnsembleParameterEdit,
) -> Result<(), EnsembleEffectError<Q::Error>> {
    let p = tables
        .prepare(assignments, edit)
        .ok_or(EnsembleEffectError::ParameterValue)?;
    queue
        .enqueue_parameter(&p.batch)
        .map_err(EnsembleEffectError::Queue)?;
    *assignments = p.next;
    Ok(())
}
