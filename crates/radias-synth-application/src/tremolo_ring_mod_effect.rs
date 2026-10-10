use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::tremolo_ring_mod_effect::{
    TremoloRingModEdit, TremoloRingModRack, TremoloRingModTables,
};
#[derive(Debug, PartialEq, Eq)]
pub enum TremoloRingModError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_tremolo_ring_mod_parameter<Q: EffectParameterQueue>(
    rack: &mut TremoloRingModRack,
    queue: &mut Q,
    tables: &TremoloRingModTables,
    edit: TremoloRingModEdit,
) -> Result<(), TremoloRingModError<Q::Error>> {
    let p = tables
        .prepare(rack, edit)
        .ok_or(TremoloRingModError::ParameterValue)?;
    queue
        .enqueue_parameter(&p.batch)
        .map_err(TremoloRingModError::Queue)?;
    *rack = p.next;
    Ok(())
}
