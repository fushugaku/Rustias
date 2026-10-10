use crate::effect_parameters::EffectParameterQueue;
#[derive(Debug, PartialEq, Eq)]
pub enum EffectModulationError<E> {
    ParameterValue,
    Queue(E),
}
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct EffectModulationEvaluation {
    pub evaluated: u16,
    pub pairs: [[i32; 2]; 9],
}
pub fn update_effect_modulation<Q: EffectParameterQueue>(
    rack: &mut radias_synth_domain::effect_modulation::EffectModulationRack,
    queue: &mut Q,
    program: &radias_synth_domain::program::Program,
    states: [radias_synth_domain::effect_lfo_values::EffectLfoValueState; 9],
    tables: &radias_synth_domain::effect_modulation::EffectModulationTables<'_>,
    direct_switch: u32,
) -> Result<EffectModulationEvaluation, EffectModulationError<Q::Error>> {
    let prepared = rack
        .prepare(program, states, tables, direct_switch)
        .ok_or(EffectModulationError::ParameterValue)?;
    queue
        .enqueue_parameter(&prepared.batch)
        .map_err(EffectModulationError::Queue)?;
    *rack = prepared.next;
    Ok(EffectModulationEvaluation {
        evaluated: prepared.evaluated,
        pairs: prepared.computed_pairs,
    })
}
/// Read the live synthesis pool's own phase/random state. No source snapshots
/// or recorded waveform/control targets enter this production bridge.
pub fn update_effect_modulation_from_pool<Q: EffectParameterQueue>(
    pool: &crate::polyphony::PolyphonicRenderer,
    rack: &mut radias_synth_domain::effect_modulation::EffectModulationRack,
    queue: &mut Q,
    program: &radias_synth_domain::program::Program,
    tables: &radias_synth_domain::effect_modulation::EffectModulationTables<'_>,
    direct_switch: u32,
) -> Result<EffectModulationEvaluation, EffectModulationError<Q::Error>> {
    update_effect_modulation(
        rack,
        queue,
        program,
        pool.effect_lfo_value_states(),
        tables,
        direct_switch,
    )
}
