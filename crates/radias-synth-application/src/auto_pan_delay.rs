use crate::effect_parameters::EffectParameterQueue;
use radias_synth_domain::auto_pan_delay::{AutoPanDelayEdit, AutoPanDelayRack, AutoPanDelayTables};
#[derive(Debug, PartialEq, Eq)]
pub enum AutoPanDelayError<E> {
    ParameterValue,
    Queue(E),
}
pub fn change_auto_pan_delay<Q: EffectParameterQueue>(
    rack: &mut AutoPanDelayRack,
    queue: &mut Q,
    edit: AutoPanDelayEdit,
    tables: &AutoPanDelayTables,
) -> Result<(), AutoPanDelayError<Q::Error>> {
    let prepared = tables
        .prepare(rack, edit)
        .ok_or(AutoPanDelayError::ParameterValue)?;
    queue
        .enqueue_parameter(&prepared.batch)
        .map_err(AutoPanDelayError::Queue)?;
    *rack = prepared.next;
    Ok(())
}
