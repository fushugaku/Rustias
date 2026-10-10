use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::{
    effect_routing::{EffectRoutingContext, EffectRoutingInstance, EffectRoutingTables},
    program::Program,
};
#[derive(Debug, PartialEq, Eq)]
pub enum EffectRoutingError<E> {
    ParameterValue,
    Queue(E),
}
pub fn publish_effect_routing<Q: EffectParameterQueue>(
    queue: &mut Q,
    program: &Program,
    instances: &[EffectRoutingInstance; 9],
    tables: &EffectRoutingTables,
    context: EffectRoutingContext,
    reset_argument: u32,
) -> Result<(), EffectRoutingError<Q::Error>> {
    let batch = tables
        .prepare(program, instances, context, reset_argument)
        .ok_or(EffectRoutingError::ParameterValue)?;
    queue
        .enqueue_parameter(&batch)
        .map_err(EffectRoutingError::Queue)
}
pub fn mute_effect_inputs<Q: EffectParameterQueue>(
    queue: &mut Q,
    program: &Program,
    instances: &[EffectRoutingInstance; 9],
    tables: &EffectRoutingTables,
    context: EffectRoutingContext,
) -> Result<(), EffectRoutingError<Q::Error>> {
    let batch = tables
        .prepare_input_mute(program, instances, context)
        .ok_or(EffectRoutingError::ParameterValue)?;
    queue
        .enqueue_parameter(&batch)
        .map_err(EffectRoutingError::Queue)
}
