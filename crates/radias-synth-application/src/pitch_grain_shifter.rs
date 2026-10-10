use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::pitch_grain_shifter::{PitchGrainEdit, PitchGrainRack, PitchGrainTables};
#[derive(Debug, PartialEq, Eq)]
pub enum PitchGrainError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_pitch_grain_parameter<Q: EffectParameterQueue>(
    rack: &mut PitchGrainRack,
    queue: &mut Q,
    tables: &PitchGrainTables,
    edit: PitchGrainEdit,
) -> Result<(), PitchGrainError<Q::Error>> {
    let p = tables
        .prepare(rack, edit)
        .ok_or(PitchGrainError::ParameterValue)?;
    queue
        .enqueue_parameter(&p.batch)
        .map_err(PitchGrainError::Queue)?;
    *rack = p.next;
    Ok(())
}
