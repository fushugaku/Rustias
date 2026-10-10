use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::{
    effect_pair_transition::EffectPairTransitionTables, effect_routing::EffectRoutingInstance,
};

#[derive(Debug, PartialEq, Eq)]
pub enum EffectPairTransitionError<E> {
    EffectKind,
    Queue(E),
}
pub fn transition_effect_pair<Q: EffectParameterQueue>(
    queue: &mut Q,
    tables: &EffectPairTransitionTables,
    instances: &[EffectRoutingInstance; 2],
    mute_argument: u32,
) -> Result<(), EffectPairTransitionError<Q::Error>> {
    let batch = tables
        .prepare(instances, mute_argument)
        .ok_or(EffectPairTransitionError::EffectKind)?;
    queue
        .enqueue_parameter(&batch)
        .map_err(EffectPairTransitionError::Queue)
}
