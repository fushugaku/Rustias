use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::{
    cabinet_effect::{CabinetEffectRack, CabinetEffectTables, CabinetParameterEdit},
    effect_routing::EffectRoutingTables,
    program::Program,
};
#[derive(Debug, PartialEq, Eq)]
pub enum CabinetEffectError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_cabinet_parameter<Q: EffectParameterQueue>(
    rack: &mut CabinetEffectRack,
    queue: &mut Q,
    edit: CabinetParameterEdit,
    tables: &CabinetEffectTables,
    routing: &EffectRoutingTables,
    program: &Program,
) -> Result<(), CabinetEffectError<Q::Error>> {
    let prepared = tables
        .prepare(rack, edit, routing, program)
        .ok_or(CabinetEffectError::ParameterValue)?;
    queue
        .enqueue_parameter(&prepared.batch)
        .map_err(CabinetEffectError::Queue)?;
    *rack = prepared.next;
    Ok(())
}
